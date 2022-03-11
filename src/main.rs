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
use futures_util::TryStreamExt as _;
use actix_web_actors::ws;
use actix_files::Files;
use env_logger;
use std::sync::RwLock;
use lazy_static::lazy_static;
use uuid::Uuid;
use lava_torrent::torrent::v1::Torrent;

//mod client;
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

/// do websocket handshake and start `RatioUpWS` actor
async fn ws_index(r: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    let res = ws::start(RatioUpWS::new(), &r, stream);
    res
}

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
        // process websocket messages
        //println!("Receiving... {:?}", msg);
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {self.hb = Instant::now();}
            Ok(ws::Message::Text(text)) => {
                println!("Receiving text: {:?}", text);
                if text.starts_with("upload_start:") {}
                else if text == "upload_end" {}
                else if text == "toggle_start" { //enable or disable seeding, you should stop the app instead
                    let mut w = ACTIVE.write().expect("Cannot change application state");
                    *w = !*w;
                    if *w {
                        ctx.text("{\"running\": true}");
                        log::info!("Seedding stopped");
                    } else {
                        ctx.text("{\"running\": false}");
                        log::info!("Seedding rusumed");
                    }
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
                println!("Receiving binary, size={}", bin.len());
                let mut pos = 0;
                //let mut buffer = std::fs::File::create("foo.torrent").unwrap();  // notice the name of the file that will be written
                let mut buffer = std::fs::OpenOptions::new().append(true).create(true).open("foo.torrent").unwrap();
                while pos < bin.len() {
                    let bytes_written = std::io::Write::write(&mut buffer, &bin[pos..]).unwrap();
                    pos += bytes_written
                };
                //ctx.binary(bin)},
                ctx.text("true");
            },
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            },
            _ => ctx.stop(),
        }
    }
}

impl RatioUpWS {
    fn new() -> Self {Self {
        hb: Instant::now(),
    }}

    /// helper method that sends ping to client every second also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        log::info!("Web server started");
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT { // check client heartbeats
                println!("Websocket Client heartbeat failed, disconnecting!"); // heartbeat timed out
                ctx.stop(); // stop actor
                return; // don't try to send a ping
            }
            ctx.ping(b"");
        });
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));
    log::info!("Starting");
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
                          .get_matches();
    let port = value_t!(matches, "PORT", u16).unwrap_or_else(|e| e.exit());
    let root=value_t!(matches, "WEB_ROOT", String).unwrap_or_else(|e| e.exit());
    //create torrent folder
    let torrent_folder = std::path::Path::new("torrents");
    std::fs::create_dir_all(torrent_folder).expect("Cannot create torrent folder");
    //load torrents
    { //block to release the thread lock
        let paths = std::fs::read_dir("./torrents/").expect("Cannot read torrent directory");
        for p in paths {
            let f = p.expect("Cannot get torrent path").path().into_os_string().into_string().expect("Cannot get file name");
            add_torrent(f);
        }
    }
    //start web server
    HttpServer::new(move || {App::new()
        .wrap(Logger::default())
        .service(web::resource("/ws/").route(web::get().to(ws_index)))
        .service(web::resource("/add_torrents").route(web::post().to(receive_files)))
        .service(Files::new(&root, "static/").index_file("index.html"))})
        .bind(format!("127.0.0.1:{}",port))?.system_exit().run().await
}

/// Add a torrent to the list
/// If the filename does not end with .torrent, the file is not processed
fn add_torrent(path: String) {
    if path.to_lowercase().ends_with(".torrent") {
        let c=&*CONFIG.read().expect("Cannot read configuration");
        let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
        log::info!("Loading torrent: \t{}", path);
        let t = Torrent::read_from_file(&path);
        if t.is_ok() {
            let mut t = torrent::BasicTorrent::from_torrent(t.unwrap(), path);
            //enable seeding on public torrents depending on the config value of seed_public_torrent
            if c.seed_public_torrent && !t.private {t.active = true;}
            else {t.active = false;}
            for bt in list.clone() { if bt.info_hash == t.info_hash {
                log::info!("Torrent is already in list");
                return;
            }}
            list.push(t);
        } else {log::error!("Cannot parse torrent: \t{}", path);}
    }
}
