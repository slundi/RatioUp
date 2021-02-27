use serde::{Serialize, Deserialize};
use std::{error::Error, io::Write};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

//load config file: client, min/max speed, keep_torrent_with_zero_leecher

#[derive(Serialize, Deserialize, Debug)]
pub struct Config {
    //client: client::Client,
    min_upload_rate: u16,
    max_upload_rate: u16,
    keep_torrent_with_zero_leecher: bool,
    simultaneous_seed: u16,
}

pub fn read_config_file<P: AsRef<Path>>(path: P) -> Result<Config, Box<Error>> {
    let file = File::open(path).expect("File should open in read only");
    let reader = BufReader::new(file); //remove buffer?
    let c = serde_json::from_reader(reader).expect("Unable to parse configuration file: JSON not valid!");
    Ok(c)
}

pub fn write_config_file<P: AsRef<Path>>(path: P, cfg: &Config) -> std::io::Result<()> {
    let data=serde_json::to_string(cfg);
    let mut file = File::open(path)?;
    file.write_all(data?.as_bytes());
    file.flush()?;
    Ok(())
}
