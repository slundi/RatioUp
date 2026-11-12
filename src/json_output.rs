use std::{fs, path::Path};

use crate::context::AppContext;
use tracing::error;

/// Check if the given output file is writable.
pub fn writable(path: &str) -> bool {
    if path.ends_with('/') {
        error!("OUTPUT is a path, not a file");
        return false;
    }
    let p = Path::new(path);
    let Some(parent) = p.parent() else {
        error!("Output path has no parent directory");
        return false;
    };
    if !parent.is_dir() {
        error!("Directory {:?} does not exist", parent.to_str());
        return false;
    }
    match fs::metadata(parent) {
        Ok(md) => !md.permissions().readonly(),
        Err(e) => {
            error!("Cannot read directory metadata: {e}");
            false
        }
    }
}

/// Write a session file with torrent and its stats
pub async fn write(ctx: &AppContext) {
    let config = &ctx.config;
    if let Some(path) = config.output_stats.as_ref() {
        let mut data = String::with_capacity(4096);

        // fill data in struct
        data.push_str("{\"started\":\"");
        data.push_str(&ctx.started.to_rfc3339());

        // Add client info
        data.push_str("\",\"client\":\"");
        data.push_str(&ctx.client.read().await.name);

        // Add bandwidth info
        data.push_str("\",\"min_upload_rate\":");
        data.push_str(&config.min_upload_rate.to_string());
        data.push_str(",\"max_upload_rate\":");
        data.push_str(&config.max_upload_rate.to_string());

        data.push_str(",\"torrents\":[\n");
        let mut total_uploaded: u64 = 0;
        for (index, handle) in ctx.torrents.snapshot().iter().enumerate() {
            if index > 0 {
                data.push(',');
            }
            let torrent = handle.lock().await;
            total_uploaded += torrent.uploaded;
            data.push_str(&torrent.to_json());
        }
        data.push_str("\n],\"total_uploaded\":");
        data.push_str(&total_uploaded.to_string());
        data.push('}');
        if let Err(e) = tokio::fs::write(path, data.as_bytes()).await {
            error!("Cannot write stat file: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::Config;
    use crate::context::AppContext;
    use crate::json_output::{writable, write};
    use crate::torrent::Torrent;

    fn make_torrent(hash_byte: u8, name: &str, uploaded: u64) -> Torrent {
        Torrent {
            name: String::from(name),
            urls: vec![String::from("http://tracker.example.com/announce")],
            length: 1024,
            private: false,
            uploaded,
            last_announce: std::time::Instant::now(),
            info_hash: [hash_byte; 20],
            info_hash_urlencoded: String::new(),
            seeders: 1,
            leechers: 2,
            next_upload_speed: 0,
            interval: 1800,
            error_count: 0,
            encoding: None,
            min_interval: None,
            tracker_id: None,
            source_path: None,
        }
    }

    #[tokio::test]
    async fn test_write_stats_file_from_context() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("stats.json");
        let config = Config {
            output_stats: Some(output.clone()),
            min_upload_rate: 1000,
            max_upload_rate: 5000,
            ..Config::default()
        };
        let ctx = AppContext::new(config);
        ctx.torrents.insert_if_absent(make_torrent(1, "alpha", 100));
        ctx.torrents.insert_if_absent(make_torrent(2, "beta", 250));

        write(&ctx).await;

        let content = std::fs::read_to_string(&output).unwrap();
        assert!(content.starts_with(&format!("{{\"started\":\"{}\"", ctx.started.to_rfc3339())));
        let client_name = ctx.client.read().await.name.clone();
        assert!(content.contains(&format!("\"client\":\"{client_name}\"")));
        assert!(content.contains("\"min_upload_rate\":1000"));
        assert!(content.contains("\"max_upload_rate\":5000"));
        assert!(content.contains("\"alpha\""));
        assert!(content.contains("\"beta\""));
        // Exactly one separator between the two torrent entries.
        assert_eq!(content.matches(",\t{").count(), 1);
        assert!(content.ends_with("\"total_uploaded\":350}"));
    }

    #[tokio::test]
    async fn test_write_without_output_does_nothing() {
        let ctx = AppContext::new(Config::default());
        ctx.torrents.insert_if_absent(make_torrent(1, "alpha", 100));
        // Must not panic nor write anything.
        write(&ctx).await;
    }

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
    fn test_writable_trailing_slash_is_rejected() {
        // A path ending with '/' is a directory reference, not a file — always false.
        assert!(!writable("/tmp/"));
    }

    #[test]
    fn test_writable_nonexistent_parent_is_rejected() {
        assert!(!writable("/nonexistent_dir_xyz/output.json"));
    }
}
