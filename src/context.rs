//! Application context: the shared state that used to live in global statics.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use fake_torrent_client::Client;
use fake_torrent_client::clients::ClientVersion;
use tokio::sync::RwLock;
use tracing::{error, info};

use crate::config::Config;
use crate::repository::torrents::TorrentRepository;

/// HTTP timeout used for tracker announces.
const HTTP_TIMEOUT_SECS: u64 = 60;

/// Everything the services need at runtime, built once at startup and shared
/// as `Arc<AppContext>`.
#[derive(Debug)]
pub struct AppContext {
    pub config: Config,
    /// Start time of the session, reported in the JSON stats.
    pub started: DateTime<Utc>,
    /// Emulated torrent client. Write-locked when its key is renewed.
    pub client: RwLock<Client>,
    /// HTTP client used for tracker announces (user agent of the emulated client).
    pub http: reqwest::Client,
    pub torrents: TorrentRepository,
}

impl AppContext {
    pub fn new(config: Config) -> Self {
        let client = build_client(&config.client);
        let http = build_http_client(&client.user_agent);
        Self {
            config,
            started: Utc::now(),
            client: RwLock::new(client),
            http,
            torrents: TorrentRepository::new(),
        }
    }
}

/// Build the emulated client, falling back to the default one when the name is unknown.
fn build_client(name: &str) -> Client {
    let mut client = Client::default();
    match ClientVersion::from_str(name) {
        Ok(selected) => client.build(selected),
        Err(e) => error!("Client {name} does not exist, using default one: {e}"),
    }
    info!(
        "Client {} (key: {}, peer ID:{})",
        client.name, client.key, client.peer_id
    );
    client
}

fn build_http_client(user_agent: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(user_agent)
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .expect("Failed to build HTTP client")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_new_context_uses_configured_client() {
        let config = Config {
            client: String::from("Transmission_3_00"),
            ..Config::default()
        };
        let ctx = AppContext::new(config);
        let client = ctx.client.read().await;
        assert!(
            client.name.to_lowercase().contains("transmission"),
            "unexpected client name: {}",
            client.name
        );
        assert!(!client.user_agent.is_empty());
        assert_eq!(ctx.torrents.len(), 0);
    }

    #[tokio::test]
    async fn test_new_context_unknown_client_falls_back_to_default() {
        let config = Config {
            client: String::from("NotARealClient_0_0"),
            ..Config::default()
        };
        // Must not panic; the default client is kept.
        let ctx = AppContext::new(config);
        assert_eq!(ctx.config.client, "NotARealClient_0_0");
        let _client = ctx.client.read().await;
    }

    #[test]
    fn test_started_is_set_at_creation() {
        let before = Utc::now();
        let ctx = AppContext::new(Config::default());
        let after = Utc::now();
        assert!(before <= ctx.started && ctx.started <= after);
    }
}
