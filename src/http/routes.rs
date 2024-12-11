use crate::{
    http::handlers::{
        handle_torrent_add, handle_torrent_get, handle_torrent_remove, handle_torrent_set,
    },
    services::{
        putio,
        transmission::{TransmissionConfig, TransmissionRequest, TransmissionResponse},
    },
    AppData,
};
use actix_web::{
    get,
    http::header::{ContentType, Header},
    post, web, HttpRequest, HttpResponse,
};
use actix_web_httpauth::headers::authorization::{Authorization, Basic};
use anyhow::{bail, Context, Result};
use log::{error, info};
use serde_json::json;

const SESSION_ID: &str = "useless-session-id";

#[post("/transmission/rpc")]
pub(crate) async fn rpc_post(
    payload: web::Json<TransmissionRequest>,
    req: HttpRequest,
    app_data: web::Data<AppData>,
) -> HttpResponse {
    let putio_api_token = &app_data.config.putio.api_key;
    let target_folder_id = {
        let folder_id = app_data.root_folder_id.read().unwrap();
        *folder_id
    };

    // Not sure if necessary since we might just look at the session id.
    if validate_user(req, &app_data).await.is_err() {
        return HttpResponse::Conflict()
            .content_type(ContentType::json())
            .insert_header(("X-Transmission-Session-Id", SESSION_ID))
            .body("");
    }

    info!("client rpc request for {}", payload.method);

    let arguments = match payload.method.as_str() {
        "session-get" => Some(json!(TransmissionConfig {
            download_dir: app_data.config.download_directory.clone(),
            ..Default::default()
        })),
        "torrent-set" => None, // Nothing to do here
        "torrent-get" => handle_torrent_get(putio_api_token, target_folder_id, &app_data).await,
        "queue-move-top" => None,
        "torrent-remove" => handle_torrent_remove(putio_api_token, &payload).await,
        "torrent-add" => {
            match handle_torrent_add(putio_api_token, target_folder_id, &payload).await {
                Ok(v) => v,
                Err(e) => {
                    error!("{}", e);
                    return HttpResponse::BadRequest().body(e.to_string());
                }
            }
        }
        _ => panic!("Unknwon method {}", payload.method),
    };

    let response = TransmissionResponse {
        result: String::from("success"),
        arguments,
    };

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .json(response)
}

/// Pretty much only used for authentication.
#[get("/transmission/rpc")]
async fn rpc_get(req: HttpRequest, app_data: web::Data<AppData>) -> HttpResponse {
    if validate_user(req, &app_data).await.is_err() {
        return HttpResponse::Forbidden().body("forbidden");
    }

    HttpResponse::Conflict()
        .content_type(ContentType::json())
        .insert_header(("X-Transmission-Session-Id", SESSION_ID))
        .body("")
    // HttpResponse::Ok().body("Hello world!")
}
async fn validate_user(req: HttpRequest, app_data: &web::Data<AppData>) -> Result<()> {
    let auth = Authorization::<Basic>::parse(&req)?;
    let user_username = auth.as_ref().user_id();
    let user_password = auth.as_ref().password().context("No password given")?;
    if user_username == app_data.config.username && user_password == app_data.config.password {
        Ok(())
    } else {
        bail!("Username or password mismatch")
    }
}
