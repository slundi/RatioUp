use std::path::PathBuf;

use tracing::{error, info, warn};

#[derive(Debug, Clone)]
pub struct Config {
    /// torrent port
    pub port: u16,
    pub min_upload_rate: u32,
    pub max_upload_rate: u32,

    pub use_pid_file: bool,

    /// To set the number of peers we want
    pub numwant: Option<u16>,
    pub client: String,
    /// Directory where torrents are saved. Default is in the working directory.
    pub torrent_dir: PathBuf,
    /// Output file path for the JSON file.
    /// You may want something like `/var/www/ratio_up.json` to expose it on your web server.
    pub output_stats: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: fastrand::u16(49152..65534),
            min_upload_rate: 8192,
            max_upload_rate: 2097152,
            use_pid_file: false,
            numwant: None,
            torrent_dir: PathBuf::from("."),
            client: String::from("Transmission_3_00"),
            output_stats: None,
        }
    }
}

impl Config {
    pub async fn load_from_file(path: &PathBuf) -> Config {
        match tokio::fs::read_to_string(path).await {
            Ok(content) => {
                let config_value = match toml_span::parse(&content) {
                    Ok(val) => val,
                    Err(e) => {
                        error!("Cannot load config file: {e}");
                        return Config::default();
                    }
                };

                let mut config = Config::default();

                let root_table = match config_value.as_table() {
                    Some(table) => table,
                    None => {
                        error!("Invalid type in config file");
                        return config;
                    }
                };

                if let Some(v) = root_table.get("client") {
                    if let Some(s) = v.as_str() {
                        config.client = String::from(s);
                    } else {
                        error!("client is not a string");
                    }
                }

                if let Some(v) = root_table.get("port") {
                    if let Some(port) = v.as_integer() {
                        if !(1..=65535).contains(&port) {
                            error!("Invalid port");
                        } else {
                            config.port = port as u16;
                        }
                    } else {
                        error!("port is not an integer");
                    }
                }

                if let Some(v) = root_table.get("numwant") {
                    if let Some(numwant) = v.as_integer() {
                        if !(1..=65535).contains(&numwant) {
                            error!("Invalid numwant");
                        } else {
                            config.numwant = Some(numwant as u16);
                        }
                    } else {
                        error!("numwant is not an integer");
                    }
                }

                if let Some(v) = root_table.get("use_pid_file") {
                    if let Some(b) = v.as_bool() {
                        config.use_pid_file = b;
                    } else {
                        error!("use_pid_file is not a boolean");
                    }
                }

                if let Some(v) = root_table.get("min_upload_rate") {
                    if let Some(value) = v.as_integer() {
                        config.min_upload_rate = value as u32;
                    } else {
                        error!("Invalid min_upload_rate");
                        return config;
                    }
                }

                if let Some(v) = root_table.get("max_upload_rate") {
                    if let Some(value) = v.as_integer() {
                        config.max_upload_rate = value as u32;
                    } else {
                        error!("Invalid max_upload_rate");
                        return config;
                    }
                }

                if let Some(v) = root_table.get("torrent_dir") {
                    if let Some(s) = v.as_str() {
                        config.torrent_dir = PathBuf::from(s);
                    } else {
                        error!("Invalid torrent_dir");
                    }
                }

                if let Some(v) = root_table.get("output_stats") {
                    if let Some(s) = v.as_str() {
                        config.output_stats = Some(PathBuf::from(s));
                    } else {
                        error!("Invalid output_stats");
                    }
                }

                if !config.speeds_ok() {
                    warn!(
                        "Min upload rate ({}) is greater than max upload rate ({}), switching values",
                        config.min_upload_rate, config.max_upload_rate
                    );
                    std::mem::swap(&mut config.min_upload_rate, &mut config.max_upload_rate);
                }

                config
            }
            Err(e) => {
                error!("Could not read config file: {} {e}", path.display());
                info!("Using default configuration");
                Config::default()
            }
        }
    }

    fn speeds_ok(&self) -> bool {
        self.min_upload_rate <= self.max_upload_rate
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::Config;

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

    // --- load_from_file ---

    #[tokio::test]
    async fn test_load_from_file_valid_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(
            &path,
            b"port = 51413\nmin_upload_rate = 8192\nmax_upload_rate = 1048576\n",
        )
        .await
        .unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert_eq!(cfg.port, 51413);
        assert_eq!(cfg.min_upload_rate, 8192);
        assert_eq!(cfg.max_upload_rate, 1_048_576);
    }

    #[tokio::test]
    async fn test_load_from_file_missing_file() {
        let cfg = Config::load_from_file(&PathBuf::from("/nonexistent/ratioup_config.toml")).await;
        assert!(cfg.speeds_ok());
    }

    #[tokio::test]
    async fn test_load_from_file_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(&path, b"this is ::: not valid toml !!!")
            .await
            .unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert!(cfg.speeds_ok());
    }

    #[tokio::test]
    async fn test_load_from_file_swapped_rates_are_corrected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(
            &path,
            b"min_upload_rate = 2097152\nmax_upload_rate = 8192\n",
        )
        .await
        .unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert!(cfg.min_upload_rate <= cfg.max_upload_rate);
    }

    #[tokio::test]
    async fn test_load_from_file_invalid_port_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(&path, b"port = 0\n").await.unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert_ne!(cfg.port, 0);
    }

    #[tokio::test]
    async fn test_load_from_file_numwant() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(&path, b"numwant = 50\n").await.unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert_eq!(cfg.numwant, Some(50));
    }

    #[tokio::test]
    async fn test_load_from_file_use_pid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(&path, b"use_pid_file = true\n")
            .await
            .unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert!(cfg.use_pid_file);
    }

    #[tokio::test]
    async fn test_load_from_file_torrent_dir_and_output_stats() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(
            &path,
            b"torrent_dir = \"/tmp/torrents\"\noutput_stats = \"/tmp/stats.json\"\n",
        )
        .await
        .unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert_eq!(cfg.torrent_dir, PathBuf::from("/tmp/torrents"));
        assert_eq!(cfg.output_stats, Some(PathBuf::from("/tmp/stats.json")));
    }

    #[tokio::test]
    async fn test_load_from_file_client_string() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        tokio::fs::write(&path, b"client = \"qBittorrent_4_60\"\n")
            .await
            .unwrap();
        let cfg = Config::load_from_file(&path).await;
        assert_eq!(cfg.client, "qBittorrent_4_60");
    }
}
