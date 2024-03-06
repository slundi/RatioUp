#![allow(non_snake_case)]

#[macro_use]
extern crate serde_derive;
extern crate rand;

use config::WebServerConfig;
use dotenv::dotenv;
use fake_torrent_client::Client;
use log::{self, error, info};
use std::sync::{Mutex, OnceLock, RwLock};
use tokio::time::Duration;

use crate::announcer::scheduler::run as run_announcer;
use crate::config::AnnouncerConfig;
use crate::webui::server::run as run_webui;

mod announcer;
mod config;
mod directory;
mod torrent;
mod webui;

static CONFIG: OnceLock<AnnouncerConfig> = OnceLock::new();
static WS_CONFIG: OnceLock<WebServerConfig> = OnceLock::new();
static CLIENT: RwLock<Option<Client>> = RwLock::new(None);
static TORRENTS: RwLock<Vec<Mutex<torrent::BasicTorrent>>> = RwLock::new(Vec::new()); // TODO: replace with mutex

fn run_key_renewer(refresh_every: u16) {
    loop {
        if let Some(client) = &mut *CLIENT.write().expect("Cannot read client") {
            client.generate_key();
        }
        std::thread::sleep(Duration::from_secs(u64::try_from(refresh_every).unwrap()));
    }
}

#[actix::main]
async fn main() {
    dotenv().ok();
    WS_CONFIG.get_or_init(|| WebServerConfig::load());
    let config = AnnouncerConfig::load();
    CONFIG.get_or_init(|| config.clone());
    //configure logger
    simple_logger::init_with_level(match &config.log_level as &str {
        "WARN" => log::Level::Warn,
        "ERROR" => log::Level::Error,
        "DEBUG" => log::Level::Debug,
        "TRACE" => log::Level::Trace,
        _ => log::Level::Info,
    })
    .unwrap();

    // schedule client refresh key if applicable
    if let Some(refresh_every) = config::init_client(&config) {
        std::thread::spawn(move || run_key_renewer(refresh_every));
    }

    directory::prepare_torrent_folder(&config.torrent_dir);
    let wait_time = directory::load_torrents(&config.torrent_dir);

    tokio::spawn(async move {
        // graceful exit when Ctrl + C
        tokio::signal::ctrl_c().await.unwrap();
        announcer::tracker::announce_stopped();
    });
    // Spawn probes (background thread)
    if WS_CONFIG.get().unwrap().disabled {
        run_announcer(wait_time);
    } else {
        std::thread::spawn(move || run_announcer(wait_time));
        run_webui().await // start web server
    }
}

/// Add a torrent to the list. If the filename does not end with .torrent, the file is not processed.
/// It returns the time to wait before anouncing.
fn add_torrent(path: String) -> u64 {
    let mut interval = u64::MAX;
    if path.to_lowercase().ends_with(".torrent") {
        let config = CONFIG.get().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        info!("Loading torrent: \t{}", path);
        let t = torrent::from_file(path.clone());
        match t {
            Ok(torrent) => {
                let mut t = torrent::from_torrent(torrent, path);
                if config.min_download_rate > 0 && config.max_download_rate > 0 {
                    t.downloaded = 0;
                } else {
                    t.downloaded = t.length;
                }
                for items in list.iter() {
                    let existing = items.lock().unwrap();
                    if existing.info_hash == t.info_hash {
                        info!("Torrent is already in list");
                        return interval;
                    }
                }
                t.interval =
                    announcer::tracker::announce(&mut t, Some(announcer::tracker::Event::Started));
                interval = t.interval;
                list.push(Mutex::new(t));
            }
            Err(e) => error!("Cannot parse torrent: \t{} {:?}", path, e),
        }
    }
    interval
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Test if it creates the torrent directory and do not panic when it exists
    #[test]
    fn test_torrent_directory() {
        let mut dir = env::temp_dir();
        dir.push("ratioup-test-torrents-dir");
        if dir.is_dir() {
            let _ = std::fs::remove_dir(dir.clone());
        }
        directory::prepare_torrent_folder(&dir.display().to_string());
        assert!(dir.is_dir());
        directory::prepare_torrent_folder(&dir.display().to_string());
    }
}
