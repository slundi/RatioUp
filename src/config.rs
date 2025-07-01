use std::str::FromStr;

use std::path::PathBuf;
use toml::Value;
use tracing::{error, info, warn};

// use crate::json_output;

#[derive(Debug, Clone)]
pub struct Config {
    /// torrent port
    pub port: u16,
    pub min_upload_rate: u32, //in byte
    pub max_upload_rate: u32, //in byte

    pub use_pid_file: bool,

    // /// when announcing on HTTPS tracker, do we check the SSL certificate
    // pub check_https_certs: bool,
    /// To set the number of peers we want
    pub numwant: Option<u16>,
    // pub simultaneous_seed: u16, //useful ?
    pub client: String,
    /// Directory where torrents are saved. Default is in the working directory.
    pub torrent_dir: PathBuf,
    // pub key_refresh_every: u16,
    /// Output file path for the JSON file.
    /// You may want somethink like `/var/www/ratio_up.json` to expose it on your web server.
    pub output_stats: Option<PathBuf>,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            // The port number that the client is listening on. Ports reserved for BitTorrent are typically 6881-6889. Clients may choose to give up if it cannot establish
            // a port within this range. Here ports are random between 49152 and 65534
            port: fastrand::u16(49152..65534),
            min_upload_rate: 8192,    //8*1024
            max_upload_rate: 2097152, //2048*1024
            // check_https_certs: false,
            use_pid_file: false,
            numwant: None,
            torrent_dir: PathBuf::from("."),
            //client: fake_torrent_client::Client::from(fake_torrent_client::clients::ClientVersion::Qbittorrent_4_4_2),
            // key_refresh_every: 0,
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
                let config_value: Value = match toml::from_str(&content) {
                    Ok(val) => val,
                    Err(e) => {
                        error!("Cannot load config file: {e}");
                        return config;
                    }
                };

                let root_table = match config_value {
                    Value::Table(table) => table,
                    _ => {
                        error!("Invalid type in config file");
                        return config;
                    }
                };

                if let Some(client) = root_table.get("client") {
                    if let Some(client) = client.as_str() {
                        config.client = String::from(client);
                    } else {
                        error!("Client is not a string");
                    }
                }

                if let Some(port) = root_table.get("port") {
                    if let Some(port) = port.as_integer() {
                        if !(1..=65535).contains(&port) {
                            error!("Invalid port");
                        } else {
                            config.port = port as u16;
                        }
                    } else {
                        error!("port is not an integer");
                    }
                };

                if let Some(numwant) = root_table.get("numwant") {
                    if let Some(numwant) = numwant.as_integer() {
                        if !(1..=65535).contains(&numwant) {
                            error!("Invalid numwant");
                        } else {
                            config.numwant = Some(numwant as u16);
                        }
                    } else {
                        error!("numwant is not an integer");
                    }
                };

                if let Some(pid) = root_table.get("use_pid_file") {
                    if let Some(pid) = pid.as_bool() {
                        config.use_pid_file = pid;
                    } else {
                        error!("use_pid_file is not an integer");
                    }
                    match bool::from_str(&pid.to_string()) {
                        Ok(value) => config.use_pid_file = value,
                        Err(e) => {
                            error!("Invalid use_pid: {e}");
                            return config;
                        }
                    }
                }

                if let Some(speed) = root_table.get("min_upload_rate") {
                    if let Some(value) = speed.as_integer() {
                        config.min_upload_rate = value as u32;
                    } else {
                        error!("Invalid min upload rate");
                        return config;
                    }
                }
                if let Some(speed) = root_table.get("max_upload_rate") {
                    if let Some(value) = speed.as_integer() {
                        config.max_upload_rate = value as u32;
                    } else {
                        error!("Invalid max upload rate");
                        return config;
                    }
                }

                if let Some(dir) = root_table.get("torrent_dir") {
                    if let Some(dir) = dir.as_str() {
                        config.torrent_dir = PathBuf::from(dir);
                    } else {
                        error!("Invalid torrent_dir");
                    }
                }

                if let Some(value) = root_table.get("output_stats") {
                    if let Some(path) = value.as_str() {
                        config.output_stats = Some(PathBuf::from(path));
                    } else {
                        error!("Invalid output_stats");
                    }
                }
            }
            Err(e) => {
                error!("Could not read config file: {} {e}", path.display());
                info!("Using default configuration");
            }
        };

        if !config.speeds_ok() {
            warn!(
                "Min upload rate ({}) is greater than max upload rate ({}), switching values",
                config.min_upload_rate, config.max_upload_rate
            );
            std::mem::swap(&mut config.min_upload_rate, &mut config.max_upload_rate);
        }

        config
    }

    fn speeds_ok(&self) -> bool {
        self.min_upload_rate <= self.max_upload_rate
    }
}

/// Init the client from the configuration and returns the interval to refresh client key if applicable
pub async fn init_client(config: &Config) -> Option<u16> {
    let mut client = fake_torrent_client::Client::default();
    match fake_torrent_client::clients::ClientVersion::from_str(&config.client) {
        Ok(selected) => {
            client.build(selected);
        }
        Err(e) => {
            error!(
                "Client {} does not exist, using default one: {e}",
                config.client
            );
        }
    }
    info!(
        "Client {} (key: {}, peer ID:{})",
        client.name, client.key, client.peer_id
    );
    let key_interval = client.key_refresh_every;
    let mut guard = crate::CLIENT.write().await;
    *guard = Some(client);
    key_interval
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    #[test]
    fn test_speed_ok() {
        let mut cfg = Config::default();
        assert!(cfg.speeds_ok());

        cfg.min_upload_rate = 8192;
        cfg.max_upload_rate = 8192;
        assert!(cfg.speeds_ok());

        cfg.min_upload_rate = 8192;
        cfg.max_upload_rate = 4096;
        assert!(!cfg.speeds_ok());
    }
}
