use std::sync::{Mutex, RwLock, RwLockWriteGuard};

use crate::{http::routes, services::putio};
use actix_web::{middleware::Logger, web, App, HttpServer};
use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use directories::ProjectDirs;
use env_logger::TimestampPrecision;
use figment::{
    providers::{Format, Serialized, Toml},
    Figment,
};
use log::{error, info};
use serde::{Deserialize, Serialize};
use utils::{generate_config, get_token};

mod download_system;
mod http;
mod services;
mod utils;

/// put.io to sonarr/radarr proxy
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the proxy
    Run(RunArgs),
    /// Generate a put.io API token
    GetToken,
    /// Generate config
    GenerateConfig(RunArgs),
}

#[derive(Parser)]
struct RunArgs {
    #[arg(short, long = "config", default_value_t = ProjectDirs::from("nl", "evenflow", "putioarr").unwrap().config_dir().join("config.toml").into_os_string().into_string().unwrap(), env("APP_CONFIG_PATH"))]
    pub config_path: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    bind_address: String,
    download_directory: String,
    download_workers: usize,
    loglevel: String,
    orchestration_workers: usize,
    password: String,
    polling_interval: u64,
    port: u16,
    skip_directories: Vec<String>,
    uid: u32,
    username: String,
    putio: PutioConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PutioConfig {
    api_key: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ArrConfig {
    url: String,
    api_key: String,
}

pub struct AppData {
    pub config: Config,
    root_folder_id: RwLock<u64>,
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[actix_web::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Commands::Run(args) => {
            let config: Config = Figment::new()
                .join(Serialized::default("bind_address", "0.0.0.0"))
                .join(Serialized::default("download_workers", 4))
                .join(Serialized::default("orchestration_workers", 10))
                .join(Serialized::default("loglevel", "info"))
                .join(Serialized::default("polling_interval", 10))
                .join(Serialized::default("port", 9091))
                .join(Serialized::default("uid", 1000))
                .join(Serialized::default(
                    "skip_directories",
                    vec!["sample", "extras"],
                ))
                .merge(Toml::file(&args.config_path))
                .extract()?;

            let log_timestamp = if in_container::in_container() {
                Some(TimestampPrecision::Seconds)
            } else if let Ok(istty) = nix::unistd::isatty(0) {
                if istty {
                    Some(TimestampPrecision::Seconds)
                } else {
                    None
                }
            } else {
                None
            };

            env_logger::Builder::new()
                .default_format()
                .format_module_path(false)
                .format_target(false)
                .format_timestamp(log_timestamp)
                .parse_filters(config.loglevel.as_str())
                .init();

            info!("Starting putioarr, version {}", VERSION);

            let app_data = web::Data::new(AppData {
                config: config.clone(),
                root_folder_id: RwLock::new(0),
            });

            match putio::account_info(&app_data.config.putio.api_key).await {
                Ok(account_info) => {
                    info!(
                        "Logged in as user: {} (ID: {}) with email: {}",
                        account_info.info.username,
                        account_info.info.user_id,
                        account_info.info.mail
                    );
                    info!(
                        "Available space: {:.2} GB out of {:.2} GB ({:.2}%)",
                        account_info.info.disk.avail as f64 / 1_073_741_824.0,
                        account_info.info.disk.size as f64 / 1_073_741_824.0,
                        account_info.info.disk.avail as f64 / account_info.info.disk.size as f64
                            * 100.0
                    );
                }
                Err(e) => {
                    error!("{}", e);
                    bail!(e)
                }
            }

            // create putioarr folder on put.io if it doesn't exist
            match putio::create_folder(&app_data.config.putio.api_key, "putioarr", 0).await {
                Ok(_) => info!("Created putioarr folder on put.io"),
                Err(e) => {
                    if e.to_string().contains("400 Bad Request") {
                        info!("putioarr folder already exists on put.io");
                    } else {
                        error!("Failed to create putioarr folder: {}", e);
                        bail!(e);
                    }
                    // get folder ID of putioarr folder and store it in config
                    match putio::list_files(&app_data.config.putio.api_key, 0).await {
                        Ok(file_list) => {
                            // find folder with name "putioarr"
                            let folder_id = file_list
                                .files
                                .iter()
                                .find(|f| f.name == "putioarr")
                                .unwrap()
                                .id;
                            info!("putioarr folder ID: {}", folder_id);
                            let mut config_folder_id: RwLockWriteGuard<u64> =
                                app_data.root_folder_id.write().unwrap();
                            *config_folder_id = folder_id;
                        }
                        Err(e) => {
                            error!("Failed to get folder ID: {}", e);
                            bail!(e);
                        }
                    }
                }
            };

            let data_for_download_system = app_data.clone();
            download_system::start(data_for_download_system)
                .await
                .unwrap();

            info!(
                "Starting web server at http://{}:{}",
                config.bind_address, config.port
            );
            HttpServer::new(move || {
                App::new()
                    .wrap(Logger::new(
                        "%a \"%r\" %s %b \"%{Referer}i\" \"%{User-Agent}i\" %T",
                    ))
                    .app_data(app_data.clone())
                    .service(routes::rpc_post)
                    .service(routes::rpc_get)
            })
            .bind((config.bind_address, config.port))?
            .run()
            .await
            .context("Unable to start http server")
        }
        Commands::GetToken => {
            get_token().await?;
            Ok(())
        }
        Commands::GenerateConfig(args) => {
            generate_config(&args.config_path).await?;
            Ok(())
        }
    }
}
