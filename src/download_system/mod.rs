/// Module for managing the download system and its components.
/// Contains submodules for download, orchestration, and transfer functionality.
use crate::AppData;
use actix_web::web::Data;
use anyhow::Result;

pub mod download;
pub mod orchestration;
pub mod transfer;

/// Starts the download system by initializing workers and communication channels.
///
/// # Arguments
/// * `app_data` - Shared application data wrapped in an Actix Data container
///
/// # Returns
/// * `Result<()>` - Ok if the system starts successfully
pub async fn start(app_data: Data<AppData>) -> Result<()> {
    let (sender, receiver) = async_channel::unbounded();
    let (download_sender, download_receiver) = async_channel::unbounded();
    let data = app_data.clone();
    let tx = sender.clone();
    actix_rt::spawn(async { transfer::produce_transfers(data, tx).await });

    for id in 0..app_data.config.orchestration_workers {
        let data = app_data.clone();
        let tx = sender.clone();
        let rx = receiver.clone();
        let dtx = download_sender.clone();
        orchestration::Worker::start(id, data, tx, rx, dtx);
    }

    for id in 0..app_data.config.download_workers {
        let drx = download_receiver.clone();
        let data = app_data.clone();
        download::Worker::start(id, data, drx)
    }

    Ok(())
}
