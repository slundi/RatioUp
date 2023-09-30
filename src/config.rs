use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Server `<IP or hostaname>:<port>`. Default is `127.0.0.1:8070`
    #[serde(skip_serializing)]
    pub server_addr: String,
    /// Log level (available options are: INFO, WARN, ERROR, DEBUG, TRACE). Default is `INFO`.
    #[serde(skip_serializing)]
    pub log_level: String,
    /// torrent port
    #[serde(skip_serializing)]
    pub port: u16,
    pub min_upload_rate: u32,   //in byte
    pub max_upload_rate: u32,   //in byte
    pub min_download_rate: u32, //in byte
    pub max_download_rate: u32, //in bytes
    //pub simultaneous_seed: u16, //useful ?
    pub client: String,
    /// Directory where torrents are saved
    #[serde(skip_serializing)]
    pub torrent_dir: String,
    /// Set a custom web root (ex: / or /ratio-up/)
    #[serde(skip_serializing)]
    pub web_root: String,
    #[serde(skip_serializing)]
    pub key_refresh_every: u16,
}
impl Default for Config {
    fn default() -> Self {
        Config {
            server_addr: "127.0.0.1:8330".to_owned(),
            log_level: "INFO".to_owned(),
            /// The port number that the client is listening on. Ports reserved for BitTorrent are typically 6881-6889. Clients may choose to give up if it cannot establish
            /// a port within this range. Here ports are random between 49152 and 65534
            port: rand::thread_rng().gen_range(49152..65534),
            min_upload_rate: 8192,    //8*1024
            max_upload_rate: 2097152, //2048*1024
            min_download_rate: 8192,
            max_download_rate: 16777216, //16*1024*1024
            torrent_dir: String::from("./torrents"),
            web_root: String::from("/"),
            //client: fake_torrent_client::Client::from(fake_torrent_client::clients::ClientVersion::Qbittorrent_4_4_2),
            key_refresh_every: 0,
            client: String::from("INVALID"),
        }
    }
}
impl Config {
    /// Load configuration in environment. Also load client.
    pub fn load_config() -> Config {
        let mut config: Config = Config::default();
        for (key, value) in std::env::vars() {
            if key == "SERVER_ADDR" {
                config.server_addr = value.clone();
            }
            if key == "LOG_LEVEL" {
                config.log_level = value.clone();
            }
            if key == "MIN_UPLOAD_RATE" {
                config.min_upload_rate = value.clone().parse::<u32>().expect("Wrong upload rate");
            }
            if key == "MAX_UPLOAD_RATE" {
                config.max_upload_rate = value.clone().parse::<u32>().expect("Wrong upload rate");
            }
            if key == "MIN_DOWNLOAD_RATE" {
                config.min_download_rate =
                    value.clone().parse::<u32>().expect("Wrong download rate");
            }
            if key == "MAX_DOWNLOAD_RATE" {
                config.max_download_rate =
                    value.clone().parse::<u32>().expect("Wrong download rate");
            }
            if key == "CLIENT" {
                config.client = value.clone();
            }
            if key == "TORRENT_DIR" {
                config.torrent_dir = value.clone();
            }
        }
        // let client = &mut *CLIENT.write().expect("Cannot get client");
        // client.build(clients::ClientVersion::from_str(&config.client).expect("Wrong client"));
        config.clone()
    }
}
