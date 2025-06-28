use crate::{TORRENTS, torrent::Torrent};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{error, info, warn};

pub async fn prepare_torrent_folder(directory: PathBuf) {
    if !std::path::Path::new(&directory).is_dir() {
        tokio::fs::create_dir_all(directory.clone())
            .await
            .unwrap_or_else(|_e| {
                error!("Cannot create torrent folder directory(ies)");
            });
        info!("Torrent directory created: {}", directory.display());
    }
    info!("Will load torrents from: {}", directory.display());
}

/// Load torrents from the provided directory.
///
/// Add a torrent to the list. If the filename does not end with .torrent, the file is not processed.
pub async fn load_torrents(directory: PathBuf) -> u16 {
    let paths = std::fs::read_dir(&directory).expect("Cannot read torrent directory");
    let mut count = 0u16;
    let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
    let mut added_hashes: Vec<String> = Vec::new();
    for p in paths {
        let path = p.expect("Cannot get torrent path").path();
        if let Some(extension) = path.clone().extension() {
            if extension.eq_ignore_ascii_case("torrent") {
                match Torrent::from_file(path.clone()) {
                    Ok(torrent) => {
                        info!("Found torrent {}", path.display());
                        // info!("Found torrent {} {:?}", path.display(), torrent);
                        // TODO: dedup, ignore UDP
                        if torrent.urls.is_empty() {
                            warn!(
                                "Skipping torrent because there is no URL (DHT or not supported URLs)"
                            );
                            continue;
                        }
                        if added_hashes.contains(&torrent.info_hash_urlencoded) {
                            warn!("A torrent with the same hash is already added");
                        } else {
                            added_hashes.push(torrent.info_hash_urlencoded.clone());
                            list.push(Mutex::new(torrent));
                            count += 1;
                        }
                    }
                    Err(e) => error!("Cannot add torrent {}: {e}", path.display()),
                }
            }
        }
    }
    info!("{} torrent(s) loaded", count);
    count
}
