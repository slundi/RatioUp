#![allow(non_snake_case)]

#[macro_use]
extern crate serde_derive;
extern crate lazy_static;
extern crate rand;

use actix::prelude::*;
use actix_files::Files;
use actix_web::{middleware, App, HttpServer};
use byte_unit::Byte;
use dotenv::dotenv;
use fake_torrent_client::Client;
use lazy_static::lazy_static;
use log::{self, error, info};
use rand::Rng;
use std::sync::RwLock;
use std::time::Duration;

mod routes;
mod torrent;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    /// Server `<IP or hostaname>:<port>`. Default is `127.0.0.1:8070`
    #[serde(skip_serializing)] pub server_addr: String,
    /// Log level (available options are: INFO, WARN, ERROR, DEBUG, TRACE). Default is `INFO`.
    #[serde(skip_serializing)] pub log_level: String,
    /// torrent port
    #[serde(skip_serializing)] pub port: u16,
    /// HTTP web port
    #[serde(skip_serializing)] pub http_port: u16,
    pub min_upload_rate: u32,   //in byte
    pub max_upload_rate: u32,   //in byte
    pub min_download_rate: u32, //in byte
    pub max_download_rate: u32, //in byte
    //pub simultaneous_seed: u16, //useful ?
    pub client: String,
    /// Directory where torrents are saved
    #[serde(skip_serializing)] pub torrent_dir: String,
    /// Set a custom web root (ex: / or /ratio-up/)
    #[serde(skip_serializing)] pub web_root: String,
    #[serde(skip_serializing)] pub key_refresh_every: u16,
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
            http_port: 8070,
            torrent_dir: String::from("./torrents"),
            web_root: String::from("/"),
            //client: fake_torrent_client::Client::from(fake_torrent_client::clients::ClientVersion::Qbittorrent_4_4_2),
            key_refresh_every: 0,
            client: String::from("INVALID"),
        }
    }
}

// Use Jemalloc only for musl-64 bits platforms (https://kerkour.com/rust-small-docker-image)
#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

lazy_static! {
    static ref CONFIG: RwLock<Config> = RwLock::new(Config::default());
    static ref CLIENT: RwLock<Client> = RwLock::new(Client::default());
    static ref ACTIVE: RwLock<bool> = RwLock::new(true);
    static ref TORRENTS: RwLock<Vec<torrent::BasicTorrent>> = RwLock::new(Vec::new());
}

/// A cron that check every minutes if it needs to announce, stop or start a torrent
pub struct Scheduler;
impl Actor for Scheduler {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Context<Self>) {
        let client = &*CLIENT.read().expect("Cannot read client");
        self.announce(ctx, torrent::EVENT_STARTED);
        if client.key_refresh_every > 0 {
            ctx.run_interval(
                Duration::from_secs((client.key_refresh_every as u64) * 60),
                move |this, ctx| this.refresh_key(ctx),
            );
        }
    }
    fn stopped(&mut self, ctx: &mut Context<Self>) {
        self.announce(ctx, torrent::EVENT_STOPPED);
    }
}
impl Scheduler {
    /// Build the announce query and perform it in another thread
    fn announce(&self, ctx: &mut Context<Self>, event: &str) {
        let client = &*CLIENT.read().expect("Cannot read client");
        let config = &*CONFIG.read().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        let mut available_download_speed: u32 = config.max_download_rate;
        let mut available_upload_speed: u32 = config.max_upload_rate;
        // send queries to trackers
        for t in list {
            let mut process = false;
            let mut interval: u64 = torrent::TORRENT_INFO_INTERVAL;
            if !t.last_announce.elapsed().as_secs() <= t.interval {
                let url = &t.build_urls(event, client.key.clone())[0];
                let req = client.get_http_request(url);
                interval = t.announce(event, req);
                process = true;
            }
            //compute the download and upload speed
            if available_upload_speed > 0 && t.leechers > 0 && t.seeders > 0 {
                if process {
                    t.next_upload_speed =
                        rand::thread_rng().gen_range(config.min_upload_rate..available_upload_speed);
                }
                available_upload_speed -= t.next_upload_speed;
            }
            if available_download_speed > 0 && t.leechers > 0 && t.seeders > 0 {
                if process {
                    t.next_download_speed =
                        rand::thread_rng().gen_range(config.min_download_rate..available_download_speed);
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
                ctx.run_later(Duration::from_secs(interval as u64), move |this, ctx| {
                    this.announce(ctx, torrent::EVENT_NONE);
                });
            }
        }
    }

    fn refresh_key(&self, _ctx: &mut Context<Self>) {
        info!("Refreshing key");
        let client = &mut *CLIENT.write().expect("Cannot read client");
        client.generate_key();
    }
}

/*  /// Function to send periodically torrent informations: up/download speeds, seeders, leechers, butes completed, ...
    fn create_job_send_info_at_interval(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(TORRENT_INFO_INTERVAL, |act, ctx| {
            let list = &*TORRENTS.read().expect("Cannot get torrent list");
            let mut msg = String::from("{\"infos\":[");
            for i in 0..list.len() {
                msg.push_str("{\"info_hash\":\"");
                msg.push_str(&list[i].info_hash);
                msg.push_str("\",\"downloaded\":");
                msg.push_str(list[i].downloaded.to_string().as_str());
                msg.push_str(",\"seeders\":");
                msg.push_str(list[i].seeders.to_string().as_str());
                msg.push_str(",\"leechers\":");
                msg.push_str(list[i].leechers.to_string().as_str());
                msg.push_str(",\"download_speed\":");
                msg.push_str(list[i].next_download_speed.to_string().as_str());
                msg.push_str(",\"upload_speed\":");
                msg.push_str(list[i].next_upload_speed.to_string().as_str());
                if i < list.len() - 1 {msg.push_str("},");}else{msg.push_str("}]}");}
            }
            ctx.text(msg);
        });
    }
}*/

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenv().ok();
    let config_ = config::Config::builder()
        .add_source(::config::Environment::default())
        .build()
        .expect("Cannot build config");
    let config: Config = config_.try_deserialize().expect("Cannot get config");

    //configure logger
    simple_logger::init_with_level(match &config.log_level as &str {
        "WARN" => log::Level::Warn,
        "ERROR" => log::Level::Error,
        "DEBUG" => log::Level::Debug,
        "TRACE" => log::Level::Trace,
        _ => log::Level::Info,
    })
    .unwrap();

    // info!("Client: {}", config.client);
    info!(
        "Bandwidth: {} - {}",
        Byte::from_bytes(config.min_upload_rate as u128)
            .get_appropriate_unit(true)
            .to_string(),
        Byte::from_bytes(config.max_upload_rate as u128)
            .get_appropriate_unit(true)
            .to_string()
    );

    if !std::path::Path::new(&config.torrent_dir).is_dir() {
        std::fs::create_dir_all(&config.torrent_dir).unwrap_or_else(|_e| {
            error!("Cannot create torrent folder directory(ies)");
        });
        info!("Torrent directory created: {}", config.torrent_dir);
    }
    //create torrent folder
    let torrent_folder = std::path::Path::new("torrents");
    std::fs::create_dir_all(torrent_folder).expect("Cannot create torrent folder");
    //load torrents
    let paths = std::fs::read_dir("./torrents/").expect("Cannot read torrent directory");
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
            .service(Files::new(&config.web_root, "static/").index_file("index.html"))
    })
    .bind(format!("127.0.0.1:{}", config.port))?
    .system_exit()
    .run();
    info!("starting HTTP server at http://{}/", &config.server_addr);
    server.await
}

/// Add a torrent to the list. If the filename does not end with .torrent, the file is not processed
fn add_torrent(path: String) {
    if path.to_lowercase().ends_with(".torrent") {
        let client = &*CLIENT.read().expect("Cannot read client");
        let config = &*CONFIG.read().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        info!("Loading torrent: \t{}", path);
        let t = torrent::from_file(path.clone());
        //let t = Torrent::read_from_file(&path);
        if t.is_ok() {
            let mut t = torrent::from_torrent(t.unwrap(), path);
            t.prepare_urls(client.query.clone(), config.port, client.peer_id.clone(), client.num_want); //build the static part of the annouce query
                                                                                    //download torrent if download speeds are set
            if config.min_download_rate > 0 && config.max_download_rate > 0 {
                t.downloaded = 0;
            } else {
                t.downloaded = t.length;
            }
            for i in 0..list.len() {
                if list[i].info_hash == t.info_hash {
                    info!("Torrent is already in list");
                    return;
                }
            }
            list.push(t);
        } else {
            error!("Cannot parse torrent: \t{}", path);
        }
    }
}
