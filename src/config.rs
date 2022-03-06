use serde::{Serialize, Deserialize};
use std::error::Error;
use std::fs::File;
use std::io::BufReader;

//load config file: client, min/max speed, seed_if_zero_leecher

#[derive(Default, Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub client: String,
    pub min_upload_rate: u32, //in byte
    pub max_upload_rate: u32, //in byte
    pub seed_if_zero_leecher: bool,
    //pub simultaneous_seed: u16, //useful ?
}

impl<'a> Config {
    fn default() -> Self { Config {
        min_upload_rate: 8*1024, max_upload_rate: 2048*1024,
        seed_if_zero_leecher: false,
        //simultaneous_seed:5,
        client: "qbittorrent-4.3.3".to_owned(),
    }}
}

pub fn get_config(path: &str) -> Config {
    let cfg=read_config_file(path.to_owned());
    if cfg.is_ok() {return cfg.unwrap();}
    //cfg not OK, initializing with default configuration
    return Config::default();
}

pub fn read_config_file(path: String) -> Result<Config, Box<dyn Error>> {
    let file = File::open(&path).expect("File should open in read only");
    let reader = BufReader::new(file); //remove buffer?
    let c = serde_json::from_reader(reader).expect("Unable to parse configuration file: JSON not valid!");
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_read_config() {
        let mut d = std::env::temp_dir(); d.push("ratioup.json");
        let path:String = String::from(d.to_str().unwrap());
        if std::path::Path::new(&path).exists() {std::fs::remove_file(d);}
        //create the file for the test
        let mut f : File = std::fs::File::create(std::path::Path::new(&path)).expect("Unable to create file");
        f.write_all("{\"client\":\"qbittorrent-4.3.3\", \"min_upload_rate\": 8, \"max_upload_rate\": 2048, \"seed_if_zero_leecher\": true, \"simultaneous_seed\": 5}".as_bytes());
        f.flush();
        let cfg = read_config_file(path).unwrap();
        assert_eq!(cfg.min_upload_rate, 8*1024);
        assert_eq!(cfg.max_upload_rate, 2048*2048);
        assert_eq!(cfg.seed_if_zero_leecher, true);
        //assert_eq!(cfg.simultaneous_seed, 5);
        assert_eq!(cfg.client, String::from("qbittorrent-4.3.3"));
    }
}
