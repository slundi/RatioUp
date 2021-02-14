use std::collections::HashMap;
use serde::Deserialize;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use globset::Glob;

enum Refresh {
    NEVER,
    TIMED_OR_AFTER_STARTED_ANNOUNCE,
    TORRENT_VOLATILE,
    TORRENT_PERSISTENT,
}
enum Key_Case {NONE, LOWER, UPPER}
enum Case {LOWER, UPPER}
#[derive(Deserialize, Debug)]
enum Algorithm_Method {
    HASH_NO_LEADING_ZERO,
    HASH,
    DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES,
    REGEX,
    ///for peer ID
    /// RANDOM_POOL_WITH_CHECKSUM,
}

#[derive(Deserialize, Debug)]
struct Algorithm {
    method: Algorithm_Method,
    ///for HASH_NO_LEADING_ZERO, HASH methods
    length: Option<u8>,
    ///for REGEX method
    pattern: Option<String>,
    ///for DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES
    inclusive_lower_bound: Option<u32>,
    ///for DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES
    inclusive_upper_bound: Option<u32>,
    /// for RANDOM_POOL_WITH_CHECKSUM
    prefix: String,
    /// for RANDOM_POOL_WITH_CHECKSUM
    character_pool: Option<String>,
    /// for RANDOM_POOL_WITH_CHECKSUM
    base: Option<u8>,
}

#[derive(Deserialize, Debug)]
struct Generator {
    algorithm: Algorithm,
    refresh_on: Refresh,
    should_url_encode: bool,
    refresh_every: u8,
}

#[derive(Deserialize, Debug)]
struct URL_Encocer {
    encoding_exclusion_pattern: String,
    /// if the encoded hex string should be in upper case or no
    uppercase_encoded_hex: bool,
}

#[derive(Deserialize, Debug)]
struct Client {
    key_generator: Generator,
    peer_id_generator = Generator,
    url_encoder = URL_Encocer,
    //https://docs.rs/reqwest/0.11.0/reqwest/
    request_headers: HashMap<String, String>,
    num_want: u16,
    num_want_on_stom: u16,
}

fn read_config_file<P: AsRef<Path>>(path: P) -> Result<Client, Box<Error>> {
    let file = File::open(path).expect("File should open in read only");
    let reader = BufReader::new(file); //remove buffer?
    let c = serde_json::from_reader(reader).expect("Unable to parse configuration file: JSON not valid!");
    Ok(c);
}

fn list_clients<P: AsRef<Path>>(path: P) -> Vec<String> {
    let mut result=vec<String>::with_capacity();
    let glob = Glob::new("*.client")?.compile_matcher();
    for entry in glob("*.client").expect("Failed to browse client folder") {
        match entry {
            Ok(path) => println!("{:?}", path.display()),
            Err(e) => println!("{:?}", e),
        }
}
