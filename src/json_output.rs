use std::{fs, path::Path};

use tracing::error;

use crate::{STARTED, TORRENTS, torrent::CleansedTorrent};

#[derive(Serialize, PartialEq, Debug)]
struct Output {
    pub started: chrono::DateTime<chrono::Utc>,
    pub torrents: Vec<CleansedTorrent>,
}

impl Default for Output {
    fn default() -> Self {
        Self {
            started: chrono::offset::Utc::now(),
            torrents: Vec::new(),
        }
    }
}

/// Check if the given output file is writable.
pub fn writable(path: &str) -> bool {
    if path.ends_with('/') {
        error!("OUTPUT is a path, not a file");
        return false;
    }
    let p = Path::new(path);
    let parent = p.parent().unwrap();
    if !parent.is_dir() {
        error!("Directory {:?} does not exist", parent.to_str());
        return false;
    }
    let md = fs::metadata(parent).unwrap();
    !md.permissions().readonly()
}

/// Write a session file with torrent and its stats
pub async fn write() {
    let config = crate::CONFIG.get().unwrap();
    if let Some(path) = config.output_stats.clone() {
        // fill data in struct
        let started = *STARTED.get().unwrap();
        let torrents = TORRENTS.read().expect("Cannot get torrent list");
        let mut data = Output {
            started,
            torrents: Vec::with_capacity(torrents.len()),
        };
        for m in torrents.iter() {
            data.torrents.push(m.lock().unwrap().clone());
        }
        // write content
        let content = serde_json::to_string(&data).unwrap();
        if let Err(e) = tokio::fs::write(path, content.as_bytes()).await {
            error!("Cannot write stat file: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{json_output::writable, torrent::CleansedTorrent};

    use super::Output;

    #[test]
    fn test_writable() {
        assert!(writable("/dev/null"));

        // case with non writable folder
        let unwritable = "/tmp/unwritable";
        let p = std::path::Path::new(unwritable);
        if !p.is_dir() {
            std::fs::create_dir(unwritable).unwrap();
        }
        let md = std::fs::metadata(unwritable).unwrap();
        let mut permissions = md.permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(unwritable, permissions).unwrap();
        assert!(!writable("/tmp/unwritable/ratioup.json"));

        // case when folder does not exists
        assert!(!writable("/aze/rty/uio/pqs/ratioup.json"));
    }

    #[test]
    fn test_serialized_output() {
        let now = chrono::offset::Utc::now();
        let mut data = Output {
            started: now,
            torrents: Vec::with_capacity(1),
        };
        // case 1: no torrent
        assert_eq!(
            serde_json::to_string(&data).unwrap(),
            format!(
                "{{\"started\":\"{}\",\"torrents\":[]}}",
                now.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
            )
        );

        // case 2: with one torrent
        data.torrents.push(CleansedTorrent {
            name: "Test".to_owned(),
            urls: vec!["https://localhost:7777/announce".to_string()],
            length: 123456,
            private: true,
            info_hash: "infohash".to_owned(),
            downloaded: 123456,
            uploaded: 654321,
            last_announce: std::time::Instant::now(),
            info_hash_urlencoded: "hash".to_owned(),
            seeders: 1,
            leechers: 2,
            next_upload_speed: 6789,
            next_download_speed: 0,
            interval: 1800,
            error_count: 0,
        });
        assert_eq!(
            serde_json::to_string(&data).unwrap(),
            format!(
                "{{\"started\":\"{}\",\"torrents\":[{{\"name\":\"Test\",\"urls\":[\"https://localhost:7777/announce\"],\"length\":123456,\"private\":true,\"info_hash\":\"infohash\",\"downloaded\":123456,\"uploaded\":654321,\"seeders\":1,\"leechers\":2,\"next_upload_speed\":6789,\"next_download_speed\":0}}]}}",
                now.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
            )
        );
    }
}
