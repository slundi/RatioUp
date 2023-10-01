#![allow(non_snake_case)]

#[macro_use]
extern crate serde_derive;
extern crate rand;

use actix::prelude::*;
use actix_files::Files;
use actix_web::{middleware, App, HttpServer};
use byte_unit::Byte;
use dotenv::dotenv;
use fake_torrent_client::Client;
use log::{self, debug, error, info};
use rand::Rng;
use std::convert::TryFrom;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::{OnceLock, RwLock};
use std::time::Duration;

use crate::config::Config;

mod config;
mod routes;
mod torrent;

static CONFIG: OnceLock<Config> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(true);
static CLIENT: RwLock<Option<Client>> = RwLock::new(None);
static TORRENTS: RwLock<Vec<torrent::BasicTorrent>> = RwLock::new(Vec::new());

/// A cron that check every minutes if it needs to announce, stop or start a torrent
pub struct Scheduler;
impl Actor for Scheduler {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Context<Self>) {
        debug!("Scheduler started");
        self.announce(ctx, torrent::EVENT_STARTED);
        if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
            if let Some(refresh_every) = client.key_refresh_every {
                ctx.run_interval(
                    Duration::from_secs(u64::try_from(refresh_every).unwrap() * 60),
                    move |this, ctx| this.refresh_key(ctx),
                );
            }
        }
    }
    fn stopped(&mut self, ctx: &mut Context<Self>) {
        self.announce(ctx, torrent::EVENT_STOPPED);
    }
}
impl Scheduler {
    /// Build the announce query and perform it in another thread
    fn announce(&self, ctx: &mut Context<Self>, event: &str) {
        debug!("Announcing");
        if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
            let config = CONFIG.get().expect("Cannot read configuration");
            let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
            let mut available_download_speed: u32 = config.max_download_rate;
            let mut available_upload_speed: u32 = config.max_upload_rate;
            // send queries to trackers
            for t in list {
                let mut process = false;
                let mut interval: u64 = torrent::TORRENT_INFO_INTERVAL;
                if !t.last_announce.elapsed().as_secs() <= t.interval || event == torrent::EVENT_STARTED || event == torrent::EVENT_STOPPED {
                    let url = &t.build_urls(event, client.key.clone())[0];
                    let query = client.get_query();
                    let agent = ureq::AgentBuilder::new()
                        .timeout(std::time::Duration::from_secs(60))
                        .user_agent(&client.user_agent);
                    let mut req = agent
                        .build()
                        .get(url)
                        .timeout(std::time::Duration::from_secs(90));
                    req = query
                        .1
                        .into_iter()
                        .fold(req, |req, header| req.set(&header.0, &header.1));
                    interval = t.announce(event, req);
                    process = true;
                    info!("Anounced: interval={}, event={}, downloaded={}, uploaded={}, seeders={}, leechers={}, torrent={}", t.interval, event, t.downloaded, t.uploaded, t.seeders, t.leechers, t.name);
                }
                //compute the download and upload speed
                if available_upload_speed > 0 && t.leechers > 0 && t.seeders > 0 {
                    if process {
                        t.next_upload_speed = rand::thread_rng()
                            .gen_range(config.min_upload_rate..available_upload_speed);
                    }
                    available_upload_speed -= t.next_upload_speed;
                }
                if available_download_speed > 0 && t.leechers > 0 && t.seeders > 0 {
                    if process {
                        t.next_download_speed = rand::thread_rng()
                            .gen_range(config.min_download_rate..available_download_speed);
                    }
                    available_download_speed -= t.next_download_speed;
                }
                if !process {
                    continue;
                }
                t.uploaded += (interval as usize) * (t.next_upload_speed as usize);
                if t.length < t.downloaded + (t.next_download_speed as usize * interval as usize) {
                    //compute next interval to for an EVENT_COMPLETED
                    let t: u64 =
                        (t.length - t.downloaded).div_euclid(t.next_download_speed as usize) as u64;
                    ctx.run_later(Duration::from_secs(t + 5), move |this, ctx| {
                        this.announce(ctx, torrent::EVENT_COMPLETED);
                    });
                } else {
                    ctx.run_later(Duration::from_secs(interval), move |this, ctx| {
                        this.announce(ctx, torrent::EVENT_NONE);
                    });
                }
            }
        }
    }

    fn refresh_key(&self, _ctx: &mut Context<Self>) {
        info!("Refreshing key");
        if let Some(client) = &mut *CLIENT.write().expect("Cannot read client") {
            client.generate_key();
        }
    }
}

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

    info!("Torrent client: {}", config.client);
    init_client(&config);
    info!(
        "Bandwidth: \u{2191} {} - {} \t \u{2193} {} - {}",
        Byte::from_bytes(u128::try_from(config.min_upload_rate).unwrap())
            .get_appropriate_unit(true)
            .to_string(),
        Byte::from_bytes(u128::try_from(config.max_upload_rate).unwrap())
            .get_appropriate_unit(true)
            .to_string(),
        Byte::from_bytes(u128::try_from(config.min_download_rate).unwrap())
            .get_appropriate_unit(true)
            .to_string(),
        Byte::from_bytes(u128::try_from(config.max_download_rate).unwrap())
            .get_appropriate_unit(true)
            .to_string(),
    );

    if !std::path::Path::new(&config.torrent_dir).is_dir() {
        std::fs::create_dir_all(&config.torrent_dir).unwrap_or_else(|_e| {
            error!("Cannot create torrent folder directory(ies)");
        });
        info!("Torrent directory created: {}", config.torrent_dir);
    }
    //create torrent folder
    let torrent_folder = std::path::Path::new(&config.torrent_dir);
    std::fs::create_dir_all(torrent_folder).expect("Cannot create torrent folder");
    //load torrents
    let paths = std::fs::read_dir(&config.torrent_dir).expect("Cannot read torrent directory");
    for p in paths {
        let f = p
            .expect("Cannot get torrent path")
            .path()
            .into_os_string()
            .into_string()
            .expect("Cannot get file name");
        add_torrent(f);
    }
    
    Scheduler.start();
    //start web server
    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(routes::toggle_active)
            .service(routes::get_config)
            .service(routes::get_torrents)
            .service(routes::receive_files)
            .service(routes::process_user_command)
            .service(Files::new(&config.web_root.clone(), "static/").index_file("index.html"))
    })
    .bind(config.server_addr.clone())?
    .workers(2)
    .system_exit()
    .run();
    info!("Starting HTTP server at http://{}/", &config.server_addr);
    server.await
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
                t.prepare_urls(
                    client.query.clone(),
                    config.port,
                    client.peer_id.clone(),
                    client.num_want,
                ); //build the static part of the annouce query
                   //download torrent if download speeds are set
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
                list.push(t);
            }
        } else {
            error!("Cannot parse torrent: \t{}", path);
        }
    }
}

fn init_client(config: &Config) {
    let mut client = Client::default();
    client.build(
        fake_torrent_client::clients::ClientVersion::from_str(&config.client)
            .expect("Wrong client"),
    );
    info!("Client information (key: {}, peer ID:{})", client.key, client.peer_id);
    let mut guard = CLIENT.write().unwrap();
    *guard = Some(client);
}
