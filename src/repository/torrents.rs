//! In-memory storage of the torrents being seeded.

use std::sync::Arc;

use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use tokio::sync::Mutex;

use crate::torrent::Torrent;

/// Info hash of a torrent (SHA-1 of the bencoded `info` dictionary).
pub type InfoHash = [u8; 20];

/// Shared, lockable handle to a torrent stored in the repository.
pub type TorrentHandle = Arc<Mutex<Torrent>>;

/// Thread-safe torrent store keyed by info hash.
///
/// Every accessor returns owned values (cloned `Arc` handles) so callers never
/// hold a `DashMap` shard lock across an `.await` point.
#[derive(Debug, Default)]
pub struct TorrentRepository {
    torrents: DashMap<InfoHash, TorrentHandle>,
}

impl TorrentRepository {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored torrents.
    pub fn len(&self) -> usize {
        self.torrents.len()
    }

    /// Store the torrent unless one with the same info hash already exists.
    ///
    /// Returns the handle of the newly stored torrent, or `None` for a duplicate.
    /// The check and the insertion are atomic.
    pub fn insert_if_absent(&self, torrent: Torrent) -> Option<TorrentHandle> {
        match self.torrents.entry(torrent.info_hash) {
            Entry::Occupied(_) => None,
            Entry::Vacant(vacant) => {
                let handle = Arc::new(Mutex::new(torrent));
                vacant.insert(Arc::clone(&handle));
                Some(handle)
            }
        }
    }

    /// Remove a torrent and return its handle if it was present.
    pub fn remove(&self, info_hash: &InfoHash) -> Option<TorrentHandle> {
        self.torrents.remove(info_hash).map(|(_, handle)| handle)
    }

    /// Owned snapshot of all torrent handles.
    pub fn snapshot(&self) -> Vec<TorrentHandle> {
        self.torrents
            .iter()
            .map(|entry| Arc::clone(entry.value()))
            .collect()
    }

    /// Owned snapshot of all `(info hash, handle)` pairs.
    pub fn entries(&self) -> Vec<(InfoHash, TorrentHandle)> {
        self.torrents
            .iter()
            .map(|entry| (*entry.key(), Arc::clone(entry.value())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_torrent(hash_byte: u8, name: &str) -> Torrent {
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
            source_path: None,
        }
    }

    #[test]
    fn test_new_repository_is_empty() {
        let repo = TorrentRepository::new();
        assert_eq!(repo.len(), 0);
        assert!(repo.snapshot().is_empty());
        assert!(repo.entries().is_empty());
    }

    #[tokio::test]
    async fn test_insert_if_absent_returns_handle_to_stored_torrent() {
        let repo = TorrentRepository::new();
        let handle = repo
            .insert_if_absent(make_torrent(1, "first"))
            .expect("first insert must succeed");
        assert_eq!(repo.len(), 1);

        // The returned handle and the stored one point to the same torrent.
        handle.lock().await.uploaded = 42;
        let stored = &repo.snapshot()[0];
        assert!(Arc::ptr_eq(&handle, stored));
        assert_eq!(stored.lock().await.uploaded, 42);
    }

    #[tokio::test]
    async fn test_insert_if_absent_rejects_duplicate_hash() {
        let repo = TorrentRepository::new();
        assert!(repo.insert_if_absent(make_torrent(1, "first")).is_some());
        assert!(repo.insert_if_absent(make_torrent(1, "second")).is_none());
        assert_eq!(repo.len(), 1);
        // The original torrent is kept.
        assert_eq!(repo.snapshot()[0].lock().await.name, "first");
    }

    #[test]
    fn test_insert_distinct_hashes() {
        let repo = TorrentRepository::new();
        assert!(repo.insert_if_absent(make_torrent(1, "first")).is_some());
        assert!(repo.insert_if_absent(make_torrent(2, "second")).is_some());
        assert_eq!(repo.len(), 2);
        assert_eq!(repo.snapshot().len(), 2);
    }

    #[test]
    fn test_remove_existing_and_missing() {
        let repo = TorrentRepository::new();
        repo.insert_if_absent(make_torrent(1, "first"));

        assert!(repo.remove(&[1u8; 20]).is_some());
        assert_eq!(repo.len(), 0);
        assert!(repo.remove(&[1u8; 20]).is_none());
    }

    #[test]
    fn test_entries_pairs_hash_with_handle() {
        let repo = TorrentRepository::new();
        repo.insert_if_absent(make_torrent(7, "seven"));
        let entries = repo.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0, [7u8; 20]);
    }

    #[tokio::test]
    async fn test_snapshot_does_not_block_writes() {
        // Holding a torrent lock taken from a snapshot must not prevent the
        // repository itself from being modified.
        let repo = TorrentRepository::new();
        repo.insert_if_absent(make_torrent(1, "first"));
        let snapshot = repo.snapshot();
        let _guard = snapshot[0].lock().await;
        assert!(repo.insert_if_absent(make_torrent(2, "second")).is_some());
        assert!(repo.remove(&[1u8; 20]).is_some());
    }
}
