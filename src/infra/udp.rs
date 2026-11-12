//! UDP tracker transport (BEP 15).
//!
//! - <https://xbtt.sourceforge.net/udp_tracker_protocol.html>
//! - <https://www.bittorrent.org/beps/bep_0015.html>

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use fake_torrent_client::Client;
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::{debug, info};
use url::Url;

use crate::infra::tracker::{
    AnnounceError, AnnounceFuture, AnnounceRequest, AnnounceResponse, Event, TrackerTransport,
};

/// Magic constant identifying the UDP tracker protocol.
const PROTOCOL_ID: u64 = 0x41727101980;
const CONNECT_ACTION: u32 = 0;
const ANNOUNCE_ACTION: u32 = 1;
const ERROR_ACTION: u32 = 3;
/// Default time to wait for each tracker answer.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct TrackerRequest {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub downloaded: u64,
    pub left: u64,
    pub uploaded: u64,
    pub event: Option<Event>,
    pub key: u32,
    pub num_want: i32,
    pub port: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub struct TrackerResponse {
    pub interval: u32,
    pub leechers: u32,
    pub seeders: u32,
    pub peers: Vec<SocketAddr>,
}

#[derive(Debug)]
pub enum TrackerError {
    IoError(std::io::Error),
    Timeout,
    InvalidResponse,
    Remote(String),
    ParseError,
}

impl std::fmt::Display for TrackerError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrackerError::IoError(error) => write!(formatter, "IO error: {error}"),
            TrackerError::Timeout => write!(formatter, "connection timed out"),
            TrackerError::InvalidResponse => write!(formatter, "invalid response from tracker"),
            TrackerError::Remote(msg) => write!(formatter, "tracker error: {msg}"),
            TrackerError::ParseError => write!(formatter, "failed to parse tracker URL"),
        }
    }
}

impl From<std::io::Error> for TrackerError {
    fn from(error: std::io::Error) -> Self {
        TrackerError::IoError(error)
    }
}

impl From<TrackerError> for AnnounceError {
    fn from(error: TrackerError) -> Self {
        match error {
            TrackerError::Remote(msg) => AnnounceError::Rejected(msg),
            other => AnnounceError::Transport(other.to_string()),
        }
    }
}

/// BEP 15 event code: 0=none, 1=completed, 2=started, 3=stopped.
fn event_code(event: Option<Event>) -> u32 {
    match event {
        Some(Event::Started) => 2,
        Some(Event::Stopped) => 3,
        None => 0,
    }
}

fn read_u32(buffer: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(buffer[offset..offset + 4].try_into().unwrap())
}

fn connect_packet(transaction_id: u32) -> [u8; 16] {
    let mut packet = [0u8; 16];
    packet[0..8].copy_from_slice(&PROTOCOL_ID.to_be_bytes());
    packet[8..12].copy_from_slice(&CONNECT_ACTION.to_be_bytes());
    packet[12..16].copy_from_slice(&transaction_id.to_be_bytes());
    packet
}

/// Extract the connection ID from a connect response.
fn parse_connect_response(buffer: &[u8], transaction_id: u32) -> Result<u64, TrackerError> {
    if buffer.len() < 16
        || read_u32(buffer, 0) != CONNECT_ACTION
        || read_u32(buffer, 4) != transaction_id
    {
        return Err(TrackerError::InvalidResponse);
    }
    Ok(u64::from_be_bytes(buffer[8..16].try_into().unwrap()))
}

fn announce_packet(connection_id: u64, transaction_id: u32, request: &TrackerRequest) -> Vec<u8> {
    let mut packet = Vec::with_capacity(98);
    packet.extend_from_slice(&connection_id.to_be_bytes());
    packet.extend_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
    packet.extend_from_slice(&transaction_id.to_be_bytes());
    packet.extend_from_slice(&request.info_hash);
    packet.extend_from_slice(&request.peer_id);
    packet.extend_from_slice(&request.downloaded.to_be_bytes());
    packet.extend_from_slice(&request.left.to_be_bytes());
    packet.extend_from_slice(&request.uploaded.to_be_bytes());
    packet.extend_from_slice(&event_code(request.event).to_be_bytes());
    packet.extend_from_slice(&0u32.to_be_bytes()); // IP address (0 = default)
    packet.extend_from_slice(&request.key.to_be_bytes());
    packet.extend_from_slice(&request.num_want.to_be_bytes());
    packet.extend_from_slice(&request.port.to_be_bytes());
    packet
}

/// Decode an announce response (or the tracker's error message).
fn parse_announce_response(
    buffer: &[u8],
    transaction_id: u32,
) -> Result<TrackerResponse, TrackerError> {
    if buffer.len() < 8 {
        return Err(TrackerError::InvalidResponse);
    }
    if read_u32(buffer, 0) == ERROR_ACTION {
        let msg = String::from_utf8_lossy(&buffer[8..]);
        return Err(TrackerError::Remote(msg.to_string()));
    }
    if buffer.len() < 20
        || read_u32(buffer, 0) != ANNOUNCE_ACTION
        || read_u32(buffer, 4) != transaction_id
    {
        return Err(TrackerError::InvalidResponse);
    }

    let peers_data = &buffer[20..];
    if !peers_data.len().is_multiple_of(6) {
        return Err(TrackerError::InvalidResponse);
    }
    let peers = peers_data
        .chunks_exact(6)
        .map(|chunk| {
            let ip = Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
            SocketAddr::new(ip.into(), u16::from_be_bytes([chunk[4], chunk[5]]))
        })
        .collect();

    Ok(TrackerResponse {
        interval: read_u32(buffer, 8),
        leechers: read_u32(buffer, 12),
        seeders: read_u32(buffer, 16),
        peers,
    })
}

/// Fixed-size peer ID: truncated to 20 bytes or zero padded.
fn peer_id_bytes(peer_id: &str) -> [u8; 20] {
    let bytes = peer_id.as_bytes();
    let mut result = [0u8; 20];
    let len = bytes.len().min(20);
    result[..len].copy_from_slice(&bytes[..len]);
    result
}

/// Saturating conversion of tracker counters to the `u16` stored in a torrent.
fn saturate_u16(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

pub struct UdpTracker {
    socket: UdpSocket,
    tracker_addr: SocketAddr,
    timeout: Duration,
}

impl UdpTracker {
    pub async fn new(tracker_addr: SocketAddr, timeout: Duration) -> Result<Self, TrackerError> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Self {
            socket,
            tracker_addr,
            timeout,
        })
    }

    pub async fn announce(
        &self,
        request: &TrackerRequest,
    ) -> Result<TrackerResponse, TrackerError> {
        let connection_id = self.connect().await?;
        let transaction_id = fastrand::u32(..);
        let packet = announce_packet(connection_id, transaction_id, request);
        let mut buffer = [0u8; 1024];
        let received = self.exchange(&packet, &mut buffer).await?;
        parse_announce_response(&buffer[..received], transaction_id)
    }

    async fn connect(&self) -> Result<u64, TrackerError> {
        let transaction_id = fastrand::u32(..);
        let mut buffer = [0u8; 16];
        let received = self
            .exchange(&connect_packet(transaction_id), &mut buffer)
            .await?;
        parse_connect_response(&buffer[..received], transaction_id)
    }

    /// Send a packet and wait for the answer, within the timeout.
    async fn exchange(&self, packet: &[u8], buffer: &mut [u8]) -> Result<usize, TrackerError> {
        self.socket.send_to(packet, self.tracker_addr).await?;
        match timeout(self.timeout, self.socket.recv(buffer)).await {
            Ok(Ok(received)) => Ok(received),
            Ok(Err(error)) => Err(TrackerError::IoError(error)),
            Err(_) => Err(TrackerError::Timeout),
        }
    }
}

/// Parse a UDP tracker URL and resolve the hostname to a SocketAddr.
async fn resolve_tracker_addr(url: &str) -> Result<SocketAddr, TrackerError> {
    let parsed = Url::parse(url).map_err(|_| TrackerError::ParseError)?;

    if parsed.scheme() != "udp" {
        return Err(TrackerError::ParseError);
    }

    let host = parsed.host_str().ok_or(TrackerError::ParseError)?;
    let port = parsed.port().ok_or(TrackerError::ParseError)?;

    // Resolve hostname to IP address using tokio's DNS resolution
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host(format!("{host}:{port}"))
        .await
        .map_err(TrackerError::IoError)?
        .collect();

    // Prefer IPv4 address, fall back to any available
    addrs
        .iter()
        .find(|addr| addr.is_ipv4())
        .or_else(|| addrs.first())
        .copied()
        .ok_or_else(|| TrackerError::Remote(format!("Could not resolve hostname: {host}")))
}

/// Announces over UDP.
#[derive(Debug, Clone)]
pub struct UdpTransport {
    timeout: Duration,
}

impl Default for UdpTransport {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl UdpTransport {
    #[cfg(test)]
    fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }

    async fn send(
        &self,
        url: &str,
        request: &AnnounceRequest,
        client: &Client,
    ) -> Result<AnnounceResponse, AnnounceError> {
        debug!("UDP announce to {}", url);
        let tracker_addr = resolve_tracker_addr(url).await?;
        let tracker = UdpTracker::new(tracker_addr, self.timeout).await?;
        let udp_request = TrackerRequest {
            info_hash: request.info_hash,
            peer_id: peer_id_bytes(&client.peer_id),
            downloaded: 0,
            left: 0,
            uploaded: request.uploaded,
            event: request.event,
            key: client.key,
            num_want: i32::from(request.numwant),
            port: request.port,
        };
        let response = tracker.announce(&udp_request).await?;
        info!(
            "UDP announce OK: interval={}, seeders={}, leechers={}, peers={}",
            response.interval,
            response.seeders,
            response.leechers,
            response.peers.len()
        );
        Ok(AnnounceResponse {
            interval: Some(u64::from(response.interval)),
            seeders: Some(saturate_u16(response.seeders)),
            leechers: Some(saturate_u16(response.leechers)),
            ..AnnounceResponse::default()
        })
    }
}

impl TrackerTransport for UdpTransport {
    fn announce<'a>(
        &'a self,
        url: &'a str,
        request: &'a AnnounceRequest,
        client: &'a Client,
    ) -> AnnounceFuture<'a> {
        Box::pin(self.send(url, request, client))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(event: Option<Event>) -> TrackerRequest {
        TrackerRequest {
            info_hash: [1u8; 20],
            peer_id: *b"TEST_PEER_ID_1234567",
            downloaded: 1024,
            left: 2048,
            uploaded: 512,
            event,
            key: 42,
            num_want: 30,
            port: 6881,
        }
    }

    fn announce_request() -> AnnounceRequest {
        AnnounceRequest {
            info_hash: [7u8; 20],
            info_hash_urlencoded: String::new(),
            uploaded: 4096,
            event: Some(Event::Started),
            port: 6881,
            numwant: 50,
        }
    }

    fn announce_response_bytes(transaction_id: u32, peers: &[([u8; 4], u16)]) -> Vec<u8> {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
        buffer.extend_from_slice(&transaction_id.to_be_bytes());
        buffer.extend_from_slice(&1800u32.to_be_bytes()); // interval
        buffer.extend_from_slice(&5u32.to_be_bytes()); // leechers
        buffer.extend_from_slice(&10u32.to_be_bytes()); // seeders
        for (ip, port) in peers {
            buffer.extend_from_slice(ip);
            buffer.extend_from_slice(&port.to_be_bytes());
        }
        buffer
    }

    /// How the fake UDP tracker answers the announce packet.
    enum Reply {
        Announce,
        Error(&'static str),
        Silent,
    }

    /// Fake UDP tracker answering one connect and one announce.
    /// Returns its URL and a handle yielding the announce packet received.
    async fn fake_udp_tracker(reply: Reply) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let address = socket.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let mut buffer = [0u8; 1024];
            let (received, peer) = socket.recv_from(&mut buffer).await.unwrap();
            assert_eq!(received, 16, "connect packet size");
            assert_eq!(read_u32(&buffer, 8), CONNECT_ACTION);
            let mut answer = Vec::new();
            answer.extend_from_slice(&CONNECT_ACTION.to_be_bytes());
            answer.extend_from_slice(&buffer[12..16]); // echo transaction ID
            answer.extend_from_slice(&0xDEAD_BEEF_u64.to_be_bytes()); // connection ID
            socket.send_to(&answer, peer).await.unwrap();

            let (received, peer) = socket.recv_from(&mut buffer).await.unwrap();
            let packet = buffer[..received].to_vec();
            let transaction_id = read_u32(&packet, 12);
            match reply {
                Reply::Announce => {
                    let answer = announce_response_bytes(transaction_id, &[([127, 0, 0, 1], 6881)]);
                    socket.send_to(&answer, peer).await.unwrap();
                }
                Reply::Error(msg) => {
                    let mut answer = Vec::new();
                    answer.extend_from_slice(&ERROR_ACTION.to_be_bytes());
                    answer.extend_from_slice(&transaction_id.to_be_bytes());
                    answer.extend_from_slice(msg.as_bytes());
                    socket.send_to(&answer, peer).await.unwrap();
                }
                Reply::Silent => {}
            }
            packet
        });
        (format!("udp://{address}/announce"), handle)
    }

    // --- packets ---

    #[test]
    fn test_connect_packet_layout() {
        let packet = connect_packet(0x0102_0304);
        assert_eq!(&packet[0..8], &PROTOCOL_ID.to_be_bytes());
        assert_eq!(read_u32(&packet, 8), CONNECT_ACTION);
        assert_eq!(read_u32(&packet, 12), 0x0102_0304);
    }

    #[test]
    fn test_announce_packet_layout() {
        let packet = announce_packet(0x41727101980, 12345, &make_request(Some(Event::Stopped)));
        assert_eq!(packet.len(), 98);
        assert_eq!(&packet[0..8], &0x41727101980u64.to_be_bytes());
        assert_eq!(read_u32(&packet, 8), ANNOUNCE_ACTION);
        assert_eq!(read_u32(&packet, 12), 12345);
        assert_eq!(&packet[16..36], &[1u8; 20]); // info hash
        assert_eq!(&packet[36..56], b"TEST_PEER_ID_1234567");
        assert_eq!(&packet[56..64], &1024u64.to_be_bytes()); // downloaded
        assert_eq!(&packet[64..72], &2048u64.to_be_bytes()); // left
        assert_eq!(&packet[72..80], &512u64.to_be_bytes()); // uploaded
        assert_eq!(read_u32(&packet, 80), 3); // event = stopped
        assert_eq!(read_u32(&packet, 88), 42); // key
        assert_eq!(&packet[92..96], &30i32.to_be_bytes()); // num_want
        assert_eq!(&packet[96..98], &6881u16.to_be_bytes()); // port
    }

    #[test]
    fn test_event_code() {
        assert_eq!(event_code(None), 0);
        assert_eq!(event_code(Some(Event::Started)), 2);
        assert_eq!(event_code(Some(Event::Stopped)), 3);
    }

    // --- responses ---

    #[test]
    fn test_parse_connect_response() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&CONNECT_ACTION.to_be_bytes());
        buffer.extend_from_slice(&7u32.to_be_bytes());
        buffer.extend_from_slice(&99u64.to_be_bytes());
        assert_eq!(parse_connect_response(&buffer, 7).unwrap(), 99);
        // Wrong transaction ID, wrong action, too short
        assert!(parse_connect_response(&buffer, 8).is_err());
        buffer[3] = 1;
        assert!(parse_connect_response(&buffer, 7).is_err());
        assert!(parse_connect_response(&buffer[..10], 7).is_err());
    }

    #[test]
    fn test_parse_announce_response_with_peers() {
        let buffer =
            announce_response_bytes(9, &[([192, 168, 1, 1], 6881), ([10, 0, 0, 1], 51413)]);
        let response = parse_announce_response(&buffer, 9).unwrap();
        assert_eq!(response.interval, 1800);
        assert_eq!(response.leechers, 5);
        assert_eq!(response.seeders, 10);
        assert_eq!(response.peers.len(), 2);
        assert_eq!(response.peers[0].to_string(), "192.168.1.1:6881");
        assert_eq!(response.peers[1].to_string(), "10.0.0.1:51413");
    }

    #[test]
    fn test_parse_announce_response_invalid() {
        let valid = announce_response_bytes(9, &[]);
        assert!(matches!(
            parse_announce_response(&valid, 10),
            Err(TrackerError::InvalidResponse)
        ));
        assert!(matches!(
            parse_announce_response(&valid[..12], 9),
            Err(TrackerError::InvalidResponse)
        ));
        assert!(matches!(
            parse_announce_response(&valid[..4], 9),
            Err(TrackerError::InvalidResponse)
        ));
        // Peer list not a multiple of 6 bytes
        let mut truncated = announce_response_bytes(9, &[([1, 2, 3, 4], 1)]);
        truncated.pop();
        assert!(matches!(
            parse_announce_response(&truncated, 9),
            Err(TrackerError::InvalidResponse)
        ));
    }

    #[test]
    fn test_parse_announce_response_short_error_message() {
        let mut buffer = Vec::new();
        buffer.extend_from_slice(&ERROR_ACTION.to_be_bytes());
        buffer.extend_from_slice(&1u32.to_be_bytes());
        buffer.extend_from_slice(b"banned");
        match parse_announce_response(&buffer, 1) {
            Err(TrackerError::Remote(msg)) => assert_eq!(msg, "banned"),
            other => panic!("unexpected: {other:?}"),
        }
    }

    // --- helpers ---

    #[test]
    fn test_peer_id_bytes_pads_and_truncates() {
        let short = peer_id_bytes("SHORT");
        assert_eq!(&short[..5], b"SHORT");
        assert!(short[5..].iter().all(|&byte| byte == 0));
        let long = peer_id_bytes("ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        assert_eq!(&long, b"ABCDEFGHIJKLMNOPQRST");
    }

    #[test]
    fn test_saturate_u16() {
        assert_eq!(saturate_u16(12), 12);
        assert_eq!(saturate_u16(70_000), u16::MAX);
    }

    #[test]
    fn test_tracker_error_display_and_conversion() {
        let io_err = std::io::Error::other("disk fail");
        assert!(
            TrackerError::IoError(io_err)
                .to_string()
                .contains("disk fail")
        );
        assert!(TrackerError::Timeout.to_string().contains("timed out"));
        assert!(
            TrackerError::InvalidResponse
                .to_string()
                .contains("invalid")
        );
        assert!(TrackerError::ParseError.to_string().contains("parse"));
        assert_eq!(
            AnnounceError::from(TrackerError::Remote(String::from("bad tracker"))),
            AnnounceError::Rejected(String::from("bad tracker"))
        );
        assert!(matches!(
            AnnounceError::from(TrackerError::Timeout),
            AnnounceError::Transport(_)
        ));
    }

    // --- resolve_tracker_addr ---

    #[tokio::test]
    async fn test_resolve_tracker_addr_valid() {
        let addr = resolve_tracker_addr("udp://localhost:6969/announce")
            .await
            .unwrap();
        assert_eq!(addr.port(), 6969);
    }

    #[tokio::test]
    async fn test_resolve_tracker_addr_invalid() {
        for url in [
            "http://tracker.example.com:6969/announce",
            "udp://tracker.example.com/announce",
            "not a url",
        ] {
            assert!(matches!(
                resolve_tracker_addr(url).await,
                Err(TrackerError::ParseError)
            ));
        }
    }

    // --- UdpTransport against a local fake tracker ---

    #[tokio::test]
    async fn test_udp_transport_success() {
        let (url, tracker) = fake_udp_tracker(Reply::Announce).await;
        let client = Client::default();
        let response = UdpTransport::default()
            .announce(&url, &announce_request(), &client)
            .await
            .unwrap();
        assert_eq!(response.interval, Some(1800));
        assert_eq!(response.seeders, Some(10));
        assert_eq!(response.leechers, Some(5));

        let packet = tracker.await.unwrap();
        assert_eq!(&packet[0..8], &0xDEAD_BEEF_u64.to_be_bytes()); // connection ID reused
        assert_eq!(&packet[16..36], &[7u8; 20]); // info hash
        assert_eq!(&packet[72..80], &4096u64.to_be_bytes()); // uploaded
        assert_eq!(read_u32(&packet, 80), 2); // started
        assert_eq!(&packet[92..96], &50i32.to_be_bytes()); // numwant
    }

    #[tokio::test]
    async fn test_udp_transport_tracker_error_is_rejected() {
        let (url, tracker) = fake_udp_tracker(Reply::Error("torrent not registered")).await;
        let result = UdpTransport::default()
            .announce(&url, &announce_request(), &Client::default())
            .await;
        assert_eq!(
            result,
            Err(AnnounceError::Rejected(String::from(
                "torrent not registered"
            )))
        );
        tracker.await.unwrap();
    }

    #[tokio::test]
    async fn test_udp_transport_timeout_is_transport_error() {
        let (url, tracker) = fake_udp_tracker(Reply::Silent).await;
        let result = UdpTransport::with_timeout(Duration::from_millis(200))
            .announce(&url, &announce_request(), &Client::default())
            .await;
        match result {
            Err(AnnounceError::Transport(reason)) => {
                assert!(reason.contains("timed out"), "reason: {reason}");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        tracker.await.unwrap();
    }

    #[tokio::test]
    async fn test_udp_transport_bad_url_is_transport_error() {
        let result = UdpTransport::default()
            .announce(
                "udp://tracker.example.com/announce",
                &announce_request(),
                &Client::default(),
            )
            .await;
        assert!(matches!(result, Err(AnnounceError::Transport(_))));
    }
}
