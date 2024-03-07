use bytes::{BufMut, BytesMut};
use fake_torrent_client::Client;
use log::debug;
use log::warn;
use rand::prelude::*;
use rand::Rng;
use std::{convert::TryInto, net::SocketAddr, time::Duration};
use tokio::net::UdpSocket;
use tokio::time::timeout;

use crate::torrent::BasicTorrent;
use crate::CONFIG;

use super::tracker::Event;

/// Get a random UDP port, if the port is used, try the next port
pub async fn get_udp_socket() -> UdpSocket {
    // TODO : Currently this function exhaustively checks for each port and tries to
    // give one of the ports incrementing from 6881
    let config = CONFIG.get().unwrap();
    //Bind to a random port
    let mut port = rand::thread_rng().gen_range(1025..65000);
    // TODO: Get a list of all the ports used by the entire application as well,
    // i.e store a global use  of entire sockets somewhere in a global state
    //
    // Gets a port that is not used by the application
    loop {
        let adr = format!("0.0.0.0:{}", config.port);
        match UdpSocket::bind(adr).await {
            Ok(socket) => {
                return socket;
            }
            Err(_e) => {
                //println!("{:?}", e.to_string());
                port = port + 1;
            }
        }
    }
}

async fn announce_udp(
    url: &str,
    torrent: &mut BasicTorrent,
    client: &Client,
    event: Option<Event>,
) -> u64 {
    // All of the potential addressese of a URL
    let url = reqwest::Url::parse(url).expect("Cannot parse tracker URL");
    debug!("Announce UDP URL  {:?}", url);
    let mut addrs = url.socket_addrs(|| None).unwrap();
    // Shuffle the list
    addrs.shuffle(&mut thread_rng());

    //TODO: Make an error for not finding an actual IPV4 address
    let addr = *addrs.iter().find(|a| a.is_ipv4()).unwrap();

    let mut failure_reason = None;

    let result = (connect_udp(addr).await).unwrap();
    let connection_id: i64 = result.0;
    let sock = result.1;
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
        Err(e) => failure_reason = Some(format!("timeout after {}s", e)),
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
    debug!(
        "UDP announced:  interval: {}, seeders: {}, leecherc: {}",
        interval, seeders, leechers
    );
    u64::try_from(interval).unwrap()
}

///https://www.bittorrent.org/beps/bep_0015.html
async fn connect_udp(ip_addr: SocketAddr) -> Option<(i64, UdpSocket)> {
    let sock = crate::announcer::udp::get_udp_socket().await;
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
    let ready = sock
        .ready(tokio::io::Interest::READABLE | tokio::io::Interest::WRITABLE)
        .await;
    debug!("connect_udp: ready? {:?}", ready);
    sock.send_to(bytes_to_send, ip_addr).await.unwrap();
    debug!("connect_udp: 1st data send");

    let wait_time = std::time::Duration::from_secs(3);
    debug!("connect_udp: waited");
    let mut attempts: u8 = 0;
    debug!("connect_udp: attempts: {}", attempts);

    let mut could_connect = false;

    while response_buf == [0; 16] && attempts < 5 {
        could_connect = timeout(wait_time, sock.recv_from(&mut response_buf))
            .await
            .is_ok();
        attempts += 1;
    }

    let transaction_id_recv: i32 = i32::from_be_bytes((&response_buf[4..8]).try_into().unwrap());

    match could_connect && transaction_id == transaction_id_recv {
        true => Some((
            i64::from_be_bytes((&response_buf[8..]).try_into().unwrap()),
            sock,
        )),
        false => None,
    }
}
