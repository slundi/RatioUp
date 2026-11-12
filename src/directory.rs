use crate::repository::torrents::TorrentRepository;
use crate::torrent::Torrent;
use std::path::PathBuf;
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

/// Load torrents from the provided directory into the repository.
///
/// Add a torrent to the list. If the filename does not end with .torrent, the file is not processed.
pub async fn load_torrents(torrents: &TorrentRepository, directory: PathBuf) -> usize {
    let paths = std::fs::read_dir(&directory).expect("Cannot read torrent directory");
    let mut count = 0usize;
    for entry in paths {
        let path = entry.expect("Cannot get torrent path").path();
        if let Some(extension) = path.extension()
            && extension.eq_ignore_ascii_case("torrent")
        {
            match Torrent::from_file(path.clone()) {
                Ok(torrent) => {
                    info!("Found torrent {}", path.display());
                    if torrent.urls.is_empty() {
                        warn!(
                            "Skipping torrent because there is no URL (DHT or not supported URLs)"
                        );
                        continue;
                    }
                    if torrents.insert_if_absent(torrent).is_some() {
                        count += 1;
                    } else {
                        warn!("A torrent with the same hash is already added");
                    }
                }
                Err(e) => error!("Cannot add torrent {}: {e}", path.display()),
            }
        }
    }
    info!("{} torrent(s) loaded", count);
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Return the path to a real .torrent fixture bundled with the test suite.
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

    #[tokio::test]
    async fn test_load_torrents_counts_valid_files() {
        let repo = TorrentRepository::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu.torrent")).unwrap();

        let count = load_torrents(&repo, dir.path().to_path_buf()).await;
        assert_eq!(count, 1);
        assert_eq!(repo.len(), 1);
    }

    #[tokio::test]
    async fn test_load_torrents_sets_source_path() {
        let repo = TorrentRepository::new();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ubuntu.torrent");
        std::fs::copy(ubuntu_fixture(), &path).unwrap();

        load_torrents(&repo, dir.path().to_path_buf()).await;
        let handle = &repo.snapshot()[0];
        assert_eq!(handle.lock().await.source_path.as_ref(), Some(&path));
    }

    #[tokio::test]
    async fn test_load_torrents_multiple_distinct_files() {
        let repo = TorrentRepository::new();
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu.torrent")).unwrap();
        std::fs::copy(audio_fixture(), dir.path().join("audio.torrent")).unwrap();

        let count = load_torrents(&repo, dir.path().to_path_buf()).await;
        assert_eq!(count, repo.len());
        assert!(count >= 1);
    }

    #[tokio::test]
    async fn test_load_torrents_skips_non_torrent_files() {
        let repo = TorrentRepository::new();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("README.txt"), b"not a torrent")
            .await
            .unwrap();

        let count = load_torrents(&repo, dir.path().to_path_buf()).await;
        assert_eq!(count, 0);
        assert_eq!(repo.len(), 0);
    }

    #[tokio::test]
    async fn test_load_torrents_skips_invalid_torrent_file() {
        let repo = TorrentRepository::new();
        let dir = tempfile::tempdir().unwrap();
        tokio::fs::write(dir.path().join("broken.torrent"), b"not bencode")
            .await
            .unwrap();

        let count = load_torrents(&repo, dir.path().to_path_buf()).await;
        assert_eq!(count, 0);
        assert_eq!(repo.len(), 0);
    }

    #[tokio::test]
    async fn test_load_torrents_skips_duplicates() {
        let repo = TorrentRepository::new();
        let dir = tempfile::tempdir().unwrap();
        // Two copies of the same torrent → same info hash → only one inserted
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu1.torrent")).unwrap();
        std::fs::copy(ubuntu_fixture(), dir.path().join("ubuntu2.torrent")).unwrap();

        let count = load_torrents(&repo, dir.path().to_path_buf()).await;
        assert_eq!(count, 1);
        assert_eq!(repo.len(), 1);
    }

    #[tokio::test]
    async fn test_load_torrents_empty_directory() {
        let repo = TorrentRepository::new();
        let dir = tempfile::tempdir().unwrap();
        let count = load_torrents(&repo, dir.path().to_path_buf()).await;
        assert_eq!(count, 0);
    }
}
