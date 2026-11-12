//! Session statistics exported as a JSON file (see `output_stats` in the config).

use tracing::error;

use crate::context::AppContext;

/// Builds and writes the session statistics.
pub struct StatsService<'ctx> {
    ctx: &'ctx AppContext,
}

impl<'ctx> StatsService<'ctx> {
    pub fn new(ctx: &'ctx AppContext) -> Self {
        Self { ctx }
    }

    /// Render the session statistics as JSON.
    pub async fn render(&self) -> String {
        let config = &self.ctx.config;
        let mut data = String::with_capacity(4096);

        data.push_str("{\"started\":\"");
        data.push_str(&self.ctx.started.to_rfc3339());

        data.push_str("\",\"client\":\"");
        data.push_str(&self.ctx.client.read().await.name);

        data.push_str("\",\"min_upload_rate\":");
        data.push_str(&config.min_upload_rate.to_string());
        data.push_str(",\"max_upload_rate\":");
        data.push_str(&config.max_upload_rate.to_string());

        data.push_str(",\"torrents\":[\n");
        let mut total_uploaded: u64 = 0;
        for (index, handle) in self.ctx.torrents.snapshot().iter().enumerate() {
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
        data
    }

    /// Write the statistics to the configured output file, if any.
    pub async fn write(&self) {
        let Some(path) = self.ctx.config.output_stats.as_ref() else {
            return;
        };
        let data = self.render().await;
        if let Err(e) = tokio::fs::write(path, data.as_bytes()).await {
            error!("Cannot write stat file: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
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
    async fn test_render_contains_session_and_torrents() {
        let config = Config {
            min_upload_rate: 1000,
            max_upload_rate: 5000,
            ..Config::default()
        };
        let ctx = AppContext::new(config);
        ctx.torrents.insert_if_absent(make_torrent(1, "alpha", 100));
        ctx.torrents.insert_if_absent(make_torrent(2, "beta", 250));

        let content = StatsService::new(&ctx).render().await;
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
    async fn test_render_without_torrents() {
        let ctx = AppContext::new(Config::default());
        let content = StatsService::new(&ctx).render().await;
        assert!(content.contains("\"torrents\":[\n\n]"));
        assert!(content.ends_with("\"total_uploaded\":0}"));
    }

    #[tokio::test]
    async fn test_write_creates_configured_file() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("stats.json");
        let config = Config {
            output_stats: Some(output.clone()),
            ..Config::default()
        };
        let ctx = AppContext::new(config);
        ctx.torrents.insert_if_absent(make_torrent(1, "alpha", 100));

        let service = StatsService::new(&ctx);
        service.write().await;
        let written = std::fs::read_to_string(&output).unwrap();
        assert_eq!(written, service.render().await);
    }

    #[tokio::test]
    async fn test_write_without_output_does_nothing() {
        let ctx = AppContext::new(Config::default());
        ctx.torrents.insert_if_absent(make_torrent(1, "alpha", 100));
        // Must not panic nor write anything.
        StatsService::new(&ctx).write().await;
    }

    #[tokio::test]
    async fn test_write_to_unwritable_path_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config {
            output_stats: Some(dir.path().join("missing").join("stats.json")),
            ..Config::default()
        };
        let ctx = AppContext::new(config);
        StatsService::new(&ctx).write().await;
        assert!(!dir.path().join("missing").exists());
    }
}
