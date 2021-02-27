#![allow(non_snake_case)]

extern crate rand;
extern crate clap;

use clap::{Arg, SubCommand, value_t};
use std::time::{Duration, Instant};
use actix::prelude::*;
use actix_web::{middleware, web, App, Error, HttpRequest, HttpResponse, HttpServer};
use actix_web_actors::ws;
use actix_files::Files;
use serde_json::{Map, Value};

mod client;
mod algorithm;
mod config;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const CLIENT_TIMEOUT: Duration = Duration::from_secs(30);

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
    /*for c in client::load_clients().into_iter() {
        println!("{}", c.0);
    }*/
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
    //if command line argument upgrade config.json
    
    HttpServer::new(move || {App::new()
        .service(web::resource("/ws/").route(web::get().to(ws_index)))
        .service(Files::new(&root, "static/").index_file("index.html"))})
        .bind(format!("127.0.0.1:{}",port))?.system_exit().run().await
}
