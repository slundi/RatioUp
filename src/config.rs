use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use byte_unit::Byte;
use tracing::{info, error};
use rand::Rng;
use url::form_urlencoded::byte_serialize;
use fake_torrent_client;

//refresh interval
const NEVER: u8 = 0;
const TIMED_OR_AFTER_STARTED_ANNOUNCE: u8 = 1;
const TORRENT_VOLATILE: u8 = 2;
const TORRENT_PERSISTENT: u8 = 3;

//algorithms for ket and peer generator
const REGEX: u8 = 10;
const HASH: u8 = 11;
const HASH_NO_LEADING_ZERO: u8 = 12;
const DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES: u8 = 13; //inclusive bounds: from 1 to 2147483647
const RANDOM_POOL_WITH_CHECKSUM: u8 = 14;
const PEER_ID_LENGTH: usize = 20;

//load config file: client, min/max speed, seed_if_zero_leecher

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    /// The port number that the client is listening on. Ports reserved for BitTorrent are typically 6881-6889. Clients may choose to give up if it cannot establish 
    /// a port within this range. Here ports are random between 49152 and 65534
    pub port: u16,
    pub min_upload_rate: u32, //in byte
    pub max_upload_rate: u32, //in byte
    pub min_download_rate: u32, //in byte
    pub max_download_rate: u32, //in byte
    //pub simultaneous_seed: u16, //useful ?
    pub client: fake_torrent_client::Client,
    #[serde(skip_serializing)] pub key_refresh_every: u16,
}

impl Config {
    fn default() -> Self { Config {
        min_upload_rate: 8*1024, max_upload_rate: 2048*1024,
        min_download_rate: 8*1024, max_download_rate: 16*1024*1024,
        //simultaneous_seed:5,
        client: fake_torrent_client::Client::from(fake_torrent_client::clients::ClientVersion::Qbittorrent_4_4_2),
        port: rand::thread_rng().gen_range(49152..65534),

        //client configuration
        //key generator default values
        key_refresh_every: 0,
    }}

    /// Generate the client key, and encode it for HTTP request
    pub fn generate_key(&mut self) {
        match self.key_algorithm {
            HASH => self.key = algorithm::hash(8, false, self.key_uppercase),
            HASH_NO_LEADING_ZERO => self.key = algorithm::hash(8, true, self.key_uppercase),
            DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES => self.key = algorithm::digit_range_transformed_to_hex_without_leading_zero(),
            _ => {error!("Cannot generate key"); panic!("Cannot generate pkey");},
        }
        info!("Key:     \t{}", self.key); 
    }
    /// Generate the peer ID and encode it for HTTP request
    pub fn generate_peer_id(&mut self) {
        match self.peer_algorithm {
            REGEX                     => self.peer_id = algorithm::regex(self.peer_pattern.replace("\\\\", "\\")), //replace \ otherwise the generator crashes
            RANDOM_POOL_WITH_CHECKSUM => self.peer_id = algorithm::random_pool_with_checksum(PEER_ID_LENGTH as usize, &self.peer_prefix, &self.peer_pattern),
            _ => {error!("Cannot generate peer ID"); panic!("Cannot generate peer ID");},
        }
        self.peer_id = byte_serialize(&self.peer_id.as_bytes()[0..PEER_ID_LENGTH]).collect(); //take the first 20 charsencode it because weird chars
        info!("Peer ID: \t{}", self.peer_id); 
    }

    /// Get the HTTP request with the bittorrent client headers (user-agent, accept, accept-encoding, accept-language)
    pub fn get_http_request(&self, url: &str) -> ureq::Request {
        let mut agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(60));
        if !self.user_agent.is_empty() {agent = agent.user_agent(&self.user_agent);}
        let mut req = agent.build().get(url);
        if !self.accept.is_empty() {req = req.set("accept", &self.accept);}
        if !self.accept_encoding.is_empty() {req = req.set("accept-encoding", &self.accept_encoding);}
        if !self.accept_language.is_empty() {req = req.set("accept-language", &self.accept_language);}
        req.timeout(std::time::Duration::from_secs(90))
    }
}

pub fn get_config(path: &str) -> Config {
    //get the config from config.json
    if !Path::new("config.json").exists() {panic!("config.json does not exists");}
    let mut cfg= Config::default();
    let file = File::open(&path).expect("File should open in read only");
    let mut buffer = String::with_capacity(2048);
    BufReader::new(file).read_to_string(& mut buffer).expect("Cannot read config file");
    let v: Value = serde_json::from_str(&buffer).expect("Unable to parse configuration file: JSON not valid!");
    cfg.min_upload_rate      = v["min_upload_rate"].as_u64().expect("Cannot get the min_upload_rate in config.json") as u32;
    cfg.max_upload_rate      = v["max_upload_rate"].as_u64().expect("Cannot get the min_upload_rate in config.json") as u32;
    cfg.min_download_rate    = v["min_download_rate"].as_u64().expect("Cannot get the min_download_rate in config.json") as u32;
    cfg.max_download_rate    = v["max_download_rate"].as_u64().expect("Cannot get the max_download_rate in config.json") as u32;
    //cfg.client               = v["client"].as_str().expect("Cannot get the client in config.json").to_owned();
    info!("Client: {}", cfg.client);
    info!("Bandwidth: {} - {}", Byte::from_bytes(cfg.min_upload_rate as u128).get_appropriate_unit(true).to_string(), Byte::from_bytes(cfg.max_upload_rate as u128).get_appropriate_unit(true).to_string());
    //get client from xxxxxxxxxxx.client
    //key generator
    if v["keyGenerator"]["refreshEvery"].is_u64() {cfg.key_refresh_every = v["keyGenerator"]["refreshEvery"].as_u64().unwrap() as u16;}
    cfg
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn test_read_config() {
        let mut d = std::env::temp_dir(); d.push("ratioup.json");
        let path:String = String::from(d.to_str().unwrap());
        if std::path::Path::new(&path).exists() {assert!(std::fs::remove_file(d).is_ok());}
        //create the file for the test
        let mut f : File = std::fs::File::create(std::path::Path::new(&path)).expect("Unable to create file");
        assert!(f.write_all("{\"client\":\"qbittorrent-4.3.3\", \"min_upload_rate\": 8, \"max_upload_rate\": 2048, \"seed_if_zero_leecher\": true, \"simultaneous_seed\": 5}".as_bytes()).is_ok());
        assert!(f.flush().is_ok());
        let cfg = get_config(&path);
        assert_eq!(cfg.min_upload_rate, 8*1024);
        assert_eq!(cfg.max_upload_rate, 2048*2048);
        //assert_eq!(cfg.simultaneous_seed, 5);
        assert_eq!(cfg.client, String::from("qbittorrent-4.3.3"));
    }
}
