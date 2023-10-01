use std::io::Write;

use actix_multipart::Multipart;
use actix_web::{
    delete, get,
    http::{header::ContentType, StatusCode},
    post, web, HttpResponse, Result,
};
use futures_util::TryStreamExt;
use log::info;
use serde_json::json;
use uuid::Uuid;

use crate::{ACTIVE, CONFIG, TORRENTS};

#[post("/add_torrents")]
async fn receive_files(mut payload: Multipart) -> Result<HttpResponse> {
    while let Some(mut field) = payload.try_next().await? {
        // iterate over multipart stream
        let content_disposition = field.content_disposition(); // A multipart/form-data stream has to contain `content_disposition`
        let filename = content_disposition
            .get_filename()
            .map_or_else(|| Uuid::new_v4().to_string(), sanitize_filename::sanitize);
        let config = CONFIG.get().expect("Cannot read configuration");
        let filepath = format!("{}/{}", config.torrent_dir, filename);
        let filepath2 = filepath.clone();
        let mut f = web::block(|| std::fs::File::create(filepath)).await??; // File::create is blocking operation, use threadpool
        while let Some(chunk) = field.try_next().await? {
            // Field in turn is stream of *Bytes* object
            // filesystem operations are blocking, we have to use threadpool
            f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
        }
        crate::add_torrent(filepath2);
    }
    let list = &*TORRENTS.read().expect("Cannot get torrent list");
    Ok(HttpResponse::build(StatusCode::OK)
        .content_type(ContentType::json())
        .body(format!("{{\"torrents\":{}}}", json!(list))))
}

/// Remove one or more torrents from a list of info hash.
#[delete("/delete_torrent")]
async fn delete_torrent(info: web::Json<Vec<String>>) -> Result<HttpResponse> {
    if !info.is_empty() {
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        let mut i = 0usize;
        while i < list.len() {
            for j in 0..info.len() {
                if list[i].info_hash == info[j] {
                    list.swap_remove(i);
                    continue;
                }
                i+=1;
            }
        }
    }
    Ok(HttpResponse::NoContent().finish())
}

/// Returns the configuration as a JSON string
#[get("/config")]
async fn get_config() -> Result<HttpResponse> {
    let c = CONFIG.get().expect("Cannot read configuration");
    Ok(HttpResponse::build(StatusCode::OK)
        .content_type(ContentType::json())
        .body(format!("{{\"config\":{}}}", json!(c))))
}

/// Returns the torrent list as a JSON string
#[get("/torrents")]
async fn get_torrents() -> Result<HttpResponse> {
    let list = &*TORRENTS.read().expect("Cannot get torrent list");
    Ok(HttpResponse::build(StatusCode::OK)
        .content_type(ContentType::json())
        .body(format!("{{\"torrents\":{}}}", json!(list))))
}

/// Stort or stop the seeding depending on the current state, you should stop the app instead
#[get("/toggle")]
async fn toggle_active() -> Result<HttpResponse> {
    let w = ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
    if !w {
        // resume seeding
        ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        info!("Seedding resumed");
        return Ok(HttpResponse::build(StatusCode::OK)
            .content_type(ContentType::json())
            .body("true"));
    } else {
        // stop seeding
        ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
        info!("Seedding stopped");
        return Ok(HttpResponse::build(StatusCode::OK)
            .content_type(ContentType::json())
            .body("false"));
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct CommandParams {
    command: String,
    infohash: String,
}
#[post("/command")]
async fn process_user_command(params: web::Form<CommandParams>) -> HttpResponse {
    info!("Processing user command: {}", params.command);
    if params.command.to_lowercase() == "remove" && !params.infohash.is_empty() {
        //enable disable torrent
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        for i in 0..list.len() {
            if list[i].info_hash == params.infohash {
                let r = std::fs::remove_file(&list[i].path);
                if r.is_ok() {
                    list.remove(i);
                    return HttpResponse::build(StatusCode::OK)
                        .content_type(ContentType::json())
                        .body(format!("{{\"removed\":\"{}\"}}", params.infohash));
                } else {
                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
                        .body("Cannot remove torrent file");
                }
            }
        }
    }
    HttpResponse::build(StatusCode::BAD_REQUEST).finish()
}
