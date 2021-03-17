use serde::{Serialize, Deserialize};
use std::{error::Error, io::Write};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use log::{info, error};

//load config file: client, min/max speed, seed_if_zero_leecher

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct Config {
    pub client: String,
    pub min_upload_rate: u16,
    pub max_upload_rate: u16,
    pub seed_if_zero_leecher: bool,
    //pub simultaneous_seed: u16, //useful ?
}

impl<'a> Config {
    fn default() -> Self { Config {
        min_upload_rate: 8, max_upload_rate: 2048,
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

pub async fn write_config_file(path: &str, cfg: &Config) {
    let data=serde_json::to_string_pretty(&cfg);
    tokio::fs::write(&path, data.unwrap().as_bytes()).await.expect("Unable to write file config.json");
    /*let mut file: File;
    let p=Path::new(&path);
    if p.exists() {file = File::open(path).expect("Unable to open file config.json for writing");}
    else {file=File::create(p).expect("Unable to create file config.json");}
    if file.write_all(data.unwrap().as_bytes()).is_err() {error!("Error while writing config.json");}
    if file.flush().is_err() {error!("Cannot write config.json");}
    //println!("{:?}", serde_json::json!(&*cfg))*/
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_config() {
        //create file if not exists
        let mut d = std::env::temp_dir(); d.push("ratioup.json");
        let path:String = String::from(d.to_str().unwrap());
        if std::path::Path::new(&path).exists() {std::fs::remove_file(d);}
        let mut cfg=Config::default();
        assert_eq!(cfg.min_upload_rate, 8);
        assert_eq!(cfg.max_upload_rate, 2048);
        assert_eq!(cfg.seed_if_zero_leecher, false);
        //assert_eq!(cfg.simultaneous_seed, 5);
        assert_eq!(cfg.client, String::from("qbittorrent-4.3.3"));
        write_config_file(&path, cfg);
    }
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
        assert_eq!(cfg.min_upload_rate, 8);
        assert_eq!(cfg.max_upload_rate, 2048);
        assert_eq!(cfg.seed_if_zero_leecher, true);
        //assert_eq!(cfg.simultaneous_seed, 5);
        assert_eq!(cfg.client, String::from("qbittorrent-4.3.3"));
    }
}
