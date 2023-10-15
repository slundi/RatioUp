// https://xbtt.sourceforge.net/udp_tracker_protocol.html
// based on https://github.com/billyb2/cratetorrent/blob/master/cratetorrent/src/tracker.rs

use std::{convert::TryInto, io::Read, net::SocketAddr, time::Duration};

use bytes::{BufMut, BytesMut};

use fake_torrent_client::Client;
use log::{debug, error, info, warn};
use rand::prelude::*;

use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::torrent::BasicTorrent;
use crate::{CLIENT, TORRENTS};

pub const URL_ENCODE_RESERVED: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'~')
    .remove(b'.');


enum ErrorCode {
    /// Client request was not a HTTP GET
    InvalidRequestType = 100, 
    MissingInfosash = 101,
    MissingPeerId = 102,
    MissingPort = 103,
    /// infohash is not 20 bytes long.
    InvalidInfohash = 150,
    /// peerid is not 20 bytes long
    InvalidPeerId = 151,
    /// Client requested more peers than allowed by tracker
    InvalidNumwant = 152,
    /// info_hash not found in the database. Sent only by trackers that do not automatically include new hashes into the database.
    InfohashNotFound = 200, 
    /// Client sent an eventless request before the specified time.
    ClientSentEventlessRequest = 500,
    GenericError = 900,
}

/// The optional announce event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The first request to tracker must include this value.
    Started,
    /// Must be sent to the tracker when the client becomes a seeder. Must not be
    /// present if the client started as a seeder.
    Completed,
    /// Must be sent to tracker if the client is shutting down gracefully.
    Stopped,
}

/// The tracker responds with "text/plain" document consisting of a bencoded dictionary
#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct FailureTrackerResponse {
    /// If present, then no other keys may be present. The value is a human-readable error message as to why the request failed
    #[serde(rename = "failure reason")]
    pub reason: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq, Clone)]
pub struct OkTrackerResponse {
    /// (new, optional) Similar to failure reason, but the response still gets processed normally. The warning message is shown just like an error.
    #[serde(default, rename = "warning message")]
    pub warning_message: Option<String>,
    /// Interval in seconds that the client should wait between sending regular requests to the tracker
    pub interval: i64,
    /// (optional) Minimum announce interval. If present clients must not reannounce more frequently than this.
    #[serde(default, rename = "min interval")]
    pub min_interval: Option<i64>,
    /// A string that the client should send back on its next announcements. If absent and a previous announce sent a tracker id, do not discard the old value; keep using it.
    pub tracker_id: Option<String>,
    /// number of peers with the entire file, i.e. seeders
    pub complete: i64,
    /// number of non-seeder peers, aka "leechers"
    pub incomplete: i64,
    /// (dictionary model) The value is a list of dictionaries, each with the following keys.
    /// peers: (binary model) Instead of using the dictionary model described above, the peers value may be a string consisting of multiples of 6 bytes. First 4 bytes are the IP address and last 2 bytes are the port number. All in network (big endian) notation.
    #[serde(default, skip_deserializing)]
    peers: Option<u8>,
}

pub fn announce_start() {
    debug!("Annouce start");
    let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
    for t in list {
        debug!("Start: announcing {}", t.name);
        t.interval = announce(t, Some(Event::Started));
    }
}

pub async fn announce_stopped() {
    // TODO: compute uploaded and downloaded then announce
    let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
    for t in list {
        t.interval = announce(t, Some(Event::Stopped));
    }
}

/// Sends an announce request to the tracker with the specified parameters.
///
/// This may be used by a torrent to request peers to download from and to
/// report statistics to the tracker.
///
/// # Important
///
/// The tracker may not be contacted more often than the minimum interval
/// returned in the first announce response.
pub fn announce(torrent: &mut BasicTorrent, event: Option<Event>) -> u64 {
    let mut interval = 4_294_967_295u64;
    if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
        debug!("Torrent has {} url(s)", torrent.urls.len());
        for url in torrent.urls.clone() {
            if url.to_lowercase().starts_with("udp://") {
                //interval = futures::executor::block_on(announce_udp(&url, torrent, client, event));
            } else {
                interval = announce_http(&url, torrent, client, event);
            }
        }
        info!("Anounced: interval={}, event={:?}, downloaded={}, uploaded={}, seeders={}, leechers={}, torrent={}", torrent.interval, event, torrent.downloaded, torrent.uploaded, torrent.seeders, torrent.leechers, torrent.name);
    }
    interval
}

fn announce_http(
    url: &str,
    torrent: &mut BasicTorrent,
    client: &Client,
    event: Option<Event>,
) -> u64 {
    // announce parameters are built up in the query string, see:
    // https://www.bittorrent.org/beps/bep_0003.html trackers section
    // let mut query = vec![
    //     ("port", params.port.to_string()),
    //     ("downloaded", params.downloaded.to_string()),
    //     ("uploaded", params.uploaded.to_string()),
    //     ("left", params.left.to_string()),
    //     // Indicates that client accepts a compact response (each peer takes
    //     // up only 6 bytes where the first four bytes constitute the IP
    //     // address and the last 2 the port number, in Network Byte Order).
    //     // The is always true to save network traffic (many trackers don't
    //     // consider this and send compact lists anyway).
    //     ("compact", "1".to_string()),
    // ];
    // if let Some(peer_count) = params.peer_count {
    //     query.push(("numwant", peer_count.to_string()));
    // }
    // if let Some(ip) = &params.ip {
    //     query.push(("ip", ip.to_string()));
    // }

    // hack:
    // reqwest uses serde_urlencoded which doesn't support encoding a raw
    // byte array into a percent encoded string. However, the tracker
    // expects the url encoded form of the raw info hash, so we need to be
    // able to map the raw bytes to its url encoded form. The peer id is
    // also stored as a raw byte array. Using `String::from_utf8_lossy`
    // would cause information loss.
    //
    // We do this using the separate percent_encoding crate, and by
    // "hard-coding" the info hash and the peer id into the url string. This
    // is the only way in which reqwest doesn't url encode again the custom
    // url encoded info hash. All other methods, such as mutating the query
    // parameters on the `Url` object, or by serializing the info hash with
    // `serde_bytes` do not work: they throw an error due to expecting valid
    // utf8.
    //
    // However, this is decidedly _not_ great: we're relying on an
    // undocumented edge case of a third party library (reqwest) that may
    // very well break in a future update.
    // let url = format!(
    //     "{url}\
    //     ?info_hash={info_hash}\
    //     &peer_id={peer_id}",
    //     url = url,
    //     info_hash = percent_encoding::percent_encode(&params.info_hash, URL_ENCODE_RESERVED),
    //     peer_id = percent_encoding::percent_encode(&params.peer_id, URL_ENCODE_RESERVED),
    // );

    let query = client.get_query();
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(&client.user_agent);
    let built_url = build_url(url, torrent, event, client.key.clone());
    debug!("Announce HTTP URL {:?}", built_url);
    let mut req = agent
        .build()
        .get(&built_url)
        .timeout(std::time::Duration::from_secs(90));
    req = query
        .1
        .into_iter()
        .fold(req, |req, header| req.set(&header.0, &header.1));
    match req.call() {
        Ok(resp) => {
            let code = resp.status();
                info!(
                    "\tTime since last announce: {}s \t interval: {}",
                    torrent.last_announce.elapsed().as_secs(),
                    torrent.interval
                );
                let mut bytes: Vec<u8> = Vec::with_capacity(2048);
                resp.into_reader()
                    .take(1024)
                    .read_to_end(&mut bytes)
                    .expect("Cannot read response");
                //we start to check if the tracker has returned an error message, if yes, we will reannounce later
                debug!(
                    "Tracker response: {:?}",
                    String::from_utf8_lossy(&bytes.clone())
                );
                match serde_bencode::from_bytes::<OkTrackerResponse>(&bytes.clone()) {
                    Ok(tr) => {
                        torrent.seeders = u16::try_from(tr.complete).unwrap();
                        torrent.leechers = u16::try_from(tr.incomplete).unwrap();
                        torrent.interval = u64::try_from(tr.interval).unwrap();
                        info!(
                            "\tSeeders: {}\tLeechers: {}\t\t\tInterval: {:?}s",
                            tr.incomplete, tr.complete, tr.interval
                        );
                    }
                    Err(e1) => {
                        match serde_bencode::from_bytes::<FailureTrackerResponse>(&bytes.clone()) {
                            Ok(tr) => warn!("Cannot announce: {}", tr.reason),
                            Err(e2) => {
                                error!("Cannot process tracker response: {:?}, {:?}", e1, e2)
                            }
                        }
                    }
                }
                if code != actix_web::http::StatusCode::OK {
                    info!("\tResponse: code={}\tdata={:?}", code, bytes);
                }
        },
        Err(err) => error!("Cannot announce: {:?}", err),
    }
    // send request
    // let resp = self
    //     .client
    //     .get(&url)
    //     .query(&query)
    //     .send()
    //     .await?
    //     .error_for_status()?
    //     .bytes()
    //     .await?;
    // let resp = serde_bencode::from_bytes(&resp)?;
    // Ok(resp)
    torrent.interval
}

/// Build the HTTP announce URLs for the listed trackers in the torrent file.
/// It prepares the annonce query by replacing variables (port, numwant, ...) with the computed values
pub fn build_url(url: &str, torrent: &mut BasicTorrent, event: Option<Event>, key: String) -> String {
    info!("Torrent {:?}: {}", event, torrent.name);
    //compute downloads and uploads
    let elapsed: usize = if event == Some(Event::Started) {
        0
    } else {
        torrent.last_announce.elapsed().as_secs() as usize
    };
    let uploaded: usize = torrent.next_upload_speed as usize * elapsed;
    let mut downloaded: usize = torrent.next_download_speed as usize * elapsed;
    if torrent.length <= torrent.downloaded + downloaded {
        downloaded = torrent.length - torrent.downloaded;
    } //do not download more thant the torrent size
    torrent.downloaded += downloaded;

    //build URL list
    let client = (*CLIENT.read().expect("Cannot read client")).clone().unwrap();
    let mut result = String::from(url);
    result.push('?');
    result.push_str(&client.query);
    let result = result
        .replace("{infohash}", &torrent.info_hash_urlencoded)
        .replace("{key}", &key)
        .replace("{uploaded}", uploaded.to_string().as_str())
        .replace("{downloaded}", downloaded.to_string().as_str())
        .replace("{peerid}", &client.peer_id)
        .replace("{port}", &crate::CONFIG.get().unwrap().port.to_string())
        .replace("{numwant}", &client.num_want.to_string())
        .replace("ipv6={ipv6}", "")
        .replace(
            "{left}",
            (torrent.length - torrent.downloaded).to_string().as_str(),
        )
        .replace(
            "{event}",
            match event {
                Some(e) => match e {
                    Event::Started => "started",
                    Event::Completed => "completed",
                    Event::Stopped => "stopped",
                },
                None => "",
            },
        );
    info!(
        "\tDownloaded: {} \t Uploaded: {}",
        byte_unit::Byte::from_bytes(downloaded as u128)
            .get_appropriate_unit(true)
            .to_string(),
        byte_unit::Byte::from_bytes(uploaded as u128)
            .get_appropriate_unit(true)
            .to_string()
    );
    info!("\tAnnonce at: {}", url);
    result
}

async fn announce_udp(
    url: &str,
    torrent: &mut BasicTorrent,
    client: &Client,
    event: Option<Event>,
) -> u64 {
    let port = thread_rng().gen_range(1025..u16::MAX);
    let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .await
        .unwrap();

    // All of the potential addressese of a URL
    let url = reqwest::Url::parse(url).expect("Cannot parse tracker URL");
    debug!("Announce UDP URL  {:?}", url);
    let mut addrs = url.socket_addrs(|| None).unwrap();
    // Shuffle the list
    addrs.shuffle(&mut thread_rng());

    //TODO: Make an error for not finding an actual IPV4 address
    let addr = *addrs.iter().find(|a| a.is_ipv4()).unwrap();

    let mut failure_reason = None;

    let connection_id: i64 = (connect_udp(addr).await).unwrap();
    debug!("announce_udp: connect_id={}", connection_id);

    const ACTION: i32 = 1;
    let transaction_id: i32 = random();
    let key: u32 = random();

    let mut bytes_to_send = BytesMut::with_capacity(150);
    bytes_to_send.put_i64(connection_id);
    bytes_to_send.put_i32(ACTION);
    bytes_to_send.put_i32(transaction_id);

    debug_assert_eq!(torrent.info_hash.len(), 20);
    debug_assert_eq!(client.peer_id.len(), 20);

    bytes_to_send.put(torrent.info_hash.as_bytes());
    bytes_to_send.put(client.peer_id.as_bytes());
    bytes_to_send.put_i64(torrent.downloaded.try_into().unwrap());
    bytes_to_send.put_i64((torrent.length - torrent.downloaded).try_into().unwrap());
    bytes_to_send.put_i64(torrent.uploaded.try_into().unwrap());

    bytes_to_send.put_i32(match event {
        Some(val) => match val {
            Event::Completed => 1,
            Event::Started => 2,
            Event::Stopped => 3,
        },
        None => 0,
    });
    // match params.ip {
    //     Some(ip) => {
    //         match ip {
    //             IpAddr::V4(ip) => {
    //                 bytes_to_send.put(&(ip.octets()[..]));
    //             }

    //             //The IP address field must be 32 bits wide, so if the IP given is v6, the field must be set to 0
    //             IpAddr::V6(_) => {
    //                 bytes_to_send.put_i32(0);
    //             }
    //         }
    //     }
    //     None => {
    //         bytes_to_send.put_i32(0);
    //     }
    // };
    bytes_to_send.put_i32(0); // IP not supported/useless in our case

    bytes_to_send.put_u32(key);
    if client.num_want == 0 {
        bytes_to_send.put_i32(-1);
    } else {
        bytes_to_send.put_i32(i32::try_from(client.num_want).unwrap());
    }
    // bytes_to_send.put_i32(match client.num_want {
    //     Some(num) => num.try_into().unwrap(),
    //     None => -1,
    // });
    let config = crate::CONFIG.get().expect("Cannot read configuration");
    bytes_to_send.put_u16(config.port);

    let bytes_to_send = &bytes_to_send;

    const MAX_NUM_PEERS: usize = 2048;

    //Supporting around a few hundred peers, just for the test

    let mut response_buf: [u8; MAX_NUM_PEERS] = [0; MAX_NUM_PEERS];

    sock.send_to(bytes_to_send, addr).await.unwrap();
    let wait_time = match connection_id {
        0 => Duration::from_secs(0),
        _ => Duration::from_secs(3),
    };

    match timeout(wait_time, sock.recv_from(&mut response_buf)).await {
        Ok(_) => (),
        Err(e) => failure_reason = Some(format!("timeout after {}s", e))
    };

    let transaction_id_recv: i32 = i32::from_be_bytes((&response_buf[4..8]).try_into().unwrap());
    let interval: i32 = i32::from_be_bytes((&response_buf[8..12]).try_into().unwrap());

    let leechers: i32 = i32::from_be_bytes((&response_buf[12..16]).try_into().unwrap());
    let seeders: i32 = i32::from_be_bytes((&response_buf[16..20]).try_into().unwrap());
    torrent.leechers = u16::try_from(leechers).unwrap();
    torrent.seeders = u16::try_from(seeders).unwrap();

    // let mut peer_vec: Vec<SocketAddr> = Vec::new();

    // let mut index: usize = 20;

    // while index <= response_buf.len() - 6 {
    //     let peer_ip_bytes: [u8; 4] = response_buf[index..index + 4].try_into().unwrap();
    //     let peer_port_bytes: [u8; 2] = response_buf[index + 4..index + 6].try_into().unwrap();

    //     if peer_ip_bytes != [0; 4] && peer_port_bytes != [0; 2] {
    //         let peer_ipv4 = Ipv4Addr::from(peer_ip_bytes);
    //         let peer_string = peer_ipv4.to_string();
    //         let peer_port_bytes: [u8; 2] = response_buf[index + 4..index + 6].try_into().unwrap();
    //         let peer_port: u16 = u16::from_be_bytes(peer_port_bytes);
    //         let peer_string = format!("{}:{}", peer_string, peer_port);
    //         let peer_sock: SocketAddr = peer_string.parse().unwrap();

    //         peer_vec.push(peer_sock);

    //         index += 6;
    //     } else {
    //         break;
    //     }
    // }

    if transaction_id != transaction_id_recv {
        failure_reason = Some(String::from("Transaction ID's did not match"));
    }
    if let Some(fr) = failure_reason {
        warn!("Cannot announce: {}", fr);
    }

    // let response = Response {
    //     tracker_id: Some(transaction_id_recv.to_string()),
    //     failure_reason,
    //     warning_message: None,
    //     min_interval: None,
    //     interval: Some(Duration::from_secs(9)),
    //     leecher_count: Some(leechers.try_into().unwrap()),
    //     seeder_count: Some(seeders.try_into().unwrap()),
    //     peers: peer_vec,
    // };

    // Ok(response)
    debug!("UDP announced:  interval: {}, seeders: {}, leecherc: {}", interval, seeders, leechers);
    u64::try_from(interval).unwrap()
}

///https://www.bittorrent.org/beps/bep_0015.html
async fn connect_udp(ip_addr: SocketAddr) -> Option<i64> {
    //Bind to a random port
    let port = rand::thread_rng().gen_range(1025..u16::MAX);

    let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .await
        .unwrap();
    debug!("connect_udp: sock={:?}", sock);

    //The magic protocol id number
    const PROTOCOL_ID: i64 = 0x41727101980;
    const ACTION: i32 = 0;
    let transaction_id: i32 = random();
    debug!("connect_udp: transaction_id={}", transaction_id);

    let mut bytes_to_send = BytesMut::with_capacity(16);
    bytes_to_send.put_i64(PROTOCOL_ID);
    bytes_to_send.put_i32(ACTION);
    bytes_to_send.put_i32(transaction_id);

    let bytes_to_send = &bytes_to_send;

    let mut response_buf: [u8; 16] = [0; 16];

    debug!("connect_udp: will send 1st data");
    let ready = sock.ready(tokio::io::Interest::READABLE | tokio::io::Interest::WRITABLE).await;
    debug!("connect_udp: ready? {:?}", ready);
    sock.send_to(bytes_to_send, ip_addr).await.unwrap();
    debug!("connect_udp: 1st data send");

    let wait_time = std::time::Duration::from_secs(3);
    let mut attempts: u8 = 0;

    let mut could_connect = false;

    while response_buf == [0; 16] && attempts < 5 {
        could_connect = timeout(wait_time, sock.recv_from(&mut response_buf))
            .await
            .is_ok();
        attempts += 1;
    }

    let transaction_id_recv: i32 = i32::from_be_bytes((&response_buf[4..8]).try_into().unwrap());

    match could_connect && transaction_id == transaction_id_recv {
        true => Some(i64::from_be_bytes((&response_buf[8..]).try_into().unwrap())),
        false => None,
    }
}

#[cfg(test)]
mod tests {
    // use super::*;
}
