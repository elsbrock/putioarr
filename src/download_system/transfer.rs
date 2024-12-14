use crate::{
    services::putio::{self, PutIOTransfer},
    AppData,
};
use actix_web::web::Data;
use anyhow::Result;
use async_channel::Sender;
use async_recursion::async_recursion;
use colored::*;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::{fmt::Display, path::Path};
use tokio::time::sleep;

#[derive(Clone)]
pub struct Transfer {
    pub name: String,
    pub file_id: Option<u64>,
    pub hash: Option<String>,
    pub transfer_id: u64,
    pub targets: Option<Vec<DownloadTarget>>,
    pub app_data: Data<AppData>,
}

impl Transfer {
    pub async fn get_download_targets(&self) -> Result<Vec<DownloadTarget>> {
        info!("{}: generating targets", self);
        let default = "0000".to_string();
        let hash = self.hash.as_ref().unwrap_or(&default).as_str();
        recurse_download_targets(&self.app_data, self.file_id.unwrap(), hash, None, true).await
    }

    pub fn get_top_level(&self) -> DownloadTarget {
        self.targets
            .clone()
            .unwrap()
            .into_iter()
            .find(|t| t.top_level)
            .unwrap()
    }

    pub fn from(app_data: Data<AppData>, transfer: &PutIOTransfer) -> Self {
        let name = &transfer.name;
        Self {
            transfer_id: transfer.id,
            name: name.clone(),
            file_id: transfer.file_id,
            targets: None,
            hash: transfer.hash.clone(),
            app_data,
        }
    }
}

impl Display for Transfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let default = "0000".to_string();
        let hash = &self.hash.as_ref().unwrap_or(&default)[..4];
        let s = format!("[{}: {}]", hash, self.name).cyan();
        write!(f, "{s}")
    }
}

#[async_recursion]
async fn recurse_download_targets(
    app_data: &Data<AppData>,
    file_id: u64,
    hash: &str,
    override_base_path: Option<String>,
    top_level: bool,
) -> Result<Vec<DownloadTarget>> {
    let base_path = "."; //override_base_path.unwrap_or(app_data.config.download_directory.clone());
    let mut targets = Vec::<DownloadTarget>::new();
    let response = putio::list_files(&app_data.config.putio.api_key, file_id).await?;
    let to = Path::new(&base_path)
        .join(&response.parent.name)
        .to_string_lossy()
        .to_string();

    match response.parent.file_type.as_str() {
        "FOLDER" => {
            if !app_data
                .config
                .skip_directories
                .contains(&response.parent.name.to_lowercase())
            {
                let new_base_path = to.clone();

                targets.push(DownloadTarget {
                    from: None,
                    target_type: TargetType::Directory,
                    to,
                    top_level,
                    transfer_hash: hash.to_string(),
                });

                for file in response.files {
                    targets.append(
                        &mut recurse_download_targets(
                            app_data,
                            file.id,
                            hash,
                            Some(new_base_path.clone()),
                            false,
                        )
                        .await?,
                    );
                }
            }
        }
        "VIDEO" => {
            // Get download URL for file
            let url = putio::url(&app_data.config.putio.api_key, response.parent.id).await?;
            targets.push(DownloadTarget {
                from: Some(url),
                target_type: TargetType::File,
                to,
                top_level,
                transfer_hash: hash.to_string(),
            });
        }
        _ => {}
    }

    Ok(targets)
}

#[derive(Clone)]
pub enum TransferMessage {
    QueuedForDownload(Transfer),
    Downloaded(Transfer),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadTarget {
    pub from: Option<String>,
    pub to: String,
    pub target_type: TargetType,
    pub top_level: bool,
    pub transfer_hash: String,
}

impl Display for DownloadTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hash = &self.transfer_hash.as_str()[..4];
        let s = format!("[{}: {}]", hash, self.to).magenta();
        write!(f, "{s}")
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Eq, PartialEq)]
pub enum TargetType {
    Directory,
    File,
}

/// Monitors Put.io transfers and manages the download/import pipeline
///
/// This function runs in an infinite loop and performs the following:
/// 1. Initially checks for any unfinished transfers that may need importing
/// 2. Maintains a list of seen transfer IDs to avoid re-processing
/// 3. Polls Put.io API at configured intervals to check for new transfers
/// 4. When a new downloadable transfer is found:
///    - Queues it for download by sending QueuedForDownload message
///    - Marks it as seen to avoid duplicate processing
/// 5. Cleans up the seen transfers list by removing completed/deleted transfers
///
/// # Arguments
/// * `app_data` - Application configuration and state
/// * `tx` - Channel sender for communicating transfer status updates
///
/// # Returns
/// Result indicating success or failure of the monitoring process
pub async fn produce_transfers(app_data: Data<AppData>, tx: Sender<TransferMessage>) -> Result<()> {
    let putio_check_interval = std::time::Duration::from_secs(app_data.config.polling_interval);
    let mut seen = Vec::<u64>::new();

    info!("Checking unfinished transfers");
    // We only need to check if something has been imported. Just by looking at the filesystem we
    // can't determine if a transfer has been imported and removed or hasn't been downloaded.
    // This avoids downloading a tranfer that has already been imported. In case there is a download,
    // but it wasn't (completely) imported, we will attempt a (partial) download. Files that have
    // been completed downloading will be skipped.
    let target_folder_id = {
        let folder_id = app_data.root_folder_id.read().unwrap();
        *folder_id
    };

    for putio_transfer in &putio::list_transfers(&app_data.config.putio.api_key)
        .await?
        .transfers
        .iter()
        .filter(|t| t.save_parent_id == Some(target_folder_id))
        .collect::<Vec<&PutIOTransfer>>()
    {
        let mut transfer = Transfer::from(app_data.clone(), putio_transfer);
        if putio_transfer.is_downloadable() {
            let targets = transfer.get_download_targets().await?;
            transfer.targets = Some(targets);
        }
    }
    info!("Done checking for unfinished transfers. Starting to monitor transfers.");

    // Set the start time
    let mut start = std::time::Instant::now();

    loop {
        if let Ok(list_transfer_response) =
            putio::list_transfers(&app_data.config.putio.api_key).await
        {
            for putio_transfer in &list_transfer_response.transfers {
                if seen.contains(&putio_transfer.id) || !putio_transfer.is_downloadable() {
                    continue;
                }
                let transfer = Transfer::from(app_data.clone(), putio_transfer);

                info!("{}: ready for download", transfer);
                tx.send(TransferMessage::QueuedForDownload(transfer))
                    .await?;
                seen.push(putio_transfer.id);
            }

            // Remove any transfers from seen that are not in the active transfers
            let active_ids: Vec<u64> = list_transfer_response
                .transfers
                .iter()
                .map(|t| t.id)
                .collect();
            seen.retain(|t| active_ids.contains(t));

            // Log status when 60 seconds have passed since last time
            if start.elapsed().as_secs() >= 60 {
                info!(
                    "Active transfers: {}",
                    list_transfer_response.transfers.len()
                );
                list_transfer_response
                    .transfers
                    .iter()
                    .for_each(|t| info!("  {}", Transfer::from(app_data.clone(), t)));

                start = std::time::Instant::now();
            }

            sleep(putio_check_interval).await;
        } else {
            warn!("List put.io transfers failed. Retrying..");
            continue;
        };
    }
}
