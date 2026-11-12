//! Torrent lifecycle: loading torrent files, adding and removing them at runtime.
//!
//! Shared by the startup sequence (directory scan) and the file watcher, so both
//! apply the same validation rules.

use std::fmt;
use std::path::{Path, PathBuf};

use tracing::{error, info, warn};

use crate::announcer::tracker::{Event, announce};
use crate::context::AppContext;
use crate::repository::torrents::{InfoHash, TorrentHandle};
use crate::torrent::{Torrent, TorrentError};
use crate::utils::format_bytes_u64;

/// Why a torrent file was not added.
#[derive(Debug)]
pub enum AddError {
    /// The file cannot be read or is not a valid torrent.
    Invalid(TorrentError),
    /// The torrent has no supported tracker URL (DHT only or unsupported schemes).
    NoTrackerUrl,
    /// A torrent with the same info hash is already loaded.
    Duplicate { name: String },
}

impl fmt::Display for AddError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddError::Invalid(e) => write!(formatter, "invalid torrent: {e}"),
            AddError::NoTrackerUrl => {
                write!(formatter, "no tracker URL (DHT or not supported URLs)")
            }
            AddError::Duplicate { name } => {
                write!(
                    formatter,
                    "a torrent with the same hash is already added: {name}"
                )
            }
        }
    }
}

impl std::error::Error for AddError {}

/// Tell whether `path` looks like a torrent file (`.torrent` extension, any case).
pub fn is_torrent_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("torrent"))
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

/// Create the torrent directory when it does not exist yet.
pub async fn prepare_dir(directory: &Path) {
    if !directory.is_dir() {
        match tokio::fs::create_dir_all(directory).await {
            Ok(()) => info!("Torrent directory created: {}", directory.display()),
            Err(e) => error!(
                "Cannot create torrent directory {}: {e}",
                directory.display()
            ),
        }
    }
    info!("Will load torrents from: {}", directory.display());
}

/// Use cases on the set of seeded torrents.
pub struct TorrentService<'ctx> {
    ctx: &'ctx AppContext,
}

impl<'ctx> TorrentService<'ctx> {
    pub fn new(ctx: &'ctx AppContext) -> Self {
        Self { ctx }
    }

    /// Parse, validate and store a torrent file, without contacting any tracker.
    fn register(&self, path: PathBuf) -> Result<TorrentHandle, AddError> {
        let torrent = Torrent::from_file(path).map_err(AddError::Invalid)?;
        if torrent.urls.is_empty() {
            return Err(AddError::NoTrackerUrl);
        }
        let name = torrent.name.clone();
        self.ctx
            .torrents
            .insert_if_absent(torrent)
            .ok_or(AddError::Duplicate { name })
    }

    /// Load every torrent file of `directory` (non-recursive) without announcing them.
    ///
    /// Returns the number of torrents added. Invalid files are logged and skipped.
    pub fn load_dir(&self, directory: &Path) -> std::io::Result<usize> {
        let mut count = 0usize;
        for entry in std::fs::read_dir(directory)? {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(e) => {
                    error!("Cannot read entry in {}: {e}", directory.display());
                    continue;
                }
            };
            if !is_torrent_file(&path) {
                continue;
            }
            match self.register(path.clone()) {
                Ok(_) => {
                    info!("Found torrent {}", path.display());
                    count += 1;
                }
                Err(e @ AddError::Invalid(_)) => {
                    error!("Cannot add torrent {}: {e}", path.display());
                }
                Err(e) => warn!("Skipping torrent {}: {e}", path.display()),
            }
        }
        info!("{} torrent(s) loaded", count);
        Ok(count)
    }

    /// Add a torrent file at runtime and announce it with the STARTED event.
    pub async fn add_file(&self, path: PathBuf) -> Result<TorrentHandle, AddError> {
        let handle = self.register(path)?;
        {
            let mut torrent = handle.lock().await;
            announce(self.ctx, &mut torrent, Some(Event::Started)).await;
            info!(
                "Added and announced torrent: {} (interval: {}s)",
                torrent.name, torrent.interval
            );
        }
        Ok(handle)
    }

    /// Find the stored torrent that was loaded from `path`.
    async fn find_by_path(&self, path: &Path) -> Option<(InfoHash, TorrentHandle)> {
        for (info_hash, handle) in self.ctx.torrents.entries() {
            if matches_path(&*handle.lock().await, path) {
                return Some((info_hash, handle));
            }
        }
        None
    }

    /// Stop seeding the torrent loaded from `path`: announce STOPPED and remove it.
    ///
    /// Returns the removed torrent, or `None` when no torrent matches the path.
    pub async fn remove_file(&self, path: &Path) -> Option<TorrentHandle> {
        let (info_hash, handle) = self.find_by_path(path).await?;
        {
            let mut torrent = handle.lock().await;
            announce(self.ctx, &mut torrent, Some(Event::Stopped)).await;
            self.ctx.torrents.remove(&info_hash);
            info!(
                "Removed torrent \"{}\": uploaded={}, seeders={}, leechers={}, errors={}",
                torrent.name,
                format_bytes_u64(torrent.uploaded),
                torrent.seeders,
                torrent.leechers,
                torrent.error_count
            );
        }
        Some(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn ubuntu_fixture() -> &'static str {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/ubuntu-20.04.4-desktop-amd64.iso.torrent"
        )
    }

    fn audio_fixture() -> &'static str {
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/Audio_20160422_archive.torrent"
        )
    }

    fn make_torrent(hash_byte: u8, name: &str, source_path: Option<&str>) -> Torrent {
        Torrent {
            name: String::from(name),
            urls: vec![String::from("http://tracker.example.com/announce")],
            length: 1024,
            private: false,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: [hash_byte; 20],
            info_hash_urlencoded: String::new(),
            seeders: 0,
            leechers: 0,
            next_upload_speed: 0,
            interval: 1800,
            error_count: 0,
            encoding: None,
            min_interval: None,
            tracker_id: None,
            source_path: source_path.map(PathBuf::from),
        }
    }

    // --- is_torrent_file ---

    #[test]
    fn test_is_torrent_file() {
        assert!(is_torrent_file(Path::new("/tmp/test.torrent")));
        assert!(is_torrent_file(Path::new("/tmp/TEST.TORRENT")));
        assert!(!is_torrent_file(Path::new("/tmp/test.txt")));
        assert!(!is_torrent_file(Path::new("/tmp/test")));
        assert!(!is_torrent_file(Path::new("/tmp/torrent")));
    }

    // --- matches_path ---

    #[test]
    fn test_matches_path_by_source_path() {
        let torrent = make_torrent(0, "Some name", Some("/data/a.torrent"));
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
        let torrent = make_torrent(0, "ubuntu-20.04.4-desktop-amd64.iso", None);
        assert!(matches_path(
            &torrent,
            Path::new("/data/ubuntu-20.04.4-desktop-amd64.iso.torrent")
        ));
        assert!(matches_path(&torrent, Path::new("/data/ubuntu.torrent")));
        assert!(!matches_path(&torrent, Path::new("/data/debian.torrent")));
    }

    // --- AddError ---

    #[test]
    fn test_add_error_display() {
        assert!(
            AddError::NoTrackerUrl
                .to_string()
                .contains("no tracker URL")
        );
        let duplicate = AddError::Duplicate {
            name: String::from("ubuntu"),
        };
        assert!(duplicate.to_string().ends_with(": ubuntu"));
        let invalid = AddError::Invalid(TorrentError::IoError(std::io::Error::other("boom")));
        assert!(invalid.to_string().starts_with("invalid torrent:"));
    }

    // --- prepare_dir ---

    #[tokio::test]
    async fn test_prepare_dir_creates_missing_directory() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("nested").join("torrents");
        prepare_dir(&dir).await;
        assert!(dir.is_dir());
        // Calling it again on an existing directory is a no-op.
        prepare_dir(&dir).await;
        assert!(dir.is_dir());
    }

    // --- load_dir ---

    #[test]
    fn test_load_dir_counts_valid_files() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu.torrent")).unwrap();

        let count = TorrentService::new(&ctx).load_dir(dir.path()).unwrap();
        assert_eq!(count, 1);
        assert_eq!(ctx.torrents.len(), 1);
    }

    #[tokio::test]
    async fn test_load_dir_sets_source_path() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ubuntu.torrent");
        std::fs::copy(ubuntu_fixture(), &path).unwrap();

        TorrentService::new(&ctx).load_dir(dir.path()).unwrap();
        let handle = &ctx.torrents.snapshot()[0];
        assert_eq!(handle.lock().await.source_path.as_ref(), Some(&path));
    }

    #[test]
    fn test_load_dir_multiple_distinct_files() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu.torrent")).unwrap();
        std::fs::copy(audio_fixture(), dir.path().join("audio.torrent")).unwrap();

        let count = TorrentService::new(&ctx).load_dir(dir.path()).unwrap();
        assert_eq!(count, ctx.torrents.len());
        assert!(count >= 1);
    }

    #[test]
    fn test_load_dir_skips_non_torrent_and_invalid_files() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README.txt"), b"not a torrent").unwrap();
        std::fs::write(dir.path().join("broken.torrent"), b"not bencode").unwrap();

        let count = TorrentService::new(&ctx).load_dir(dir.path()).unwrap();
        assert_eq!(count, 0);
        assert_eq!(ctx.torrents.len(), 0);
    }

    #[test]
    fn test_load_dir_skips_duplicates() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        // Two copies of the same torrent → same info hash → only one inserted
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu1.torrent")).unwrap();
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu2.torrent")).unwrap();

        let count = TorrentService::new(&ctx).load_dir(dir.path()).unwrap();
        assert_eq!(count, 1);
        assert_eq!(ctx.torrents.len(), 1);
    }

    #[test]
    fn test_load_dir_empty_directory() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(TorrentService::new(&ctx).load_dir(dir.path()).unwrap(), 0);
    }

    #[test]
    fn test_load_dir_missing_directory_is_an_error() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        assert!(TorrentService::new(&ctx).load_dir(&missing).is_err());
    }

    // --- add_file (only paths that never reach a tracker) ---

    #[tokio::test]
    async fn test_add_file_invalid_torrent() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.torrent");
        std::fs::write(&path, b"not bencode").unwrap();

        let result = TorrentService::new(&ctx).add_file(path).await;
        assert!(matches!(result, Err(AddError::Invalid(_))));
        assert_eq!(ctx.torrents.len(), 0);
    }

    #[tokio::test]
    async fn test_add_file_missing_file() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();

        let result = TorrentService::new(&ctx)
            .add_file(dir.path().join("gone.torrent"))
            .await;
        assert!(matches!(result, Err(AddError::Invalid(_))));
    }

    #[tokio::test]
    async fn test_add_file_duplicate_keeps_existing() {
        let ctx = AppContext::new(Config::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ubuntu.torrent");
        std::fs::copy(ubuntu_fixture(), &path).unwrap();
        let mut existing = Torrent::from_file(path.clone()).unwrap();
        existing.name = String::from("already there");
        ctx.torrents.insert_if_absent(existing);

        let result = TorrentService::new(&ctx).add_file(path).await;
        assert!(
            matches!(&result, Err(AddError::Duplicate { name }) if name.starts_with("ubuntu")),
            "unexpected result: {result:?}"
        );
        assert_eq!(ctx.torrents.len(), 1);
        assert_eq!(
            ctx.torrents.snapshot()[0].lock().await.name,
            "already there"
        );
    }

    // --- remove_file ---

    #[tokio::test]
    async fn test_remove_file_unknown_path_keeps_torrents() {
        let ctx = AppContext::new(Config::default());
        ctx.torrents
            .insert_if_absent(make_torrent(1, "kept", Some("/data/kept.torrent")));

        let removed = TorrentService::new(&ctx)
            .remove_file(Path::new("/data/other.torrent"))
            .await;
        assert!(removed.is_none());
        assert_eq!(ctx.torrents.len(), 1);
    }

    #[tokio::test]
    async fn test_remove_file_matching_path_removes_only_that_torrent() {
        let ctx = AppContext::new(Config::default());
        // No tracker URL → the STOPPED announce makes no network call.
        let mut gone = make_torrent(1, "gone", Some("/data/gone.torrent"));
        gone.urls.clear();
        ctx.torrents.insert_if_absent(gone);
        ctx.torrents
            .insert_if_absent(make_torrent(2, "kept", Some("/data/kept.torrent")));

        let removed = TorrentService::new(&ctx)
            .remove_file(Path::new("/data/gone.torrent"))
            .await
            .expect("torrent must be found");
        assert_eq!(removed.lock().await.name, "gone");
        assert_eq!(ctx.torrents.len(), 1);
        assert_eq!(ctx.torrents.snapshot()[0].lock().await.name, "kept");
    }
}
