#![allow(non_snake_case)]

extern crate rand;
extern crate clap;
extern crate lazy_static;

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

//mod client;
mod algorithm;
mod config;
mod messages;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

lazy_static! {
    static ref CONFIG: RwLock<config::Config> = RwLock::new(config::get_config("config.json"));
    static ref ACTIVE: RwLock<bool> = RwLock::new(true);
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
        let mut f = web::block(|| std::fs::File::create(filepath)).await??; // File::create is blocking operation, use threadpool
        while let Some(chunk) = field.try_next().await? { // Field in turn is stream of *Bytes* object
            // filesystem operations are blocking, we have to use threadpool
            f = web::block(move || f.write_all(&chunk).map(|_| f)).await??;
        }
    }
    Ok(HttpResponse::Ok().into())
}

/// websocket connection is long running connection, it easier to handle with an actor
struct RatioUpWS {
    /// Client must send ping at least once per 30 seconds (CLIENT_TIMEOUT), otherwise we drop connection.
    hb: Instant,
}
impl Actor for RatioUpWS {
    type Context = ws::WebsocketContext<Self>;
    /// Method is called on actor start. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
        let c=&*CONFIG.read().expect("Cannot read configuration");
        ctx.text(format!("{{\"config\":{}}}", json!(c)));
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

                } else if text.starts_with("{\"remove\":\"") { //remove a torrent

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
    //start web server
    HttpServer::new(move || {App::new()
        .wrap(Logger::default())
        .service(web::resource("/ws/").route(web::get().to(ws_index)))
        .service(web::resource("/add_torrents").route(web::post().to(receive_files)))
        .service(Files::new(&root, "static/").index_file("index.html"))})
        .bind(format!("127.0.0.1:{}",port))?.system_exit().run().await
}
