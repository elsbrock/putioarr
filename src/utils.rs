use anyhow::Result;
use serde::Serialize;
use std::{fs, io::Write, path::Path, time::Duration};
use tinytemplate::TinyTemplate;
use tokio::time::sleep;

use crate::services;

static TEMPLATE: &str = r#"# Required. Username and password that sonarr/radarr use to connect to the proxy
username = "myusername"
password = "mypassword"

# Required. Directory where the proxy will download files to. This directory has to be readable by
# sonarr/radarr in order to import downloads
download_directory = "/path/to/downloads"

# Optional bind address, default "0.0.0.0"
bind_address = "0.0.0.0"

# Optional TCP port, default 9091
port = 9091

# Optional log level, default "info"
loglevel = "info"

# Optional UID, default 1000. Change the owner of the downloaded files to this UID. Requires root.
uid = 1000

# Optional polling interval in secs, default 10.
polling_interval = 10

# Optional skip directories when downloading, default ["sample", "extras"]
skip_directories = ["sample", "extras"]

# Optional number of orchestration workers, default 10. Unless there are many changes coming from
# put.io, you shouldn't have to touch this number. 10 is already overkill.
orchestration_workers = 10

# Optional number of download workers, default 4. This controls how many downloads we run in parallel.
download_workers = 4

[putio]
# Required. Putio API key. You can generate one using `putioarr get-token`
api_key =  "{putio_api_key}"
"#;

#[derive(Serialize)]
struct Context {
    putio_api_key: String,
}

pub async fn generate_config(config_path: &str) -> Result<()> {
    // ensure config_path directory exists, if not, create it
    if !Path::new(&config_path).exists() {
        fs::create_dir_all(config_path)?;
    }

    println!("Generating config {}", &config_path);
    let putio_api_key = get_token().await?;

    let mut tt = TinyTemplate::new();
    tt.add_template("config", TEMPLATE)?;

    let context = Context { putio_api_key };

    let rendered = tt.render("config", &context)?;

    if Path::new(&config_path).exists() {
        println!("Backing up config {}", &config_path);
        fs::rename(config_path, format!("{}.bak", &config_path))?;
    }
    println!("Writing {}", &config_path);
    let mut file = fs::File::create(config_path)?;
    file.write_all(rendered.as_bytes())?;
    file.flush()?;
    drop(file);

    Ok(())
}

pub async fn get_token() -> Result<String> {
    println!();
    // Create new OOB code and prompt user to link
    let oob_code = services::putio::get_oob().await.expect("fetching OOB code");
    println!(
        "Go to https://put.io/link and enter the code: {:#?}",
        oob_code
    );
    println!("Waiting for token...");

    // Every three seconds, check if the OOB code was linked to the user's account
    let three_seconds = Duration::from_secs(3);

    loop {
        sleep(three_seconds).await;

        let get_oauth_token_result = services::putio::check_oob(oob_code.clone()).await;

        match get_oauth_token_result {
            Ok(token) => {
                println!("Put.io API token: {token}");
                return Ok(token);
            }
            Err(_error) => {
                continue;
            }
        };
    }
}
