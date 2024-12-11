// This module handles the orchestration of downloads and transfers in the system.
// It includes functionality for managing workers, monitoring downloads,
// and handling the lifecycle of transfers from download to seeding.

use crate::{
    download_system::{
        download::{DownloadDoneStatus, DownloadTargetMessage},
        transfer::Transfer,
    },
    services::putio::{self, PutIOTransferStatus},
    AppData,
};
use actix_web::web::Data;
use anyhow::Result;
use async_channel::{Receiver, Sender};
use colored::*;
use log::{info, warn};
use std::{fs, time::Duration};
use tokio::{fs::metadata, time::sleep};

use super::transfer::TransferMessage;

/// Worker structure responsible for handling download and transfer operations
#[derive(Clone)]
pub struct Worker {
    _id: usize,
    app_data: Data<AppData>,
    tx: Sender<TransferMessage>,
    rx: Receiver<TransferMessage>,
    dtx: Sender<DownloadTargetMessage>,
}

impl Worker {
    /// Starts a new worker with the given parameters
    pub fn start(
        id: usize,
        app_data: Data<AppData>,
        tx: Sender<TransferMessage>,
        rx: Receiver<TransferMessage>,
        dtx: Sender<DownloadTargetMessage>,
    ) {
        let s = Self {
            _id: id,
            app_data,
            tx,
            rx,
            dtx,
        };
        let _join_handle = actix_rt::spawn(async move { s.work().await });
    }

    /// Main worker loop that processes incoming transfer messages
    async fn work(&self) -> Result<()> {
        loop {
            let msg = self.rx.recv().await?;
            let app_data = self.app_data.clone();
            match msg {
                // Handle downloads that are queued
                TransferMessage::QueuedForDownload(t) => {
                    info!("{}: download {}", t, "started".yellow());
                    let targets = t.get_download_targets().await?;
                    // Create a communications channel for the download worker to communicate status back.
                    let done_channels: &Vec<(
                        Sender<DownloadDoneStatus>,
                        Receiver<DownloadDoneStatus>,
                    )> = &targets.iter().map(|_| async_channel::unbounded()).collect();

                    // Send download targets to workers
                    for (i, target) in targets.iter().enumerate() {
                        let (done_tx, _) = done_channels[i].clone();
                        self.dtx
                            .send(DownloadTargetMessage {
                                download_target: target.clone(),
                                tx: done_tx,
                            })
                            .await?;
                    }

                    // Wait for all the workers having sent back their status.
                    let mut all_downloaded = vec![];
                    for (_, done_rx) in done_channels {
                        all_downloaded.push(done_rx.recv().await?);
                    }

                    // Check if all downloads were successful
                    if all_downloaded.iter().all(|d| match d {
                        DownloadDoneStatus::Success(_) => true,
                        DownloadDoneStatus::Failed(_) => false,
                    }) {
                        info!("{}: download {}", t, "done".blue());
                        self.tx
                            .send(TransferMessage::Downloaded(Transfer {
                                targets: Some(targets),
                                ..t
                            }))
                            .await?;
                    } else {
                        // TODO: figure out what to do here..
                        warn!("{}: not all targets downloaded", t)
                    }
                }
                // Handle completed downloads
                TransferMessage::Downloaded(t) => {
                    let tx = self.tx.clone();
                    actix_rt::spawn(async { watch_for_import(app_data, tx, t).await });
                }
                // Handle imported transfers
                TransferMessage::Imported(t) => {
                    actix_rt::spawn(async { watch_seeding(app_data, t).await });
                }
            }
        }
    }
}

/// Monitors a transfer for import completion and cleanup
async fn watch_for_import(
    app_data: Data<AppData>,
    tx: Sender<TransferMessage>,
    transfer: Transfer,
) -> Result<()> {
    info!("{}: watching imports", transfer);
    loop {
        if transfer.is_imported().await {
            info!("{}: imported", transfer);
            let top_level_target = transfer.get_top_level();

            // Clean up local files after import
            match metadata(&top_level_target.to).await {
                Ok(m) if m.is_dir() => {
                    fs::remove_dir_all(&top_level_target.to).unwrap();
                    info!("{}: deleted", &top_level_target);
                }
                Ok(m) if m.is_file() => {
                    fs::remove_file(&top_level_target.to).unwrap();
                    info!("{}: deleted", &top_level_target);
                }
                Ok(_) | Err(_) => {
                    panic!("{}: no idea how to handle", &top_level_target)
                }
            };
            let m = transfer.clone();
            tx.send(TransferMessage::Imported(m)).await?;

            break;
        }
        sleep(Duration::from_secs(app_data.config.polling_interval)).await;
    }
    info!("{}: removed", transfer);
    Ok(())
}

/// Monitors a transfer's seeding status and handles cleanup
async fn watch_seeding(app_data: Data<AppData>, transfer: Transfer) -> Result<()> {
    info!("{}: watching seeding", transfer);
    loop {
        let putio_transfer =
            putio::get_transfer(&app_data.config.putio.api_key, transfer.transfer_id)
                .await?
                .transfer;
        // Check if seeding has stopped
        if putio_transfer.status != PutIOTransferStatus::Seeding {
            info!("{}: stopped seeding", transfer);
            // Clean up remote resources
            putio::remove_transfer(&app_data.config.putio.api_key, transfer.transfer_id).await?;
            info!("{}: removed from put.io", transfer);
            match putio::delete_file(&app_data.config.putio.api_key, transfer.file_id.unwrap())
                .await
            {
                Ok(_) => {
                    info!("{}: deleted remote files", transfer);
                }
                Err(_) => {
                    warn!("{}: unable to delete remote files", transfer);
                }
            };
            break;
        }
        sleep(Duration::from_secs(app_data.config.polling_interval)).await;
    }

    info!("{}: done seeding", transfer);
    Ok(())
}
