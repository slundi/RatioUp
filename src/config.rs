use std::str::FromStr;

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{error, info};

// use crate::json_output;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// torrent port
    #[serde(skip_serializing)]
    pub port: u16,
    pub min_upload_rate: u32,   //in byte
    pub max_upload_rate: u32,   //in byte
    pub min_download_rate: u32, //in byte
    pub max_download_rate: u32, //in bytes
    /// To set the number of peers we want
    pub numwant: Option<u16>,
    // pub simultaneous_seed: u16, //useful ?
    pub client: String,
    /// Directory where torrents are saved. Default is in the working directory.
    #[serde(skip_serializing)]
    pub torrent_dir: String,
    #[serde(skip_serializing)]
    pub key_refresh_every: u16,
    /// Output file path for the JSON file.
    /// You may want somethink like `/var/www/ratio_up.json` to expose it on your web server.
    pub output_stats: Option<PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            // The port number that the client is listening on. Ports reserved for BitTorrent are typically 6881-6889. Clients may choose to give up if it cannot establish
            // a port within this range. Here ports are random between 49152 and 65534
            port: rand::rng().random_range(49152..65534),
            min_upload_rate: 8192,    //8*1024
            max_upload_rate: 2097152, //2048*1024
            min_download_rate: 8192,
            max_download_rate: 16777216, //16*1024*1024
            numwant: None,
            torrent_dir: String::from("."),
            //client: fake_torrent_client::Client::from(fake_torrent_client::clients::ClientVersion::Qbittorrent_4_4_2),
            key_refresh_every: 0,
            client: String::from("Transmission_3_00"),
            output_stats: None,
        }
    }
}
impl Config {
    pub async fn load_from_file(path: &PathBuf) -> Config {
        let result: tokio::io::Result<String> = tokio::fs::read_to_string(path).await;
        let mut config = Config::default();
        match result {
            Ok(content) => {
                let toml: Result<Config, toml::de::Error> = toml::from_str(&content);
                match toml {
                    Ok(loaded_config) => {
                        if loaded_config.is_ok() {
                            info!("Configuration loaded successfully from file.");
                            config = loaded_config;
                        } else {
                            info!("Using default configuration");
                        }
                    }
                    Err(e) => {
                        error!("Could not parse TOML: {}", e);
                        info!("Using default configuration");
                    }
                }
            }
            Err(e) => {
                error!("Could not read config file: {} {e}", path.display());
                info!("Using default configuration");
            }
        };
        config
    }

    /// Check if the config is OK and log error
    fn is_ok(&self) -> bool {
        if self.min_download_rate > self.max_download_rate {
            error!(
                "Min download rate ({}) is greater than max download rate ({})",
                self.min_download_rate, self.max_download_rate
            );
            return false;
        }
        if self.min_upload_rate > self.max_upload_rate {
            error!(
                "Min upload rate ({}) is greater than max upload rate ({})",
                self.min_upload_rate, self.max_upload_rate
            );
            return false;
        }
        true
    }
}

/// Init the client from the configuration and returns the interval to refresh client key if applicable
pub fn init_client(config: &Config) -> Option<u16> {
    let mut client = fake_torrent_client::Client::default();
    client.build(
        fake_torrent_client::clients::ClientVersion::from_str(&config.client)
            .expect("Wrong client"),
    );
    info!(
        "Client {} (key: {}, peer ID:{})",
        client.name, client.key, client.peer_id
    );
    let key_interval = client.key_refresh_every;
    let mut guard = crate::CLIENT.write().unwrap();
    *guard = Some(client);
    key_interval
}
