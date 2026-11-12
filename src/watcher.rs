//! File system adapter: turns changes in the torrent directory into calls to
//! [`TorrentService`].

use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{debug, error, info, warn};

use crate::context::AppContext;
use crate::service::torrents::{AddError, TorrentService, is_torrent_file};

/// Delay before reading a new file, so it has time to be fully written.
const SETTLE_DELAY: Duration = Duration::from_millis(500);

/// File system event that we care about
#[derive(Debug, Clone, PartialEq, Eq)]
enum FsEvent {
    Added(PathBuf),
    Removed(PathBuf),
}

/// Keep only creations and removals of torrent files.
fn to_fs_events(event: notify::Event) -> Vec<FsEvent> {
    let make: fn(PathBuf) -> FsEvent = match event.kind {
        EventKind::Create(_) => FsEvent::Added,
        EventKind::Remove(_) => FsEvent::Removed,
        _ => return Vec::new(),
    };
    event
        .paths
        .into_iter()
        .filter(|path| is_torrent_file(path))
        .map(make)
        .collect()
}

/// Apply one file system event to the torrent set.
async fn handle_event(ctx: &AppContext, event: FsEvent) {
    let service = TorrentService::new(ctx);
    match event {
        FsEvent::Added(path) => {
            info!("New torrent file detected: {}", path.display());
            tokio::time::sleep(SETTLE_DELAY).await;
            match service.add_file(path.clone()).await {
                Ok(_) => {}
                Err(e @ AddError::Invalid(_)) => {
                    error!("Cannot add torrent {}: {e}", path.display());
                }
                Err(e) => warn!("Skipping torrent {}: {e}", path.display()),
            }
        }
        FsEvent::Removed(path) => {
            info!("Torrent file removed: {}", path.display());
            if service.remove_file(&path).await.is_none() {
                warn!(
                    "Could not find torrent matching removed file: {}",
                    path.display()
                );
            }
        }
    }
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

    // Blocking thread: receive from the notify channel and forward to the async channel
    std::thread::spawn(move || {
        while let Ok(result) = sync_rx.recv() {
            match result {
                Ok(event) => {
                    debug!("File event: {:?}", event);
                    for fs_event in to_fs_events(event) {
                        if async_tx.send(fs_event).is_err() {
                            error!("Failed to send event to async channel");
                            return;
                        }
                    }
                }
                Err(e) => error!("Watch error: {e}"),
            }
        }
    });

    // Process events asynchronously
    while let Some(event) = async_rx.recv().await {
        handle_event(&ctx, event).await;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use notify::event::{AccessKind, CreateKind, ModifyKind, RemoveKind};

    use super::*;
    use crate::torrent::Torrent;

    fn event(kind: EventKind, paths: &[&str]) -> notify::Event {
        paths.iter().fold(notify::Event::new(kind), |acc, path| {
            acc.add_path(PathBuf::from(path))
        })
    }

    // --- to_fs_events ---

    #[test]
    fn test_to_fs_events_create_keeps_torrent_files_only() {
        let events = to_fs_events(event(
            EventKind::Create(CreateKind::File),
            &["/data/a.torrent", "/data/notes.txt", "/data/B.TORRENT"],
        ));
        assert_eq!(
            events,
            vec![
                FsEvent::Added(PathBuf::from("/data/a.torrent")),
                FsEvent::Added(PathBuf::from("/data/B.TORRENT")),
            ]
        );
    }

    #[test]
    fn test_to_fs_events_remove() {
        let events = to_fs_events(event(
            EventKind::Remove(RemoveKind::File),
            &["/data/a.torrent"],
        ));
        assert_eq!(
            events,
            vec![FsEvent::Removed(PathBuf::from("/data/a.torrent"))]
        );
    }

    #[test]
    fn test_to_fs_events_ignores_other_kinds() {
        for kind in [
            EventKind::Modify(ModifyKind::Any),
            EventKind::Access(AccessKind::Any),
            EventKind::Any,
        ] {
            assert!(to_fs_events(event(kind, &["/data/a.torrent"])).is_empty());
        }
    }

    // --- handle_event (only paths that never reach a tracker) ---

    #[tokio::test]
    async fn test_handle_event_added_invalid_file() {
        let ctx = AppContext::new(crate::config::Config::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.torrent");
        std::fs::write(&path, b"not bencode").unwrap();

        handle_event(&ctx, FsEvent::Added(path)).await;
        assert_eq!(ctx.torrents.len(), 0);
    }

    #[tokio::test]
    async fn test_handle_event_removed() {
        let ctx = AppContext::new(crate::config::Config::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ubuntu.torrent");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/ubuntu-20.04.4-desktop-amd64.iso.torrent"
            ),
            &path,
        )
        .unwrap();
        // No tracker URL → the STOPPED announce makes no network call.
        let mut torrent = Torrent::from_file(path.clone()).unwrap();
        torrent.urls.clear();
        ctx.torrents.insert_if_absent(torrent);

        // Unknown file: nothing removed.
        handle_event(&ctx, FsEvent::Removed(Path::new("/x/y.torrent").into())).await;
        assert_eq!(ctx.torrents.len(), 1);

        handle_event(&ctx, FsEvent::Removed(path)).await;
        assert_eq!(ctx.torrents.len(), 0);
    }
}
