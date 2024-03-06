use std::io::Write;

use actix_multipart::Multipart;
use actix_web::{
    get,
    http::{header::ContentType, StatusCode},
    post, web, HttpResponse, Result,
};
use futures_util::TryStreamExt;
use log::info;
use serde_json::json;
use uuid::Uuid;

use crate::{torrent::BasicTorrent, CONFIG, TORRENTS};

/// Get the torrent list because it is originally a list of mutexes
fn get_torrent_list() -> Vec<BasicTorrent> {
    let list = &*TORRENTS.read().expect("Cannot get torrent list");
    let mut result: Vec<BasicTorrent> = Vec::with_capacity(list.len());
    for mutex in list {
        let t = mutex.lock().unwrap();
        result.push(t.clone());
    }
    result
}

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
    Ok(HttpResponse::build(StatusCode::OK)
        .content_type(ContentType::json())
        .body(format!("{{\"torrents\":{}}}", json!(get_torrent_list()))))
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
    Ok(HttpResponse::build(StatusCode::OK)
        .content_type(ContentType::json())
        .body(format!("{{\"torrents\":{}}}", json!(get_torrent_list()))))
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
        let mut item_to_remove: Option<usize> = None;
        for i in 0..list.len() {
            let t = list[i].lock().unwrap();
            if t.info_hash == params.infohash {
                let r = std::fs::remove_file(&t.path);
                if r.is_ok() {
                    item_to_remove = Some(i);
                    break;
                } else {
                    return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
                        .body("Cannot remove torrent file");
                }
            }
        }
        if let Some(index) = item_to_remove {
            list.swap_remove(index);
            return HttpResponse::build(StatusCode::OK)
                .content_type(ContentType::json())
                .body(format!("{{\"removed\":\"{}\"}}", params.infohash));
        }
    }
    HttpResponse::build(StatusCode::BAD_REQUEST).finish()
}
