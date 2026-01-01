use warp::Filter;
use warp::http::header::CONTENT_TYPE;

use crate::announcer::tracker;
use crate::torrent::TorrentJson;
use crate::{CONFIG, TORRENTS};

pub async fn run_web_server() {
    let config = CONFIG.get().unwrap();
    let bind_addr = config.http_bind_address.clone();
    let bind_port = config.http_port;
    let bind: std::net::SocketAddr = format!("{}:{}", bind_addr, bind_port).parse().unwrap();

    // API route to get statistics with JSON header
    let api = warp::path("api")
        .and(warp::path::end())
        .and_then(|| async move {
            let stats = get_statistics().await;
            let reply = warp::reply::json(&stats);
            let reply = warp::reply::with_header(reply, CONTENT_TYPE, "application/json");
            Ok::<_, warp::Rejection>(reply)
        });

    let static_files = warp::path("static").and(warp::fs::dir("static"));
    let root = warp::path::end().and(warp::fs::file("static/index.html"));
    let routes = root.or(api).or(static_files);
    warp::serve(routes).run(bind).await;
}

async fn get_statistics() -> Vec<TorrentJson> {
    let torrents = TORRENTS.read().await;
    let mut data = Vec::with_capacity(torrents.len());

    for torrent_mutex in torrents.iter() {
        let torrent = torrent_mutex.lock().await;
        data.push(torrent.to_json_struct()); // owned, no borrow issues
    }

    data
}

// async fn force_announce_all_torrents() {
//     let torrents = TORRENTS.read().await;
//     for torrent_mutex in torrents.iter() {
//         let mut torrent = torrent_mutex.lock().await;
//         tracker::announce(torrent, event)
//     }
// }
