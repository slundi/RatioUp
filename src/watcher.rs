use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{debug, error, info, warn};

use crate::announcer::tracker::{Event, announce};
use crate::context::AppContext;
use crate::torrent::Torrent;
use crate::utils::format_bytes_u64;

/// File system event that we care about
#[derive(Debug, Clone)]
enum FsEvent {
    Added(PathBuf),
    Removed(PathBuf),
}

/// Start watching the torrent directory for file changes.
pub async fn watch_directory(ctx: Arc<AppContext>, directory: PathBuf) {
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
                handle_file_added(&ctx, path).await;
            }
            FsEvent::Removed(path) => {
                info!("File removed: {}", path.display());
                handle_file_removed(&ctx, path).await;
            }
        }
    }
}

async fn handle_file_added(ctx: &AppContext, path: PathBuf) {
    info!("New torrent file detected: {}", path.display());

    let torrent = match Torrent::from_file(path.clone()) {
        Ok(t) => t,
        Err(e) => {
            error!("Cannot parse torrent {}: {e}", path.display());
            return;
        }
    };

    if torrent.urls.is_empty() {
        warn!(
            "Skipping torrent {} because there is no URL (DHT or not supported URLs)",
            path.display()
        );
        return;
    }

    let name = torrent.name.clone();
    let Some(handle) = ctx.torrents.insert_if_absent(torrent) else {
        warn!("Torrent with same hash already exists: {name}");
        return;
    };

    let mut torrent = handle.lock().await;
    announce(ctx, &mut torrent, Some(Event::Started)).await;
    info!(
        "Added and announced torrent: {} (interval: {}s)",
        name, torrent.interval
    );
}

/// Tell whether a torrent was loaded from `path`.
///
/// Torrents without a known source file are matched on the file stem.
fn matches_path(torrent: &Torrent, path: &Path) -> bool {
    if let Some(source) = &torrent.source_path {
        source == path
    } else {
        path.file_stem().is_some_and(|stem| {
            let filename = stem.to_string_lossy();
            torrent.name.starts_with(filename.as_ref())
        })
    }
}

async fn handle_file_removed(ctx: &AppContext, path: PathBuf) {
    info!("Torrent file removed: {}", path.display());

    let mut found = None;
    for (info_hash, handle) in ctx.torrents.entries() {
        if matches_path(&*handle.lock().await, &path) {
            found = Some((info_hash, handle));
            break;
        }
    }

    let Some((info_hash, handle)) = found else {
        warn!(
            "Could not find torrent matching removed file: {}",
            path.display()
        );
        return;
    };

    let mut torrent = handle.lock().await;
    announce(ctx, &mut torrent, Some(Event::Stopped)).await;
    ctx.torrents.remove(&info_hash);
    info!(
        "Removed torrent \"{}\": uploaded={}, seeders={}, leechers={}, errors={}",
        torrent.name,
        format_bytes_u64(torrent.uploaded),
        torrent.seeders,
        torrent.leechers,
        torrent.error_count
    );
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{handle_file_added, handle_file_removed, matches_path};
    use crate::config::Config;
    use crate::context::AppContext;
    use crate::torrent::Torrent;

    fn ubuntu_fixture() -> &'static str {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/ubuntu-20.04.4-desktop-amd64.iso.torrent"
        )
    }

    // --- handlers (only paths that never reach a tracker) ---

    #[tokio::test]
    async fn test_handle_file_added_invalid_torrent_is_ignored() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.torrent");
        std::fs::write(&path, b"not bencode").unwrap();

        handle_file_added(&ctx, path).await;
        assert_eq!(ctx.torrents.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_file_added_duplicate_is_ignored() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ubuntu.torrent");
        std::fs::copy(ubuntu_fixture(), &path).unwrap();
        let mut existing = Torrent::from_file(path.clone()).unwrap();
        existing.name = String::from("already there");
        ctx.torrents.insert_if_absent(existing);

        handle_file_added(&ctx, path).await;
        assert_eq!(ctx.torrents.len(), 1);
        assert_eq!(
            ctx.torrents.snapshot()[0].lock().await.name,
            "already there"
        );
    }

    #[tokio::test]
    async fn test_handle_file_removed_unknown_file_keeps_torrents() {
        let ctx = AppContext::new(Config::default());
        ctx.torrents
            .insert_if_absent(make_torrent("kept", Some("/data/kept.torrent")));

        handle_file_removed(&ctx, Path::new("/data/other.torrent").to_path_buf()).await;
        assert_eq!(ctx.torrents.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_file_removed_matching_file_removes_torrent() {
        let ctx = AppContext::new(Config::default());
        // No tracker URL → the STOPPED announce makes no network call.
        let mut torrent = make_torrent("gone", Some("/data/gone.torrent"));
        torrent.urls.clear();
        ctx.torrents.insert_if_absent(torrent);

        handle_file_removed(&ctx, Path::new("/data/gone.torrent").to_path_buf()).await;
        assert_eq!(ctx.torrents.len(), 0);
    }

    fn make_torrent(name: &str, source_path: Option<&str>) -> Torrent {
        Torrent {
            name: String::from(name),
            urls: vec![String::from("http://tracker.example.com/announce")],
            length: 1024,
            private: false,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: [0u8; 20],
            info_hash_urlencoded: String::new(),
            seeders: 0,
            leechers: 0,
            next_upload_speed: 0,
            interval: 1800,
            error_count: 0,
            encoding: None,
            min_interval: None,
            tracker_id: None,
            source_path: source_path.map(std::path::PathBuf::from),
        }
    }

    #[test]
    fn test_matches_path_by_source_path() {
        let torrent = make_torrent("Some name", Some("/data/a.torrent"));
        assert!(matches_path(&torrent, Path::new("/data/a.torrent")));
        assert!(!matches_path(&torrent, Path::new("/data/b.torrent")));
        // With a known source, the name is ignored.
        assert!(!matches_path(
            &torrent,
            Path::new("/data/Some name.torrent")
        ));
    }

    #[test]
    fn test_matches_path_by_file_stem() {
        let torrent = make_torrent("ubuntu-20.04.4-desktop-amd64.iso", None);
        assert!(matches_path(
            &torrent,
            Path::new("/data/ubuntu-20.04.4-desktop-amd64.iso.torrent")
        ));
        assert!(matches_path(&torrent, Path::new("/data/ubuntu.torrent")));
        assert!(!matches_path(&torrent, Path::new("/data/debian.torrent")));
    }

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
