use std::io::Write;

use actix_multipart::Multipart;
use actix_web::{
    get,
    http::{header::ContentType, StatusCode},
    post, web, HttpResponse, Result,
};
use futures_util::TryStreamExt;
use log::{info, warn};
use serde_json::json;
use uuid::Uuid;

use crate::{
    torrent::{self, CleansedTorrent},
    CLIENT, CONFIG, TORRENTS,
};

/// Get the torrent list because it is originally a list of mutexes
fn get_torrent_list() -> Vec<CleansedTorrent> {
    let list = &*TORRENTS.read().expect("Cannot get torrent list");
    let mut result: Vec<CleansedTorrent> = Vec::with_capacity(list.len());
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

/// Returns the torrent file list when applicable
#[get("/torrents/{info_hash}/files")]
async fn torrent_files(path: web::Path<String>) -> Result<HttpResponse> {
    let torrents = get_torrent_list();
    #[derive(Debug, Serialize)]
    struct File {
        path: String,
        size: i64,
    }
    for t in torrents {
        if t.info_hash == path.as_str() {
            match torrent::from_file(t.path) {
                Ok(torrent) => {
                    if let Some(files) = torrent.info.files {
                        let mut result: Vec<File> = Vec::with_capacity(files.len());
                        for f in files.iter() {
                            result.push(File {
                                path: f.get_path_with_separator(),
                                size: f.length,
                            });
                        }
                        return Ok(HttpResponse::build(StatusCode::OK)
                            .content_type(ContentType::json())
                            .json(json!(result)));
                    }
                    break; // returns the 404 Not found error
                }
                Err(err) => {
                    warn!("Torrent not found with info hash: {}\tError: {}", path, err);
                    return Ok(HttpResponse::build(StatusCode::NOT_FOUND).finish());
                }
            }
        }
    }
    Ok(HttpResponse::build(StatusCode::NOT_FOUND).finish())
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
        for it in list.iter().enumerate() {
            let t = it.1.lock().unwrap();
            if t.info_hash == params.infohash {
                let r = std::fs::remove_file(&t.path);
                if r.is_ok() {
                    item_to_remove = Some(it.0);
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

/// Check the health of the service
#[get("/health")]
async fn health_check() -> Result<HttpResponse> {
    let client = &*CLIENT.read().unwrap();
    if client.is_none() {
        return Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
            .content_type(ContentType::json())
            .body("{\"error\":\"CClient is undefined\"}"));
    }
    // Check that we can read the torrent list
    let list = TORRENTS.read();
    if let Err(_e) = list {
        return Ok(HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR)
            .content_type(ContentType::json())
            .body("{\"error\":\"Cannot get torrent list\"}"));
    }

    Ok(HttpResponse::build(StatusCode::OK).finish())
}
