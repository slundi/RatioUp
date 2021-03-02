#![allow(non_snake_case)]

extern crate rand;
extern crate clap;

use clap::{Arg, value_t};
use serde_json::json;
use std::{borrow::{Borrow, BorrowMut}, collections::BTreeMap, time::{Duration, Instant}};
use actix::prelude::*;
use actix_web::{middleware, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use actix_files::Files;
use log::{info};

mod client;
mod algorithm;
mod config;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

thread_local! {
    pub static clients : BTreeMap<&'static str, client::Client<'static>> = client::load_clients();
}

/// do websocket handshake and start `RatioUpWS` actor
async fn ws_index(r: HttpRequest, stream: web::Payload) -> Result<HttpResponse, Error> {
    //println!("{:?}", r);
    let res = ws::start(RatioUpWS::new(), &r, stream);
    //println!("{:?}", res);
    res
}

/// websocket connection is long running connection, it easier to handle with an actor
struct RatioUpWS {
    /// Client must send ping at least once per 30 seconds (CLIENT_TIMEOUT), otherwise we drop connection.
    hb: Instant,
    client:Option<client::Client<'static> >,
}
impl Actor for RatioUpWS {
    type Context = ws::WebsocketContext<Self>;
    /// Method is called on actor start. We start the heartbeat process here.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
        //TODO: send the client list and the configured client
        //serde_json::to_value(client::load_clients());
        ctx.text("Hello");
        //load client list
        let mut client_list : Vec<&'static str>=Vec::with_capacity(54);
        clients.with(|l| {
            l.borrow();
            for c in l.into_iter() {client_list.push(c.0);}
        });
        println!("{}",json!(client_list));
        //TODO:send client list
        //ctx.text(serde_json::to_value(client_list));
    }
}

// Handler for `ws::Message`
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for RatioUpWS {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context,) {
        // process websocket messages
        println!("WS: {:?}", msg);
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {self.hb = Instant::now();}
            Ok(ws::Message::Text(text)) => ctx.text(text),
            Ok(ws::Message::Binary(bin)) => ctx.binary(bin),
            Ok(ws::Message::Close(reason)) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}

impl RatioUpWS {
    fn new() -> Self {Self {
        hb: Instant::now(),
        client:None
    }}

    /// helper method that sends ping to client every second also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
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
    println!("RatioUp");
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
    //read config.json, if it does not exist, it creates a new one
    let mut cfg=config::read_config_file("config.json".to_owned());
    if cfg.is_err() {
        cfg=Ok(config::Config::default());
        info!("config.json does not exist, creating a new one");
        config::write_config_file("config.json".to_owned(), cfg.unwrap());
    }
    //create torrent folder
    let torrent_folder = std::path::Path::new("torrents");
    std::fs::create_dir_all(torrent_folder).expect("Cannot create torrent folder");
    //start web server
    HttpServer::new(move || {App::new()
        .service(web::resource("/ws/").route(web::get().to(ws_index)))
        .service(Files::new(&root, "static/").index_file("index.html"))})
        .bind(format!("127.0.0.1:{}",port))?.system_exit().run().await
}
