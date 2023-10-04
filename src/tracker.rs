// https://xbtt.sourceforge.net/udp_tracker_protocol.html
// based on https://github.com/billyb2/cratetorrent/blob/master/cratetorrent/src/tracker.rs

use std::{
    convert::TryInto,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use bytes::{BufMut, BytesMut};

use log::error;
use rand::prelude::*;

use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::torrent::BasicTorrent;

pub const URL_ENCODE_RESERVED: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'~')
    .remove(b'.');

pub const EVENT_NONE: &str = "";
pub const EVENT_COMPLETED: &str = "completed"; //not used because we do not download for now
pub const EVENT_STARTED: &str = "started";
pub const EVENT_STOPPED: &str = "stopped";

/// The optional announce event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Event {
    /// The first request to tracker must include this value.
    Started,
    /// Must be sent to the tracker when the client becomes a seeder. Must not be
    /// present if the client started as a seeder.
    Completed,
    /// Must be sent to tracker if the client is shutting down gracefully.
    Stopped,
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
pub fn announce(torrent: &BasicTorrent, event: Option<Event>) -> u64 {
    for url in torrent.urls {
        if url.to_lowercase().starts_with("udp://") {
            announce_udp(torrent, event)
        } else {
            announce_http(torrent, event)
        }
    }
    1800 //TODO
}

///https://www.bittorrent.org/beps/bep_0015.html
async fn connect_udp(ip_addr: SocketAddr) -> Option<i64> {
    //Bind to a random port
    let port = rand::thread_rng().gen_range(1025..u16::MAX);

    let sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .await
        .unwrap();

    //The magic protocol id number
    const PROTOCOL_ID: i64 = 0x41727101980;
    const ACTION: i32 = 0;
    let transaction_id: i32 = random();

    let mut bytes_to_send = BytesMut::with_capacity(16);
    bytes_to_send.put_i64(PROTOCOL_ID);
    bytes_to_send.put_i32(ACTION);
    bytes_to_send.put_i32(transaction_id.try_into().unwrap());

    let bytes_to_send = &bytes_to_send;

    let mut response_buf: [u8; 16] = [0; 16];

    sock.send_to(bytes_to_send, ip_addr).await.unwrap();

    let wait_time = std::time::Duration::from_secs(3);
    let mut attempts: u8 = 0;

    let mut could_connect = false;

    while response_buf == [0; 16] && attempts < 5 {
        could_connect = match timeout(wait_time, sock.recv_from(&mut response_buf)).await {
            Ok(_) => true,
            Err(_) => false,
        };

        attempts += 1;
    }

    let transaction_id_recv: i32 = i32::from_be_bytes((&response_buf[4..8]).try_into().unwrap());

    match could_connect && transaction_id == transaction_id_recv {
        true => Some(i64::from_be_bytes((&response_buf[8..]).try_into().unwrap())),
        false => None,
    }
}

fn announce_http(torrent: &BasicTorrent, event: Option<Event>) -> u64 {
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

    let client = crate::CLIENT
        .read()
        .expect("Cannot get client")
        .clone()
        .unwrap();
    let query = client.get_query();
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent(&client.user_agent);
    let mut req = agent
        .build()
        .get(url)
        .timeout(std::time::Duration::from_secs(90));
    req = query
        .1
        .into_iter()
        .fold(req, |req, header| req.set(&header.0, &header.1));
    match req.call() {
        Ok(resp) => todo!(),
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
    1800
}

async fn announce_udp(torrent: &BasicTorrent, event: Option<Event>) -> u64 {
    let port = thread_rng().gen_range(1025..u16::MAX);
    let mut sock = UdpSocket::bind(SocketAddr::from(([0, 0, 0, 0], port)))
        .await
        .unwrap();

    // All of the potential addressese of a URL
    let mut addrs = self.url.socket_addrs(|| None).unwrap();
    // Shuffle the list
    addrs.shuffle(&mut thread_rng());

    //TODO: Make an error for not finding an actual IPV4 address
    let addr = *addrs.iter().find(|a| a.is_ipv4()).unwrap();

    let mut failure_reason = None;

    let connection_id: i64 = connect_udp(addr).await.unwrap();

    const ACTION: i32 = 1;
    let transaction_id: i32 = random();
    let key: u32 = random();

    let mut bytes_to_send = BytesMut::with_capacity(150);
    bytes_to_send.put_i64(connection_id);
    bytes_to_send.put_i32(ACTION);
    bytes_to_send.put_i32(transaction_id);

    debug_assert_eq!(params.info_hash.len(), 20);
    debug_assert_eq!(params.peer_id.len(), 20);

    bytes_to_send.put(&params.info_hash[..]);
    bytes_to_send.put(&params.peer_id[..]);
    bytes_to_send.put_i64(params.downloaded.try_into().unwrap());
    bytes_to_send.put_i64(params.left.try_into().unwrap());
    bytes_to_send.put_i64(params.uploaded.try_into().unwrap());

    bytes_to_send.put_i32(match event {
        Some(val) => match val {
            Event::Completed => 1,
            Event::Started => 2,
            Event::Stopped => 3,
        },
        None => 0,
    });
    match params.ip {
        Some(ip) => {
            match ip {
                IpAddr::V4(ip) => {
                    bytes_to_send.put(&(ip.octets()[..]));
                }

                //The IP address field must be 32 bits wide, so if the IP given is v6, the field must be set to 0
                IpAddr::V6(_) => {
                    bytes_to_send.put_i32(0);
                }
            }
        }
        None => {
            bytes_to_send.put_i32(0);
        }
    };

    bytes_to_send.put_u32(key);
    bytes_to_send.put_i32(match params.peer_count {
        Some(num) => num.try_into().unwrap(),
        None => -1,
    });
    bytes_to_send.put_u16(params.port);

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
        Err(_) => failure_reason = Some(String::from("Couldn't announce to tracker")),
    };

    let transaction_id_recv: i32 = i32::from_be_bytes((&response_buf[4..8]).try_into().unwrap());

    let leechers: i32 = i32::from_be_bytes((&response_buf[12..16]).try_into().unwrap());
    let seeders: i32 = i32::from_be_bytes((&response_buf[16..20]).try_into().unwrap());

    let mut peer_vec: Vec<SocketAddr> = Vec::new();

    let mut index: usize = 20;

    while index <= response_buf.len() - 6 {
        let peer_ip_bytes: [u8; 4] = response_buf[index..index + 4].try_into().unwrap();
        let peer_port_bytes: [u8; 2] = response_buf[index + 4..index + 6].try_into().unwrap();

        if peer_ip_bytes != [0; 4] && peer_port_bytes != [0; 2] {
            let peer_ipv4 = Ipv4Addr::from(peer_ip_bytes);
            let peer_string = peer_ipv4.to_string();
            let peer_port_bytes: [u8; 2] = response_buf[index + 4..index + 6].try_into().unwrap();
            let peer_port: u16 = u16::from_be_bytes(peer_port_bytes);
            let peer_string = format!("{}:{}", peer_string, peer_port);
            let peer_sock: SocketAddr = peer_string.parse().unwrap();

            peer_vec.push(peer_sock);

            index += 6;
        } else {
            break;
        }
    }

    if transaction_id != transaction_id_recv {
        failure_reason = Some(String::from("Transaction ID's did not match"));
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
    1800
}

#[cfg(test)]
mod tests {
    // use super::*;
}
