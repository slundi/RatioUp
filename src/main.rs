#![allow(non_snake_case)]

#[macro_use] extern crate serde_derive;
extern crate rand;
extern crate lazy_static;

use clap::Parser;
use serde_json::{json};
use std::{time::Duration};
use std::io::{Read, Write};
use actix::prelude::*;
use actix_multipart::Multipart;
use actix_web::{
    get, post,
    http::{
        header::{self, ContentType},
        Method, StatusCode,
    },
    middleware, web, App, HttpResponse, HttpServer, Result,
};
use futures_util::{TryStreamExt as _};
use actix_files::Files;
use tracing::{info, warn, error, Level};
use tracing_subscriber::FmtSubscriber;
use std::sync::RwLock;
use lazy_static::lazy_static;
use uuid::Uuid;
use rand::Rng;
use regex::Regex;

mod algorithm;
mod config;
mod torrent;

const TORRENT_INFO_INTERVAL: Duration = Duration::from_secs(120);
//const DEFAULT_ANNOUNCE_INTERVAL: Duration = Duration::from_secs(1800); //1800s = 30min

lazy_static! {
    static ref CONFIG: RwLock<config::Config> = RwLock::new(config::get_config("config.json"));
    static ref ACTIVE: RwLock<bool> = RwLock::new(true);
    static ref TORRENTS:RwLock<Vec<torrent::BasicTorrent>> = RwLock::new(Vec::new());
}

/// A cron that check every minutes if it needs to announce, stop or start a torrent
pub struct Scheduler;
impl Actor for Scheduler {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Context<Self>) {
        let c=&*CONFIG.read().expect("Cannot read configuration");
        self.announce(ctx, torrent::EVENT_STARTED);
        if c.key_refresh_every > 0 {
            ctx.run_interval(Duration::from_secs((c.key_refresh_every as u64) * 60), move |this, ctx| { this.refresh_key(ctx) });
        }
    }
    fn stopped(&mut self, ctx: &mut Context<Self>) { self.announce(ctx, torrent::EVENT_STOPPED); }
}
impl Scheduler {
    /// Build the announce query and perform it in another thread
    fn announce(&self, ctx: &mut Context<Self>, event: &str) {
        let c=&*CONFIG.read().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        for t in list {
            //if !t.active {continue;}
            let url = &t.build_urls(c.query.clone(), event, c.peer_id.clone(), c.key.clone(), c.port, c.num_want)[0];
            let req = c.get_http_request(&url);
            match req.call() {
                Ok(resp) => {
                    let code = resp.status();
                    let mut bytes: Vec<u8> = Vec::with_capacity(2048);
                    resp.into_reader().take(1024).read_to_end(&mut bytes).expect("Cannot read response");
                    //we start to check if the tracker has returned an error message, if yes, we will reannounce later
                    let response = serde_bencode::de::from_bytes::<torrent::FailureTrackerResponse>(&bytes.clone());
                    if response.is_ok() {
                        warn!("Announce error from the tracker: {}", response.unwrap().reason);
                        ctx.run_later(Duration::from_secs(1800), move |this, ctx| { this.announce(ctx, torrent::EVENT_NONE); });
                        continue;
                    }
                    let rawdata = String::from_utf8_lossy(&bytes);
                    info!("RESPONSE: {:?}", rawdata);
                    //dirty map with regex, because binary on response prevent the parsing
                    lazy_static! {
                        static ref RE_COMPLETE:   Regex = Regex::new("8:completei(\\d+)e").unwrap();
                        static ref RE_INCOMPLETE: Regex = Regex::new("10:incompletei(\\d+)e").unwrap();
                        static ref RE_INTERVAL:   Regex = Regex::new("8:intervali(\\d+)e").unwrap();
                        //static ref RE_MIN_INTERVAL:   Regex = Regex::new("12:min intervali(\\d+)e").unwrap();
                    }
                    let x = RE_COMPLETE.captures(&rawdata);
                    t.seeders = if x.is_some() {x.unwrap().get(1).unwrap().as_str().parse().unwrap()} else {0};
                    let x = RE_INCOMPLETE.captures(&rawdata);
                    t.leechers = if x.is_some() {x.unwrap().get(1).unwrap().as_str().parse().unwrap()} else {0};
                    let x = RE_INTERVAL.captures(&rawdata);
                    let interval: u64 = if x.is_some() {x.unwrap().get(1).unwrap().as_str().parse().unwrap()} else {120};
                    t.next_upload_speed   = rand::thread_rng().gen_range(c.min_upload_rate..c.max_upload_rate);
                    info!("\tSeeders: {}\tLeechers: {}\t\t\tInterval: {:?}s", t.seeders, t.leechers, interval);
                    //info!("\tResponse: {}/{}\t{}   {:?}", bytes.len(), 1024, code, response);
                    if c.min_download_rate>0 && c.max_download_rate>0 {t.next_download_speed = rand::thread_rng().gen_range(c.min_download_rate..c.max_download_rate);}
                    if t.length < t.downloaded + (t.next_download_speed as usize * interval as usize) { //compute next interval to for an EVENT_COMPLETED
                        let t: u64 = (t.length - t.downloaded).div_euclid(t.next_download_speed as usize) as u64;
                        ctx.run_later(Duration::from_secs(t + 5), move |this, ctx| { this.announce(ctx, torrent::EVENT_NONE); });
                    } else {ctx.run_later(Duration::from_secs(interval as u64), move |this, ctx| { this.announce(ctx, torrent::EVENT_NONE); });}
                    t.last_announce = std::time::Instant::now();
                }
                Err(ureq::Error::Status(code, response)) => {warn!("Unexpected server response status: {}\t{:?}", code, response); } //the server returned an unexpected status code (such as 400, 500 etc)
                Err(_) => {if event != torrent::EVENT_STOPPED {error!("I/O error while announcing");}}
            }
        }
    }

    fn refresh_key(&self, _ctx: &mut Context<Self>) {
        info!("Refreshing key");
        let c = &mut *CONFIG.write().expect("Cannot read configuration");
        c.generate_key();
    }
}

#[post("/add_torrents")]
async fn receive_files(mut payload: Multipart) -> Result<HttpResponse> {
    while let Some(mut field) = payload.try_next().await? { // iterate over multipart stream
        let content_disposition = field.content_disposition(); // A multipart/form-data stream has to contain `content_disposition`
        let filename = content_disposition
            .get_filename()
            .map_or_else(|| Uuid::new_v4().to_string(), sanitize_filename::sanitize);
        let filepath = format!("./torrents/{}", filename);
        let filepath2 = filepath.clone();
        let mut f = web::block(|| std::fs::File::create(filepath)).await??; // File::create is blocking operation, use threadpool
        while let Some(chunk) = field.try_next().await? { // Field in turn is stream of *Bytes* object
            // filesystem operations are blocking, we have to use threadpool
            f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
        }
        add_torrent(filepath2);
    }
    let list = &*TORRENTS.read().expect("Cannot get torrent list");
    Ok(HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body(format!("{{\"torrents\":{}}}", json!(list))))
}

/// Returns the configuration as a JSON string
#[get("/config")]
async fn get_config() -> Result<HttpResponse> {
    let c=&*CONFIG.read().expect("Cannot read configuration");
    Ok(HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body(format!("{{\"config\":{}}}", json!(c))))
}

/// Returns the torrent list as a JSON string
#[get("/torrents")]
async fn get_torrents() -> Result<HttpResponse> {
    let list = &*TORRENTS.read().expect("Cannot get torrent list");
    Ok(HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body(format!("{{\"torrents\":{}}}", json!(list))))
}

/// Stort or stop the seeding depending on the current state, you should stop the app instead
#[get("/toggle")]
async fn toggle_active() -> Result<HttpResponse> {
    let mut w = ACTIVE.write().expect("Cannot change application state");
    *w = !*w;
    if *w { info!("Seedding resumed"); return Ok(HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body("true"));}
    else  { info!("Seedding stopped"); return Ok(HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body("false"));}
}

#[derive(Serialize, Deserialize, Clone)]
struct CommandParams {
    command: String,
    infohash: String,
}
#[post("/command")]
async fn process_user_command(params: web::Form<CommandParams>) -> HttpResponse {
    info!("Processing user command: {}", params.command);
    if params.command.to_lowercase() == "switch" && params.infohash != "" { //enable disable torrent
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        for t in list {
            if t.info_hash == params.infohash {
                t.active = !t.active;
                if t.active {return HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body(format!("{{\"active\":\"{}\"}}", params.infohash));}
                else        {return HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body(format!("{{\"disabled\":\"{}\"}}", params.infohash));}
            }
        }
    } else if params.command.to_lowercase() == "remove" && params.infohash != "" { //enable disable torrent
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        for i in 0..list.len() {
            if list[i].info_hash == params.infohash {
                let r = std::fs::remove_file(&list[i].path);
                if r.is_ok() {
                    list.remove(i);
                    return HttpResponse::build(StatusCode::OK).content_type(ContentType::json()).body(format!("{{\"removed\":\"{}\"}}", params.infohash));
                } else {return HttpResponse::build(StatusCode::INTERNAL_SERVER_ERROR).body("Cannot remove torrent file");}
            }
        }
    }
    HttpResponse::build(StatusCode::BAD_REQUEST).body("")
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

#[derive(Parser, Debug, Clone)]
#[clap(author="Sébastien L", version="1.0", about="A tool to cheat on your various tracker ratios", long_about = None)]
struct Args {
    /// Path to the config file. It'll be generated if it does not exists
    #[clap(short='c', long, default_value="config.json", help="Path to the config file. It'll be generated if it does not exists")] config: String,
    /// Directory where torrents are saved
    #[clap(short='d', long="dir", default_value="./torrents", help="Directory where torrents are saved")] directory: String,
    /// Sets HTTP web port
    #[clap(short='p', long, default_value="8070", help="Sets HTTP web port")] port: u16,
    /// Set a custom web root (ex: / or /ratio-up/
    #[clap(long="root", default_value="/", help="Set a custom web root (ex: / or /ratio-up/")] web_root: String,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let subscriber = FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.) will be written to stdout.
        .with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    //parse command line
    let args: Args = Args::parse();
    let config = args.config;
    let directory = args.directory;
    let web_root = args.web_root;
    let port = args.port;
    if !std::path::Path::new(&config).is_file() {config::write_default(config);}
    if !std::path::Path::new(&directory).is_dir() {
        std::fs::create_dir_all(&directory).unwrap_or_else(|_e| {error!("Cannot create torrent folder directory(ies)");});
        info!("Torrent directory created: {}", directory);
    }
    //create torrent folder
    let torrent_folder = std::path::Path::new("torrents");
    std::fs::create_dir_all(torrent_folder).expect("Cannot create torrent folder");
    //load torrents
    let paths = std::fs::read_dir("./torrents/").expect("Cannot read torrent directory");
    for p in paths {
        let f = p.expect("Cannot get torrent path").path().into_os_string().into_string().expect("Cannot get file name");
        add_torrent(f);
    }
    Scheduler.start();
    //start web server
    HttpServer::new(move || {App::new()
        .wrap(middleware::Logger::default())
        .service(toggle_active).service(get_config).service(get_torrents).service(receive_files).service(process_user_command)
        .service(Files::new(&web_root, "static/").index_file("index.html"))})
        .bind(format!("127.0.0.1:{}", port))?.system_exit().run().await
}

/// Add a torrent to the list. If the filename does not end with .torrent, the file is not processed
fn add_torrent(path: String) {
    if path.to_lowercase().ends_with(".torrent") {
        let c=&*CONFIG.read().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        info!("Loading torrent: \t{}", path);
        let t=torrent::from_file(path.clone());
        //let t = Torrent::read_from_file(&path);
        if t.is_ok() {
            let mut t = torrent::from_torrent(t.unwrap(), path);
            //enable seeding on public torrents depending on the config value of seed_public_torrent
            if c.seed_public_torrent && !t.private {t.active = true;}
            else {t.active = false;}
            //download torrent if download speeds are set
            if c.min_download_rate > 0 && c.max_download_rate > 0 {t.downloaded = 0;} else {t.downloaded = t.length;}
            for bt in list.clone() { if bt.info_hash == t.info_hash {
                info!("Torrent is already in list");
                return;
            }}
            list.push(t);
        } else {error!("Cannot parse torrent: \t{}", path);}
    }
}
