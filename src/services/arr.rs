use anyhow::{bail, Result};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ArrHistoryResponse {
    pub page: u32,
    pub page_size: u32,
    pub total_records: u32,
    pub records: Vec<ArrHistoryRecord>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ArrHistoryRecord {
    pub event_type: String,
    pub data: HashMap<String, Option<String>>,
}

/// Checks if a given API key is valid for a given target
/// # Returns
/// - Ok(()) if the API key is valid
/// - Err(anyhow::Error) if the API key is invalid
pub async fn verify_auth(target: &str, api_key: &str, base_url: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{base_url}/api");
    let response = client.get(url).header("X-Api-Key", api_key).send().await?;
    if response.status() == reqwest::StatusCode::OK {
        Ok(())
    } else {
        bail!("Invalid API key for {target}")
    }
}

/// Checks if a given target path has been imported by querying the Radarr/Sonarr history API
///
/// # Arguments
/// * `target` - The path to check for import status
/// * `api_key` - API key for authentication
/// * `base_url` - Base URL of the Radarr/Sonarr instance
///
/// # Returns
/// * `Result<bool>` - Ok(true) if target was found in import history, Ok(false) if not found
pub async fn check_imported(target: &str, api_key: &str, base_url: &str) -> Result<bool> {
    let client = reqwest::Client::new();
    let mut inspected = 0;
    let mut page = 0;
    loop {
        let url = format!(
            "{base_url}/api/v3/history?includeSeries=false&includeEpisode=false&page={page}&pageSize=1000");

        let response = client.get(&url).header("X-Api-Key", api_key).send().await?;

        if !response.status().is_success() {
            bail!("url: {}, status: {}", url, response.status());
        }

        let history_response: ArrHistoryResponse = response.json().await?;

        for record in history_response.records {
            if record.event_type == "downloadFolderImported"
                && record.data["droppedPath"].as_ref().unwrap() == target
            {
                return Ok(true);
            } else {
                inspected += 1;
                continue;
            }
        }

        if history_response.total_records < inspected {
            page += 1;
        } else {
            return Ok(false);
        }
    }
}
