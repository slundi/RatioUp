#![allow(non_snake_case)]

extern crate rand;
extern crate clap;
extern crate lazy_static;
extern crate serde_bytes;

use clap::{Arg, value_t};
use serde_json::{json};
use std::{time::{Duration, Instant}};
use std::io::Write;
use actix::prelude::*;
use actix_multipart::Multipart;
use actix_web::{middleware::Logger, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use futures_util::{TryStreamExt as _};
use actix_web_actors::ws;
use actix_files::Files;
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;
use std::sync::RwLock;
use lazy_static::lazy_static;
use uuid::Uuid;
use rand::Rng;
use lava_torrent::torrent::v1::Torrent;

/// Delay between tracker announce in minutes (30*60 = 30 minutes)

mod algorithm;
mod config;
mod torrent;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

lazy_static! {
    static ref CONFIG: RwLock<config::Config> = RwLock::new(config::get_config("config.json"));
    static ref ACTIVE: RwLock<bool> = RwLock::new(true);
    static ref TORRENTS:RwLock<Vec<torrent::BasicTorrent>> = RwLock::new(Vec::new());
}

const EVENT_NONE: u8 = 0;
//const EVENT_COMPLETED: u8 = 1; //not used because we do not download for now
const EVENT_STARTED: u8 = 2;
const EVENT_STOPPED: u8 = 3;

/// A cron that check every minutes if it needs to announce, stop or start a torrent
pub struct Scheduler;
impl Actor for Scheduler {
    type Context = Context<Self>;
    fn started(&mut self, ctx: &mut Context<Self>) {
        self.announce(ctx, EVENT_STARTED);
        ctx.run_interval(Duration::from_secs(60), move |this, ctx| { this.announce(ctx, EVENT_NONE) });
        let c=&*CONFIG.read().expect("Cannot read configuration");
        if c.key_refresh_every > 0 {
            ctx.run_interval(Duration::from_secs((c.key_refresh_every as u64) * 60), move |this, ctx| { this.refresh_key(ctx) });
        }
    }
    fn stopped(&mut self, ctx: &mut Context<Self>) { self.announce(ctx, EVENT_STOPPED); }
}
impl Scheduler {
    fn announce(&self, _ctx: &mut Context<Self>, event: u8) {
        //TODO: 
        let c=&*CONFIG.read().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        for t in list {
            let should_announce = event == EVENT_STARTED || event == EVENT_STOPPED || t.announced >= 30;
            if !should_announce {t.announced += 1; continue;}
            let mut url: String = t.announce.clone().unwrap();
            url.push('?');
            url.push_str(&c.query);
            let uploaded = rand::thread_rng().gen_range(c.min_upload_rate..c.max_upload_rate) * 60 * (t.announced as u32);
            let url = url.replace("{peerid}", &c.peer_id).replace("{infohash}", &t.info_hash).replace("{key}", &c.key)
                    .replace("{uploaded}", uploaded.to_string().as_str())
                    .replace("{downloaded}", "0").replace("{left}", "0")
                    .replace("{event}", event.to_string().as_str()).replace("{numwant}", c.num_want.to_string().as_str()).replace("{port}", c.port.to_string().as_str());
            let mut client = reqwest::Client::new().get(&url);
            if c.user_agent != "" {client = client.header("user-agent", &c.user_agent);}
            if c.accept != "" {client = client.header("accept", &c.accept);}
            if c.accept_encoding != "" {client = client.header("accept-encoding", &c.accept_encoding);}
            if c.accept_language != "" {client = client.header("accept-language", &c.accept_language);}
            client.build().expect("Cannot build announce query");
            info!("Annonce at: {}", url);
            //client.send().await?;
            //"&&port={port}&uploaded={uploaded}&&left={left}&corrupt=0&key={key}&event={event}&&compact=1&no_peer_id=1",
            //Responce is like: type AnnounceResponse struct { interval   int // Interval in seconds a client should wait |.| messages, trackerID  string, complete   uint, incomplete uint
            t.announced = 0;
        }
    }

    fn refresh_key(&self, _ctx: &mut Context<Self>) {
        info!("Refreshing key");
        let c = &mut *CONFIG.write().expect("Cannot read configuration");
        c.generate_key();
    }
}

/// do websocket handshake and start `RatioUpWS` actor
async fn ws_index(r: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> { ws::start(RatioUpWS::new(), &r, stream) }

async fn receive_files(mut payload: Multipart) -> Result<HttpResponse, Error> {
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
    //TODO: send new torrent list to the client
    //let list = &*TORRENTS.read().expect("Cannot get torrent list");
    //ctx.text(format!("{{\"torrents\":{}}}", json!(list)));
    Ok(HttpResponse::Ok().into())
}

/// websocket connection is long running connection, it easier to handle with an actor
struct RatioUpWS {
    /// Client must send ping at least once per 30 seconds (CLIENT_TIMEOUT), otherwise we drop connection.
    hb: Instant,
}
impl Actor for RatioUpWS {
    type Context = ws::WebsocketContext<Self>;
    /// Method is called on actor start, it means a web browser just loaded the page. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
        //read the configuration
        let c=&*CONFIG.read().expect("Cannot read configuration");
        ctx.text(format!("{{\"config\":{}}}", json!(c)));
        //load torrents
        let list = &*TORRENTS.read().expect("Cannot get torrent list");
        ctx.text(format!("{{\"torrents\":{}}}", json!(list)));
    }
}

// Handler for `ws::Message`
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for RatioUpWS {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context,) {
        match msg {
            Ok(ws::Message::Ping(msg)) => { self.hb = Instant::now(); ctx.pong(&msg); }
            Ok(ws::Message::Pong(_)) => {self.hb = Instant::now();}
            Ok(ws::Message::Text(text)) => {
                info!("Receiving text: {:?}", text);
                if text == "toggle_start" { //enable or disable seeding, you should stop the app instead
                    let mut w = ACTIVE.write().expect("Cannot change application state");
                    *w = !*w;
                    if *w { ctx.text("{\"running\": true}"); info!("Seedding resumed"); }
                    else  { ctx.text("{\"running\": false}");info!("Seedding stopped"); }
                } else if text.starts_with("{\"switch\":\"") { //enable disable torrent
                    let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
                    let v: serde_json::Value = serde_json::from_str(&text).expect("Cannot parse switch message");
                    let h = v["switch"].as_str().expect("Switch message does not contain a hash");
                    for t in list {
                        if t.info_hash == h {
                            t.active = !t.active;
                            if t.active {ctx.text(format!("{{\"active\":\"{}\"}}", h));}
                            else {ctx.text(format!("{{\"disabled\":\"{}\"}}", h));}
                            break;
                        }
                    }
                } else if text.starts_with("{\"remove\":\"") { //remove a torrent
                    let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
                    let v: serde_json::Value = serde_json::from_str(&text).expect("Cannot parse remove message");
                    let h = v["remove"].as_str().expect("Remove message does not contain a hash");
                    for i in 0..list.len() {
                        if list[i].info_hash == h {
                            let r = std::fs::remove_file(&list[i].path);
                            if r.is_ok() {
                                list.remove(i);
                                ctx.text(format!("{{\"removed\":\"{}\"}}", h));
                            } else {ctx.text(format!("{{\"error\":\"Cannot remove torrent file\"}}"))}
                            break;
                        }
                    }
                }
            }
            Ok(ws::Message::Binary(bin)) => {
                info!("Receiving binary, size={}", bin.len());
                let mut pos = 0;
                let mut buffer = std::fs::File::create("./torrents/foo.torrent").unwrap();  // notice the name of the file that will be written
                //let mut buffer = std::fs::OpenOptions::new().append(true).create(true).open("foo.torrent").unwrap();
                while pos < bin.len() {
                    let bytes_written = buffer.write(&bin[pos..]).unwrap();
                    pos += bytes_written
                };
                //ctx.binary(bin)},
                ctx.text("true");
            }
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}

impl RatioUpWS {
    fn new() -> Self {Self { hb: Instant::now(), }}

    /// helper method that sends ping to client every second also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        info!("Web server started");
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT { // check client heartbeats
                info!("Websocket Client heartbeat failed, disconnecting!"); // heartbeat timed out
                ctx.stop(); // stop actor
                return; // don't try to send a ping
            }
            ctx.ping(b"");
        });
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let subscriber = FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.) will be written to stdout.
        .with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
    //for c in clients.in {client_list.push(c.0);}
    //let path = std::env::current_dir()?; println!("The current directory is {}", path.display());
    //parse command line
    let matches = clap::App::new("RatioUp")
                          .arg(Arg::with_name("WEB_ROOT")
                               .long("root")
                               .help("Set a custom web root (ex: / or /ratio-up/").default_value("/").takes_value(true))
                          .arg(Arg::with_name("PORT")
                               .short("p").long("port")
                               .help("Sets HTTP web port").default_value("7070").takes_value(true))
                          .arg(Arg::with_name("CONFIG")
                               .short("c").long("config")
                               .help("Path to the config file. It'll be generated if it does not exists").default_value("config.json").takes_value(true))
                          .arg(Arg::with_name("DIRECTORY")
                               .short("d").long("dir")
                               .help("Directory where torrents are saved").default_value("./torrents").takes_value(true))
                          .get_matches();
    let port = value_t!(matches, "PORT", u16).unwrap_or_else(|e| {error!("Server port is not defined"); e.exit()});
    let root=value_t!(matches, "WEB_ROOT", String).unwrap_or_else(|e| {error!("Web root is not defined"); e.exit()});
    let config = value_t!(matches, "CONFIG", String).unwrap_or_else(|e| {error!("Config file is not defined"); e.exit()});
    let directory = value_t!(matches, "DIRECTORY", String).unwrap_or_else(|e| {error!("Config file is not defined"); e.exit()});
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
        .wrap(Logger::default())
        .service(web::resource("/ws/").route(web::get().to(ws_index)))
        .service(web::resource("/add_torrents").route(web::post().to(receive_files)))
        .service(Files::new(&root, "static/").index_file("index.html"))})
        .bind(format!("127.0.0.1:{}",port))?.system_exit().run().await
}

/// Add a torrent to the list. If the filename does not end with .torrent, the file is not processed
fn add_torrent(path: String) {
    if path.to_lowercase().ends_with(".torrent") {
        let c=&*CONFIG.read().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        info!("Loading torrent: \t{}", path);
        let t = Torrent::read_from_file(&path);
        if t.is_ok() {
            let mut t = torrent::BasicTorrent::from_torrent(t.unwrap(), path);
            //enable seeding on public torrents depending on the config value of seed_public_torrent
            if c.seed_public_torrent && !t.private {t.active = true;}
            else {t.active = false;}
            for bt in list.clone() { if bt.info_hash == t.info_hash {
                info!("Torrent is already in list");
                return;
            }}
            list.push(t);
        } else {error!("Cannot parse torrent: \t{}", path);}
    }
}
