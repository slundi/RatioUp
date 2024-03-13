use actix_files::Files;
use actix_web::{middleware, App, HttpServer};
use log::info;

use crate::webui::routes;

pub async fn run() {
    let config = crate::WS_CONFIG
        .get()
        .expect("Cannot get server configuration for the web UI thread")
        .clone();

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .service(routes::get_config)
            .service(routes::get_torrents)
            .service(routes::receive_files)
            .service(routes::process_user_command)
            .service(routes::health_check)
            .service(routes::torrent_files)
            .service(Files::new(&config.web_root, "static/").index_file("index.html"))
    })
    .bind(&config.server_addr)
    .expect("Cannot bind server address")
    .workers(1)
    .system_exit()
    .run();
    info!("Starting HTTP server at http://{}/", &config.server_addr);
    let _ = server.await;
}
