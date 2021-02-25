extern crate rand;
extern crate clap;

use clap::{Arg, SubCommand, value_t};
use actix_web::{get, web, App, Error, HttpRequest, HttpServer, Responder};
use actix_files::Files;

mod client;
mod algorithm;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let c:  client::Client;
    println!("RatioUp");
    for c in client::load_clients().into_iter() {
        println!("{}", c.0);
    }
    let matches = clap::App::new("RatioUp")
                          .arg(Arg::with_name("WEB_ROOT")
                               .long("root")
                               .value_name("PATH")
                               .default_value("/")
                               .help("Set a custom web root (ex: / or /ratio-up")
                               .takes_value(true))
                          .arg(Arg::with_name("PORT")
                               .short("p")
                               .long("port")
                               .default_value("7070")
                               .help("Sets HTTP web port")
                               .takes_value(true))
                          .get_matches();
    //TODO: check arguments
    //let listen_addr = matches.value_of("listen_addr").unwrap();
    let port = value_t!(matches, "PORT", u16).unwrap_or_else(|e| e.exit());
    //example: https://github.com/actix/examples/blob/master/http-proxy/src/main.rs
    HttpServer::new(|| {App::new()
        .service(Files::new("/", "static/").index_file("index.html"))})
        .bind(format!("127.0.0.1:{}",port))?.system_exit().run().await
}