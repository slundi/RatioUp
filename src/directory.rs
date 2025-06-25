use tracing::{error, info};
use crate::{TORRENTS, torrent::{from_file}};
use std::path::PathBuf;
use std::sync::Mutex;

pub async fn prepare_torrent_folder(directory: PathBuf) {
    if !std::path::Path::new(&directory).is_dir() {
        tokio::fs::create_dir_all(directory.clone()).await.unwrap_or_else(|_e| {
            error!("Cannot create torrent folder directory(ies)");
        });
        info!("Torrent directory created: {}", directory.display());
    }
    info!("Will load torrents from: {}", directory.display());
}

/// Load torrents from the provided directory.
pub async fn load_torrents(directory: PathBuf) -> u16 {
    let paths = std::fs::read_dir(&directory).expect("Cannot read torrent directory");
    let mut count = 0u16;
    let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
    for p in paths {
        let f = p
            .expect("Cannot get torrent path")
            .path()
            .into_os_string()
            .into_string()
            .expect("Cannot get file name");
        if f.to_lowercase().ends_with(".torrent") {
            match from_file(f.as_str().into()) {
                Ok(torrent) => {
                    list.push(Mutex::new(torrent));
                    info!("Adding torrent {f}");
                    count += 1;
                }
                Err(e) => error!("Cannot add torrent {f}: {e}")
            }
        }
    }
    info!("{} torrent(s) loaded", count);
    count
}
