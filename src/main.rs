#![allow(non_snake_case)]

#[macro_use]
extern crate serde_derive;
extern crate rand;

use actix::prelude::*;
use actix_files::Files;
use actix_web::{middleware, App, HttpServer};
use dotenv::dotenv;
use fake_torrent_client::Client;
use log::{self, error, info};
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};
use tracker::Event;

use crate::config::Config;

mod config;
mod routes;
mod scheduler;
mod torrent;
mod tracker;

static CONFIG: OnceLock<Config> = OnceLock::new();
static CLIENT: RwLock<Option<Client>> = RwLock::new(None); // TODO: remove, build it every time because it can be HTTP or UDP
static TORRENTS: RwLock<Vec<torrent::BasicTorrent>> = RwLock::new(Vec::new());

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let config = Config::load_config();
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

    prepare_torrent_directory(&config.torrent_dir);
    load_torrents(&config.torrent_dir);

    let tp = scheduled_thread_pool::ScheduledThreadPool::new(1);
    // schedule client refresh key if applicable
    let mut job_refresh_key: Option<scheduled_thread_pool::JobHandle> = None;
    if let Some(refresh_every) = init_client(&config) {
        job_refresh_key = Some(tp.execute_at_fixed_rate(
            std::time::Duration::from_secs(u64::try_from(refresh_every).unwrap()),
            std::time::Duration::from_secs(u64::try_from(refresh_every).unwrap()),
            move || {
                if let Some(client) = &mut *CLIENT.write().expect("Cannot read client") {
                    client.generate_key();
                }
            },
        ));
    }
    tracker::announce_start();
    tokio::spawn(async move { // graceful exit when Ctrl + C
        //let _addr = crate::scheduler::Scheduler.start(); // FIXME
        tokio::signal::ctrl_c().await.unwrap();
        if let Some(job) = job_refresh_key {
            job.cancel();
        }
        tracker::announce_stopped().await;
    });
    //start web server
    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(routes::get_config)
            .service(routes::get_torrents)
            .service(routes::receive_files)
            .service(routes::process_user_command)
            .service(Files::new(&config.web_root.clone(), "static/").index_file("index.html"))
    })
    .bind(config.server_addr.clone())?
    .workers(1)
    .system_exit()
    .run();
    info!("Starting HTTP server at http://{}/", &config.server_addr);
    server.await
}

fn prepare_torrent_directory(directory: &String) {
    if !std::path::Path::new(directory).is_dir() {
        std::fs::create_dir_all(directory).unwrap_or_else(|_e| {
            error!("Cannot create torrent folder directory(ies)");
        });
        info!("Torrent directory created: {}", directory);
    }
    info!("Will load torrents from: {}", directory);
}

fn load_torrents(directory: &String) {
    let paths = std::fs::read_dir(directory).expect("Cannot read torrent directory");
    let mut count = 0u16;
    for p in paths {
        let f = p
            .expect("Cannot get torrent path")
            .path()
            .into_os_string()
            .into_string()
            .expect("Cannot get file name");
        add_torrent(f);
        count += 1;
    }
    info!("{} torrent(s) loaded", count);
}

/// Add a torrent to the list. If the filename does not end with .torrent, the file is not processed
fn add_torrent(path: String) {
    if path.to_lowercase().ends_with(".torrent") {
        if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
            let config = CONFIG.get().expect("Cannot read configuration");
            let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
            info!("Loading torrent: \t{}", path);
            let t = torrent::from_file(path.clone());
            if let Ok(torrent) = t {
                let mut t = torrent::from_torrent(torrent, path);
                if config.min_download_rate > 0 && config.max_download_rate > 0 {
                    t.downloaded = 0;
                } else {
                    t.downloaded = t.length;
                }
                for existing in list.iter() {
                    if existing.info_hash == t.info_hash {
                        info!("Torrent is already in list");
                        return;
                    }
                }
                tracker::announce(&mut t, client.clone(), Some(Event::Started));
                list.push(t);
            }
        } else {
            error!("Cannot parse torrent: \t{}", path);
        }
    }
}

/// Init the client from the configuration and returns the interval to refresh client key if applicable
fn init_client(config: &Config) -> Option<u16> {
    let mut client = Client::default();
    client.build(
        fake_torrent_client::clients::ClientVersion::from_str(&config.client)
            .expect("Wrong client"),
    );
    info!(
        "Client information (key: {}, peer ID:{})",
        client.key, client.peer_id
    );
    let key_interval = client.key_refresh_every;
    let mut guard = CLIENT.write().unwrap();
    *guard = Some(client);
    key_interval
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
        prepare_torrent_directory(&dir.display().to_string());
        assert!(dir.is_dir());
        prepare_torrent_directory(&dir.display().to_string());
    }
}
