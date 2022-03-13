use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::Path;
use byte_unit::Byte;
use tracing::{info, error, Subscriber};
use rand::Rng;
use crate::algorithm;

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
    pub client: String,
    /// torrent port: random between 49152 and 65534
    pub port: u16,
    pub min_upload_rate: u32, //in byte
    pub max_upload_rate: u32, //in byte
    pub seed_if_zero_leecher: bool,
    pub seed_public_torrent: bool,
    //pub simultaneous_seed: u16, //useful ?

    //Client configuration
    //----------- algorithms
    ///key algorithm
    #[serde(skip_serializing)] key_algorithm: u8, //length=8
    #[serde(skip_serializing)] key_length: u8,
    ///for REGEX method, for RANDOM_POOL_WITH_CHECKSUM: list pf available chars, the base is the length of the string
    #[serde(skip_serializing)] key_pattern: String,
    /// for RANDOM_POOL_WITH_CHECKSUM
    #[serde(skip_serializing)] prefix: String,
    #[serde(skip_serializing)] key_refresh_on: u8,
    #[serde(skip_serializing)] key_refresh_every: u16,
    #[serde(skip_serializing)] key_uppercase: Option<bool>,

    //----------- peer ID
    #[serde(skip_serializing)] peer_algorithm: u8,
    #[serde(skip_serializing)] peer_pattern: String,
    #[serde(skip_serializing)] peer_refresh_on: u8,
    #[serde(skip_serializing)] peer_prefix:String,

    //----------- URL encoder 
    #[serde(skip_serializing)] encoding_exclusion_pattern: String,
    /// if the encoded hex string should be in upper case or no
    #[serde(skip_serializing)] uppercase_encoded_hex: bool,
    #[serde(skip_serializing)] should_url_encode: bool,

    #[serde(skip_serializing)] pub query: String,
    //request_headers: HashMap<String, String>, //HashMap<&str, i32> = [("Norway", 100), ("Denmark", 50), ("Iceland", 10)]
    #[serde(skip_serializing)] pub user_agent: String,
    #[serde(skip_serializing)] pub accept:String,
    #[serde(skip_serializing)] pub accept_encoding: String,
    #[serde(skip_serializing)] pub accept_language: String,
    #[serde(skip_serializing)] pub connection:Option<String>,
    #[serde(skip_serializing)] pub num_want: u16,
    #[serde(skip_serializing)] pub num_want_on_stop: u16,

    //generated values
    #[serde(skip_serializing)] pub infohash :String,
    #[serde(skip_serializing)] pub peer_id: String,
}

impl Config {
    fn default() -> Self { Config {
        min_upload_rate: 8*1024, max_upload_rate: 2048*1024,
        seed_if_zero_leecher: false, seed_public_torrent: false,
        //simultaneous_seed:5,
        client: "qbittorrent-4.3.3".to_owned(),
        port: rand::thread_rng().gen_range(49152..65534),

        //client configuration
        //key generator default values
        key_algorithm: HASH,
        key_length: 0,
        key_pattern:String::new(), prefix:String::new(),
        key_uppercase: None,
        key_refresh_on: TIMED_OR_AFTER_STARTED_ANNOUNCE,
        key_refresh_every: 0,
        //peer ID generator
        peer_algorithm: HASH,
        peer_pattern: String::new(),
        peer_prefix:String::new(),
        peer_refresh_on: NEVER,
        //URL encoder
        encoding_exclusion_pattern: r"[A-Za-z0-9-]".to_owned(),
        uppercase_encoded_hex: false,
        should_url_encode: false,
        //misc
        num_want: 200,
        num_want_on_stop: 0,
        //query headers
        query: "info_hash={infohash}&peer_id={peerid}&port={port}&uploaded={uploaded}&downloaded={downloaded}&left={left}&corrupt=0&key={key}&event={event}&numwant={numwant}&compact=1&no_peer_id=1".to_owned(),
        user_agent: String::new(), //must be defined
        accept: String::new(),
        accept_encoding: String::from("gzip"),
        accept_language: String::new(),
        connection: Some(String::from("Close")),
        infohash: String::new(),
        peer_id: String::new(),
    }}
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
    cfg.seed_if_zero_leecher = v["seed_if_zero_leecher"].as_bool().expect("Cannot get the seed_if_zero_leecher in config.json");
    cfg.client               = v["client"].as_str().expect("Cannot get the client in config.json").to_owned();
    cfg.num_want             = v["numwant"].as_u64().expect("Cannot get numwant in config.json") as u16;
    cfg.num_want_on_stop     = v["numwant_on_stop"].as_u64().expect("Cannot get numwant_on_stop in config.json") as u16;
    info!("Client: {}", cfg.client);
    info!("Bandwidth: {} - {}", Byte::from_bytes(cfg.min_upload_rate as u128).get_appropriate_unit(true).to_string(), Byte::from_bytes(cfg.max_upload_rate as u128).get_appropriate_unit(true).to_string());
    //get client from xxxxxxxxxxx.client
    let file = File::open(format!("{}{}{}", "./res/clients/", cfg.client.as_str(), ".client")).expect("Cannot open client file");
    let mut buffer = String::with_capacity(4096);
    BufReader::new(file).read_to_string(& mut buffer).expect("Cannot read client file");
    let v: Value = serde_json::from_str(&buffer).expect("Unable to parse client file: JSON not valid");
    cfg.query = v["query"].as_str().expect("Cannot get announce query on client file").to_owned();
    //if v["numwant"].is_u64() {cfg.num_want = v["numwant"].as_u64().unwrap() as u16;} //numwant is defined in the config
    //if v["numwantOnStop"].is_u64() {cfg.num_want = v["numwantOnStop"].as_u64().unwrap() as u16;}
    if v["requestHeaders"].is_array() {
        let a = v["requestHeaders"].as_array().unwrap();
        for rh in a {
            if rh["name"].as_str().unwrap() == "User-Agent" {cfg.user_agent = rh["value"].as_str().unwrap().to_owned();}
            if rh["name"].as_str().unwrap() == "Accept-Encoding" {cfg.accept_encoding = rh["value"].as_str().unwrap().to_owned();}
            if rh["name"].as_str().unwrap() == "Connection" {cfg.connection = Some(rh["value"].as_str().unwrap().to_owned());}
            if rh["name"].as_str().unwrap() == "Accept" {cfg.accept = rh["value"].as_str().unwrap().to_owned();}
        }
    }
    //key generator
    if v["keyGenerator"].is_object() {
        match v["keyGenerator"]["algorithm"]["type"].as_str().expect("Cannot get client key generator type") {
            "REGEX" => cfg.key_algorithm = REGEX,
            "HASH" => cfg.key_algorithm = HASH,
            "HASH_NO_LEADING_ZERO" => cfg.key_algorithm = HASH_NO_LEADING_ZERO,
            "DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES" => cfg.key_algorithm = DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES,
            "RANDOM_POOL_WITH_CHECKSUM" => cfg.key_algorithm = RANDOM_POOL_WITH_CHECKSUM,
            //"PEER_ID_LENGTH" => cfg.key_algorithm = PEER_ID_LENGTH,
            _ => panic!("Cannot get a valid key generator type"),
        }
    }
    if v["keyGenerator"]["algorithm"].get("length").is_some() {cfg.key_length = v["keyGenerator"]["algorithm"]["length"].as_u64().unwrap() as u8;}
    if v["keyGenerator"]["refreshEvery"].is_u64() {cfg.key_refresh_every = v["keyGenerator"]["refreshEvery"].as_u64().unwrap() as u16;}
    if v["keyGenerator"]["refreshOn"].is_string() {
        if v["keyGenerator"]["refreshOn"].as_str().unwrap() == "TORRENT_PERSISTENT" {cfg.key_refresh_on = TORRENT_PERSISTENT;}
        if v["keyGenerator"]["refreshOn"].as_str().unwrap() == "TIMED_OR_AFTER_STARTED_ANNOUNCE" {cfg.key_refresh_on = TIMED_OR_AFTER_STARTED_ANNOUNCE;}
        if v["keyGenerator"]["refreshOn"].as_str().unwrap() == "TORRENT_VOLATILE" {cfg.key_refresh_on = TORRENT_VOLATILE;}
    }
    if v["keyGenerator"]["keyCase"].is_string() {
        if v["keyGenerator"]["keyCase"].as_str().unwrap() == "upper" {cfg.key_uppercase = Some(true);}
        else {cfg.key_uppercase = Some(false);}
    }
    //peer ID generator
    if v["peerIdGenerator"].is_object() {
        match v["peerIdGenerator"]["algorithm"]["type"].as_str().expect("Cannot get peer ID generator type") {
            "REGEX" => cfg.peer_algorithm = REGEX,
            "HASH" => cfg.peer_algorithm = HASH,
            "HASH_NO_LEADING_ZERO" => cfg.peer_algorithm = HASH_NO_LEADING_ZERO,
            "DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES" => {
                cfg.peer_algorithm = DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES;
                cfg.key_pattern = String::from("1-2147483647");
            },
            "RANDOM_POOL_WITH_CHECKSUM" => cfg.peer_algorithm = RANDOM_POOL_WITH_CHECKSUM,
            "TORRENT_VOLATILE" => cfg.peer_algorithm = TORRENT_VOLATILE,
            //"PEER_ID_LENGTH" => cfg.key_algorithm = PEER_ID_LENGTH,
            _ => panic!("Cannot get a valid peer ID type"),
        }
        if v["peerIdGenerator"]["algorithm"].get("pattern").is_some() {cfg.peer_pattern = v["peerIdGenerator"]["algorithm"]["pattern"].as_str().unwrap().to_owned();}
        if v["peerIdGenerator"]["refreshOn"].is_string() {
            if v["peerIdGenerator"]["refreshOn"].as_str().unwrap() == "NEVER" {cfg.peer_refresh_on = NEVER;}
        }
        if v["peerIdGenerator"]["shouldUrlEncode"].is_boolean() {cfg.should_url_encode = v["peerIdGenerator"]["shouldUrlEncode"].as_bool().unwrap();}
    }
    //URL encoder
    if v["urlEncoder"].is_object() {
        if v["urlEncoder"]["encodingExclusionPattern"].is_string() {cfg.encoding_exclusion_pattern = v["urlEncoder"]["encodingExclusionPattern"].as_str().unwrap().to_owned();}
        if v["urlEncoder"]["encodedHexCase"].is_string() {cfg.uppercase_encoded_hex = v["urlEncoder"]["encodedHexCase"].as_str().unwrap() == "upper";}
    }
    //build keys
    //generate PEER_ID
    if cfg.peer_algorithm == REGEX {
        cfg.peer_id = algorithm::regex(cfg.peer_pattern.replace("\\", "")); //replace \ otherwise the generator crashes
    }
    else {algorithm::random_pool_with_checksum(PEER_ID_LENGTH, &cfg.peer_prefix, &cfg.peer_pattern);}
    //info!("Peer ID: {}", cfg.peer_id); //do not log it because weird chars
    //generate KEY
    if cfg.key_algorithm == HASH {algorithm::hash(8, false, cfg.key_uppercase);}
    else if cfg.key_algorithm == HASH_NO_LEADING_ZERO {algorithm::hash(8, true, cfg.key_uppercase);}
    else if cfg.key_algorithm == DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES {algorithm::digit_range_transformed_to_hex_without_leading_zero();}
    return cfg;
}

/// Write a default configuration file from the given path. This fonction is call at the program stratup to generate the first config file if missing.
pub fn write_default(path: String) {
    let file = std::fs::File::create(&path);
    if file.is_ok() {serde_json::to_writer_pretty(&file.unwrap(), &Config::default()).expect("Cannot write configuration file");}
    else {error!("Cannot generate the configuration file");panic!();}
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    #[test]
    fn test_read_config() {
        let mut d = std::env::temp_dir(); d.push("ratioup.json");
        let path:String = String::from(d.to_str().unwrap());
        if std::path::Path::new(&path).exists() {assert_eq!(true, std::fs::remove_file(d).is_ok());}
        //create the file for the test
        let mut f : File = std::fs::File::create(std::path::Path::new(&path)).expect("Unable to create file");
        assert_eq!(true, f.write_all("{\"client\":\"qbittorrent-4.3.3\", \"min_upload_rate\": 8, \"max_upload_rate\": 2048, \"seed_if_zero_leecher\": true, \"simultaneous_seed\": 5}".as_bytes()).is_ok());
        assert_eq!(true, f.flush().is_ok());
        let cfg = get_config(&path);
        assert_eq!(cfg.min_upload_rate, 8*1024);
        assert_eq!(cfg.max_upload_rate, 2048*2048);
        assert_eq!(cfg.seed_if_zero_leecher, true);
        //assert_eq!(cfg.simultaneous_seed, 5);
        assert_eq!(cfg.client, String::from("qbittorrent-4.3.3"));
    }
}
