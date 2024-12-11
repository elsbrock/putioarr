use anyhow::{bail, Result};
use reqwest::multipart;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, time::Duration};

#[derive(Debug, Serialize, Deserialize)]
pub struct PutIOAccountInfo {
    pub username: String,
    pub mail: String,
    pub account_active: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PutIOAccountResponse {
    pub info: PutIOAccountInfo,
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PutIOTransferStatus {
    InQueue,
    Waiting,
    PreparingDownload,
    Downloading,
    Completing,
    Seeding,
    Completed,
    Error,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PutIOTransferType {
    Torrent,
    Url,
    Playlist,
    LiveStream,
    #[serde(rename = "N/A")]
    NA,
}

#[derive(Debug, Deserialize)]
pub struct PutIOTransfer {
    pub availability: Option<u8>,
    pub callback_url: Option<String>,
    pub client_ip: Option<String>,
    pub completion_percent: Option<u8>,
    pub created_at: String,
    pub created_torrent: bool,
    pub current_ratio: Option<f32>,
    pub down_speed: Option<i64>,
    pub download_id: Option<u64>,
    pub downloaded: Option<i64>,
    pub error_message: Option<String>,
    pub estimated_time: Option<u64>,
    pub file_id: Option<u64>,
    pub finished_at: Option<String>,
    pub hash: Option<String>,
    pub id: u64,
    pub is_private: bool,
    pub name: String,
    pub peers_connected: Option<u32>,
    pub peers_getting_from_us: Option<u32>,
    pub peers_sending_to_us: Option<u32>,
    pub percent_done: Option<u8>,
    pub save_parent_id: Option<u64>,
    pub seconds_seeding: Option<u64>,
    pub simulated: bool,
    pub size: Option<i64>,
    pub source: Option<String>,
    pub started_at: Option<String>,
    pub status: PutIOTransferStatus,
    pub subscription_id: Option<u64>,
    pub torrent_link: Option<String>,
    pub tracker: Option<String>,
    pub tracker_message: Option<String>,
    #[serde(rename = "type")]
    pub type_: PutIOTransferType, // `type` is a reserved keyword in Rust
    pub up_speed: Option<i64>,
    pub uploaded: Option<i64>,
    pub userfile_exists: bool,
}

impl PutIOTransfer {
    pub fn is_downloadable(&self) -> bool {
        self.file_id.is_some()
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountInfoResponse {
    pub info: Info,
    pub status: String, // "OK"
}

#[derive(Debug, Deserialize)]
pub struct Info {
    pub account_active: bool,
    pub account_status: String, // e.g., "active"
    pub avatar_url: String,
    pub can_create_sub_account: bool,
    pub disk: Disk,
    pub family_owner: Option<String>,
    pub files_will_be_deleted_at: Option<String>, // ISO 8601 timestamp or null
    pub is_eligible_for_friend_invitation: bool,
    pub is_sub_account: bool,
    pub mail: String,
    pub monthly_bandwidth_usage: u64,
    pub password_last_changed_at: Option<String>, // ISO 8601 timestamp or null
    pub private_download_host_ip: Option<String>,
    pub settings: Settings,
    pub trash_size: u64,
    pub user_id: u32,
    pub username: String,
    pub warnings: Option<serde_json::Value>, // Assuming warnings is a JSON object
}

#[derive(Debug, Deserialize)]
pub struct Disk {
    pub avail: u64,
    pub size: u64,
    pub used: u64,
}

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub beta_user: bool,
    pub callback_url: Option<String>,
    pub dark_theme: bool,
    pub default_download_folder: u32,
    pub dont_autoselect_subtitles: bool,
    pub fluid_layout: bool,
    pub hide_subtitles: bool,
    pub history_enabled: bool,
    pub is_invisible: bool,
    pub locale: Option<String>,
    pub login_mails_enabled: bool,
    pub next_episode: bool,
    pub pushover_token: Option<String>,
    pub show_optimistic_usage: bool,
    pub sort_by: String, // e.g., "NAME_ASC"
    pub start_from: bool,
    pub subtitle_languages: Vec<String>, // Assuming it's an array of strings
    pub theater_mode: bool,
    pub theme: String, // e.g., "auto"
    pub transfer_sort_by: Option<String>,
    pub trash_enabled: bool,
    pub tunnel_route_name: String, // e.g., "cdn77"
    pub two_factor_enabled: bool,
    pub use_private_download_ip: bool,
    pub use_start_from: bool,
    pub video_player: Option<String>,
}

pub async fn account_info(api_token: &str) -> Result<AccountInfoResponse> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.put.io/v2/account/info")
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("Error getting put.io account info: {}", response.status());
    }

    Ok(response.json().await?)
}

#[derive(Debug, Deserialize)]
pub struct ListTransferResponse {
    pub transfers: Vec<PutIOTransfer>,
}

#[derive(Debug, Deserialize)]
pub struct GetTransferResponse {
    pub transfer: PutIOTransfer,
}

/// Returns the user's transfers.
pub async fn list_transfers(api_token: &str) -> Result<ListTransferResponse> {
    let client = reqwest::Client::new();
    let response = client
        .get("https://api.put.io/v2/transfers/list")
        .timeout(Duration::from_secs(10))
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("Error getting put.io transfers: {}", response.status());
    }

    Ok(response.json().await?)
}

pub async fn get_transfer(api_token: &str, transfer_id: u64) -> Result<GetTransferResponse> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("https://api.put.io/v2/transfers/{}", transfer_id))
        .timeout(Duration::from_secs(10))
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "Error getting put.io transfer id:{}: {}",
            transfer_id,
            response.status()
        );
    }

    Ok(response.json().await?)
}

pub async fn remove_transfer(api_token: &str, transfer_id: u64) -> Result<()> {
    let client = reqwest::Client::new();
    let form = multipart::Form::new().text("transfer_ids", transfer_id.to_string());
    let response = client
        .post("https://api.put.io/v2/transfers/remove")
        .timeout(Duration::from_secs(10))
        .multipart(form)
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "Error removing put.io transfer id:{}: {}",
            transfer_id,
            response.status()
        );
    }

    Ok(())
}

pub async fn delete_file(api_token: &str, file_id: u64) -> Result<()> {
    let client = reqwest::Client::new();
    let form = multipart::Form::new().text("file_ids", file_id.to_string());
    let response = client
        .post("https://api.put.io/v2/files/delete")
        .timeout(Duration::from_secs(10))
        .multipart(form)
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "Error removing put.io file/direcotry id:{}: {}",
            file_id,
            response.status()
        );
    }

    Ok(())
}

pub async fn add_transfer(api_token: &str, url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let form = multipart::Form::new().text("url", url.to_string());
    let response = client
        .post("https://api.put.io/v2/transfers/add")
        .timeout(Duration::from_secs(10))
        .multipart(form)
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("Error adding url: {} to put.io: {}", url, response.status());
    }

    Ok(())
}

pub async fn upload_file(api_token: &str, bytes: &[u8]) -> Result<()> {
    let client = reqwest::Client::new();
    let file_part = multipart::Part::bytes(bytes.to_owned()).file_name("foo.torrent");

    let form = reqwest::multipart::Form::new()
        .part("file", file_part)
        .text("filename", "foo.torrent");

    let response = client
        .post("https://upload.put.io/v2/files/upload")
        .timeout(Duration::from_secs(10))
        .header("authorization", format!("Bearer {}", api_token))
        .multipart(form)
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("Error uploading file to put.io: {}", response.status());
    }
    // Todo: error if invalid request
    Ok(())
}
#[derive(Debug, Serialize, Deserialize)]
pub struct UrlResponse {
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListFileResponse {
    pub files: Vec<FileResponse>,
    pub parent: FileResponse,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileResponse {
    pub content_type: String,
    pub id: u64,
    pub name: String,
    pub file_type: String,
}

pub async fn list_files(api_token: &str, file_id: u64) -> Result<ListFileResponse> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!(
            "https://api.put.io/v2/files/list?parent_id={}",
            file_id
        ))
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "Error listing put.io file/direcotry id:{}: {}",
            file_id,
            response.status()
        );
    }

    Ok(response.json().await?)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct URLResponse {
    pub url: String,
}

pub async fn url(api_token: &str, file_id: u64) -> Result<String> {
    let client = reqwest::Client::new();
    let response = client
        .get(format!("https://api.put.io/v2/files/{}/url", file_id))
        .header("authorization", format!("Bearer {}", api_token))
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "Error getting url for put.io file id:{}: {}",
            file_id,
            response.status()
        );
    }

    Ok(response.json::<URLResponse>().await?.url)
}

/// Returns a new OOB code.
pub async fn get_oob() -> Result<String> {
    let response = reqwest::get("https://api.put.io/v2/oauth2/oob/code?app_id=6487").await?;

    if !response.status().is_success() {
        bail!("Error getting put.io OOB: {}", response.status());
    }

    let j = response.json::<HashMap<String, String>>().await?;

    Ok(j.get("code").expect("fetching OOB code").to_string())
}

/// Returns new OAuth token if the OOB code is linked to the user's account.
pub async fn check_oob(oob_code: String) -> Result<String> {
    let response = reqwest::get(format!(
        "https://api.put.io/v2/oauth2/oob/code/{}",
        oob_code
    ))
    .await?;

    if !response.status().is_success() {
        bail!(
            "Error checking put.io OOB {}: {}",
            oob_code,
            response.status()
        );
    }
    let j = response.json::<HashMap<String, String>>().await?;

    Ok(j.get("oauth_token")
        .expect("deserializing OAuth token")
        .to_string())
}
