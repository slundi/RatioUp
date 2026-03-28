use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::announcer::tracker::{Event, announce};
use crate::torrent::Torrent;
use crate::utils::format_bytes_u64;
use crate::{CLIENT, TORRENTS};

/// File system event that we care about
#[derive(Debug, Clone)]
enum FsEvent {
    Added(PathBuf),
    Removed(PathBuf),
}

/// Start watching the torrent directory for file changes.
pub async fn watch_directory(directory: PathBuf) {
    let (sync_tx, sync_rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let (async_tx, mut async_rx) = tokio::sync::mpsc::unbounded_channel::<FsEvent>();

    let mut watcher = match RecommendedWatcher::new(
        move |res| {
            if let Err(e) = sync_tx.send(res) {
                error!("Watcher channel send error: {e}");
            }
        },
        Config::default().with_poll_interval(Duration::from_secs(2)),
    ) {
        Ok(w) => w,
        Err(e) => {
            error!("Cannot create file watcher: {e}");
            return;
        }
    };

    if let Err(e) = watcher.watch(&directory, RecursiveMode::NonRecursive) {
        error!("Cannot watch directory {}: {e}", directory.display());
        return;
    }

    info!("Watching directory for changes: {}", directory.display());

    // Spawn blocking task to receive from sync channel and forward to async channel
    let async_tx_clone = async_tx.clone();
    std::thread::spawn(move || {
        loop {
            match sync_rx.recv() {
                Ok(Ok(event)) => {
                    debug!("File event: {:?}", event);
                    for path in event.paths {
                        // Only process .torrent files
                        if !path
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
                        {
                            continue;
                        }

                        let fs_event = match event.kind {
                            EventKind::Create(_) => Some(FsEvent::Added(path)),
                            EventKind::Remove(_) => Some(FsEvent::Removed(path)),
                            _ => None,
                        };

                        if let Some(e) = fs_event
                            && async_tx_clone.send(e).is_err()
                        {
                            error!("Failed to send event to async channel");
                            return;
                        }
                    }
                }
                Ok(Err(e)) => {
                    error!("Watch error: {e}");
                }
                Err(_) => {
                    // Channel closed
                    break;
                }
            }
        }
    });

    // Process events asynchronously
    while let Some(event) = async_rx.recv().await {
        match event {
            FsEvent::Added(path) => {
                // Small delay to ensure file is fully written
                info!("File added: {}", path.display());
                tokio::time::sleep(Duration::from_millis(500)).await;
                handle_file_added(path).await;
            }
            FsEvent::Removed(path) => {
                info!("File removed: {}", path.display());
                handle_file_removed(path).await;
            }
        }
    }
}

async fn handle_file_added(path: PathBuf) {
    info!("New torrent file detected: {}", path.display());

    // Parse the torrent file
    let torrent = match Torrent::from_file(path.clone()) {
        Ok(t) => t,
        Err(e) => {
            error!("Cannot parse torrent {}: {e}", path.display());
            return;
        }
    };

    // Check if torrent has URLs
    if torrent.urls.is_empty() {
        warn!(
            "Skipping torrent {} because there is no URL (DHT or not supported URLs)",
            path.display()
        );
        return;
    }

    // Check for duplicates
    {
        let list = TORRENTS.read().await;
        for m in list.iter() {
            let t = m.lock().await;
            if t.info_hash_urlencoded == torrent.info_hash_urlencoded {
                warn!("Torrent with same hash already exists: {}", torrent.name);
                return;
            }
        }
    }

    let name = torrent.name.clone();
    let info_hash = torrent.info_hash_urlencoded.clone();

    // Add to the list
    {
        let mut list = TORRENTS.write().await;
        list.push(Mutex::new(torrent));
    }

    // Announce with STARTED event
    if CLIENT.read().await.is_some() {
        let list = TORRENTS.read().await;
        // Find the torrent we just added (last one with matching hash)
        for m in list.iter().rev() {
            let mut t = m.lock().await;
            if t.info_hash_urlencoded == info_hash {
                announce(&mut t, Some(Event::Started)).await;
                info!(
                    "Added and announced torrent: {} (interval: {}s)",
                    name, t.interval
                );
                break;
            }
        }
    }
}

async fn handle_file_removed(path: PathBuf) {
    info!("Torrent file removed: {}", path.display());

    // Find and remove the torrent
    let mut removed_info: Option<(String, u64, u16, u16, u16)> = None;
    let mut removed_hash: Option<String> = None;

    {
        let list = TORRENTS.read().await;
        for m in list.iter() {
            let t = m.lock().await;
            // First try to match by source path (most reliable)
            let matches = if let Some(ref source) = t.source_path {
                source == &path
            } else {
                // Fallback: match by filename stem vs torrent name
                path.file_stem().is_some_and(|stem| {
                    let filename = stem.to_string_lossy();
                    t.name == filename.as_ref() || t.name.starts_with(filename.as_ref())
                })
            };

            if matches {
                removed_hash = Some(t.info_hash_urlencoded.clone());
                removed_info = Some((
                    t.name.clone(),
                    t.uploaded,
                    t.seeders,
                    t.leechers,
                    t.error_count,
                ));
                break;
            }
        }
    }

    if let Some(hash) = removed_hash {
        // Announce STOPPED before removing
        if CLIENT.read().await.is_some() {
            let list = TORRENTS.read().await;
            for m in list.iter() {
                let mut t = m.lock().await;
                if t.info_hash_urlencoded == hash {
                    announce(&mut t, Some(Event::Stopped)).await;
                    break;
                }
            }
        }

        // Remove from list
        {
            let mut list = TORRENTS.write().await;
            list.retain(|m| {
                // We need to check without async, so we use try_lock
                if let Ok(t) = m.try_lock() {
                    t.info_hash_urlencoded != hash
                } else {
                    true // Keep if we can't lock (shouldn't happen)
                }
            });
        }

        // Print stats
        if let Some((name, uploaded, seeders, leechers, errors)) = removed_info {
            info!(
                "Removed torrent \"{}\": uploaded={}, seeders={}, leechers={}, errors={}",
                name,
                format_bytes_u64(uploaded),
                seeders,
                leechers,
                errors
            );
        }
    } else {
        warn!(
            "Could not find torrent matching removed file: {}",
            path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn test_torrent_extension_check() {
        let torrent_path = Path::new("/tmp/test.torrent");
        let txt_path = Path::new("/tmp/test.txt");
        let no_ext = Path::new("/tmp/test");

        assert!(
            torrent_path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
        );
        assert!(
            !txt_path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
        );
        assert!(
            !no_ext
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
        );
    }
}
