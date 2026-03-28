// https://xbtt.sourceforge.net/udp_tracker_protocol.html
// https://www.bittorrent.org/beps/bep_0015.html
use fake_torrent_client::Client;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;
use tokio::time::timeout;
use tracing::{debug, error, info, warn};
use url::Url;

use crate::CONFIG;
use crate::torrent::Torrent;

use super::tracker::Event;

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

#[derive(Debug)]
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
    TrackerError(String),
    ParseError,
}

impl std::fmt::Display for TrackerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrackerError::IoError(e) => write!(f, "IO error: {}", e),
            TrackerError::Timeout => write!(f, "connection timed out"),
            TrackerError::InvalidResponse => write!(f, "invalid response from tracker"),
            TrackerError::TrackerError(msg) => write!(f, "tracker error: {}", msg),
            TrackerError::ParseError => write!(f, "failed to parse tracker URL"),
        }
    }
}

impl From<std::io::Error> for TrackerError {
    fn from(err: std::io::Error) -> Self {
        TrackerError::IoError(err)
    }
}

pub struct UdpTracker {
    socket: UdpSocket,
    tracker_addr: SocketAddr,
}

impl UdpTracker {
    pub async fn new(tracker_addr: SocketAddr) -> Result<Self, TrackerError> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Self {
            socket,
            tracker_addr,
        })
    }

    pub async fn announce(
        &self,
        request: &TrackerRequest,
    ) -> Result<TrackerResponse, TrackerError> {
        // Étape 1: Connect
        let connection_id = self.connect().await?;

        // Étape 2: Announce
        self.announce_with_connection_id(connection_id, request)
            .await
    }

    async fn connect(&self) -> Result<u64, TrackerError> {
        const CONNECT_ACTION: u32 = 0;
        const PROTOCOL_ID: u64 = 0x41727101980; // Magic constant for BitTorrent

        let transaction_id = self.generate_transaction_id();

        // Construire le paquet de connexion
        let mut connect_packet = Vec::with_capacity(16);
        connect_packet.extend_from_slice(&PROTOCOL_ID.to_be_bytes());
        connect_packet.extend_from_slice(&CONNECT_ACTION.to_be_bytes());
        connect_packet.extend_from_slice(&transaction_id.to_be_bytes());

        // Envoyer et recevoir avec timeout
        self.socket
            .send_to(&connect_packet, self.tracker_addr)
            .await?;

        let mut buffer = [0u8; 16];
        let result = timeout(Duration::from_secs(15), self.socket.recv(&mut buffer)).await;

        match result {
            Ok(Ok(bytes_received)) => {
                if bytes_received < 16 {
                    return Err(TrackerError::InvalidResponse);
                }

                let action = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
                let received_transaction_id =
                    u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

                if action != CONNECT_ACTION || received_transaction_id != transaction_id {
                    return Err(TrackerError::InvalidResponse);
                }

                let connection_id = u64::from_be_bytes([
                    buffer[8], buffer[9], buffer[10], buffer[11], buffer[12], buffer[13],
                    buffer[14], buffer[15],
                ]);

                Ok(connection_id)
            }
            Ok(Err(e)) => Err(TrackerError::IoError(e)),
            Err(_) => Err(TrackerError::Timeout),
        }
    }

    async fn announce_with_connection_id(
        &self,
        connection_id: u64,
        request: &TrackerRequest,
    ) -> Result<TrackerResponse, TrackerError> {
        const ANNOUNCE_ACTION: u32 = 1;

        let transaction_id = self.generate_transaction_id();

        // Construire le paquet d'annonce
        let mut announce_packet = Vec::with_capacity(98);
        announce_packet.extend_from_slice(&connection_id.to_be_bytes());
        announce_packet.extend_from_slice(&ANNOUNCE_ACTION.to_be_bytes());
        announce_packet.extend_from_slice(&transaction_id.to_be_bytes());
        announce_packet.extend_from_slice(&request.info_hash);
        announce_packet.extend_from_slice(&request.peer_id);
        announce_packet.extend_from_slice(&request.downloaded.to_be_bytes());
        announce_packet.extend_from_slice(&request.left.to_be_bytes());
        announce_packet.extend_from_slice(&request.uploaded.to_be_bytes());
        // Event: 0=none, 1=completed, 2=started, 3=stopped (BEP 0015)
        let event_value: u32 = match request.event {
            Some(Event::Started) => 2,
            Some(Event::Stopped) => 3,
            None => 0,
        };
        announce_packet.extend_from_slice(&event_value.to_be_bytes());
        announce_packet.extend_from_slice(&0u32.to_be_bytes()); // IP address (0 = default)
        announce_packet.extend_from_slice(&request.key.to_be_bytes());
        announce_packet.extend_from_slice(&request.num_want.to_be_bytes());
        announce_packet.extend_from_slice(&request.port.to_be_bytes());

        // Envoyer et recevoir avec timeout
        self.socket
            .send_to(&announce_packet, self.tracker_addr)
            .await?;

        let mut buffer = [0u8; 1024];
        let result = timeout(Duration::from_secs(15), self.socket.recv(&mut buffer)).await;

        match result {
            Ok(Ok(bytes_received)) => {
                if bytes_received < 20 {
                    return Err(TrackerError::InvalidResponse);
                }

                let action = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]);
                let received_transaction_id =
                    u32::from_be_bytes([buffer[4], buffer[5], buffer[6], buffer[7]]);

                // Vérifier si c'est une erreur
                if action == 3 {
                    let error_msg = String::from_utf8_lossy(&buffer[8..bytes_received]);
                    return Err(TrackerError::TrackerError(error_msg.to_string()));
                }

                if action != ANNOUNCE_ACTION || received_transaction_id != transaction_id {
                    return Err(TrackerError::InvalidResponse);
                }

                if bytes_received < 20 {
                    return Err(TrackerError::InvalidResponse);
                }

                let interval = u32::from_be_bytes([buffer[8], buffer[9], buffer[10], buffer[11]]);
                let leechers = u32::from_be_bytes([buffer[12], buffer[13], buffer[14], buffer[15]]);
                let seeders = u32::from_be_bytes([buffer[16], buffer[17], buffer[18], buffer[19]]);

                // Parser les peers
                let mut peers = Vec::new();
                let peers_data = &buffer[20..bytes_received];

                if peers_data.len() % 6 != 0 {
                    return Err(TrackerError::InvalidResponse);
                }

                for chunk in peers_data.chunks_exact(6) {
                    let ip = std::net::Ipv4Addr::new(chunk[0], chunk[1], chunk[2], chunk[3]);
                    let port = u16::from_be_bytes([chunk[4], chunk[5]]);
                    peers.push(SocketAddr::new(ip.into(), port));
                }

                Ok(TrackerResponse {
                    interval,
                    leechers,
                    seeders,
                    peers,
                })
            }
            Ok(Err(e)) => Err(TrackerError::IoError(e)),
            Err(_) => Err(TrackerError::Timeout),
        }
    }

    fn generate_transaction_id(&self) -> u32 {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        (now.as_secs() as u32).wrapping_add(now.subsec_nanos())
    }
}

/// Parse a UDP tracker URL and resolve the hostname to a SocketAddr.
async fn resolve_tracker_addr(url: &str) -> Result<SocketAddr, TrackerError> {
    let parsed = Url::parse(url).map_err(|_| TrackerError::ParseError)?;

    if parsed.scheme() != "udp" {
        return Err(TrackerError::ParseError);
    }

    let host = parsed.host_str().ok_or(TrackerError::ParseError)?;
    let port = parsed.port().unwrap_or(80);

    // Resolve hostname to IP address using tokio's DNS resolution
    let addr_str = format!("{}:{}", host, port);
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host(&addr_str)
        .await
        .map_err(TrackerError::IoError)?
        .collect();

    // Prefer IPv4 address, fall back to any available
    addrs
        .iter()
        .find(|addr| addr.is_ipv4())
        .or_else(|| addrs.first())
        .copied()
        .ok_or_else(|| TrackerError::TrackerError(format!("Could not resolve hostname: {}", host)))
}

pub async fn announce_udp(url: &str, torrent: &mut Torrent, client: &Client, event: Option<Event>) {
    debug!("UDP announce to {}", url);

    // Resolve tracker address
    let tracker_addr = match resolve_tracker_addr(url).await {
        Ok(addr) => addr,
        Err(e) => {
            error!("Cannot resolve UDP tracker {}: {}", url, e);
            torrent.error_count += 1;
            return;
        }
    };

    // Create tracker connection
    let tracker = match UdpTracker::new(tracker_addr).await {
        Ok(t) => t,
        Err(e) => {
            error!("Cannot connect to UDP tracker {}: {}", url, e);
            torrent.error_count += 1;
            return;
        }
    };

    // Calculate uploaded bytes since last announce
    let elapsed: u64 = if event == Some(Event::Started) {
        0
    } else {
        torrent.last_announce.elapsed().as_secs()
    };
    let uploaded: u64 = torrent.next_upload_speed as u64 * elapsed;

    // Convert peer_id to fixed-size array
    let peer_id_bytes = client.peer_id.as_bytes();
    let peer_id_array: [u8; 20] = if peer_id_bytes.len() >= 20 {
        peer_id_bytes[..20].try_into().unwrap()
    } else {
        let mut arr = [0u8; 20];
        arr[..peer_id_bytes.len()].copy_from_slice(peer_id_bytes);
        arr
    };

    // Get config values
    let config = CONFIG.get().expect("Config not initialized");
    let port = config.port;
    let numwant = config.numwant.unwrap_or(80) as i32;

    let request = TrackerRequest {
        info_hash: torrent.info_hash,
        peer_id: peer_id_array,
        downloaded: 0,
        left: 0,
        uploaded,
        event,
        key: client.key,
        num_want: numwant,
        port,
    };

    match tracker.announce(&request).await {
        Ok(response) => {
            // Update torrent state on successful announce
            torrent.uploaded += uploaded;
            torrent.interval = response.interval as u64;
            torrent.seeders = response.seeders as u16;
            torrent.leechers = response.leechers as u16;
            torrent.last_announce = std::time::Instant::now();
            torrent.error_count = 0;

            info!(
                "UDP announce OK: interval={}, seeders={}, leechers={}, peers={}",
                response.interval,
                response.seeders,
                response.leechers,
                response.peers.len()
            );
        }
        Err(e) => {
            warn!("UDP announce failed for {}: {}", url, e);
            torrent.error_count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_tracker_request_creation() {
        let request = TrackerRequest {
            info_hash: [1u8; 20],
            peer_id: *b"TEST_PEER_ID_1234567",
            downloaded: 1024,
            left: 2048,
            uploaded: 512,
            event: Some(Event::Started),
            key: 42,
            num_want: 30,
            port: 6881,
        };

        assert_eq!(request.downloaded, 1024);
        assert_eq!(request.port, 6881);
    }

    #[test]
    fn test_transaction_id_generation() {
        let addr = "127.0.0.1:8080".parse().unwrap();
        let tracker = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(UdpTracker::new(addr))
            .unwrap();

        let id1 = tracker.generate_transaction_id();
        std::thread::sleep(Duration::from_millis(1));
        let id2 = tracker.generate_transaction_id();

        // IDs must be different
        assert_ne!(id1, id2);
    }

    #[tokio::test]
    async fn test_resolve_tracker_addr_valid() {
        // Test with a well-known DNS name that should resolve
        let result = resolve_tracker_addr("udp://localhost:6969/announce").await;
        assert!(result.is_ok());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 6969);
    }

    #[tokio::test]
    async fn test_resolve_tracker_addr_invalid_scheme() {
        let result = resolve_tracker_addr("http://tracker.example.com:6969/announce").await;
        assert!(matches!(result, Err(TrackerError::ParseError)));
    }

    #[tokio::test]
    async fn test_resolve_tracker_addr_no_port() {
        // Default port should be 80
        let result = resolve_tracker_addr("udp://localhost/announce").await;
        assert!(result.is_ok());
        let addr = result.unwrap();
        assert_eq!(addr.port(), 80);
    }

    #[test]
    fn test_announce_packet_structure() {
        // Test that the announce packet has the correct size (98 bytes)
        let connection_id: u64 = 0x41727101980;
        let transaction_id: u32 = 12345;
        let request = TrackerRequest {
            info_hash: [0u8; 20],
            peer_id: [0u8; 20],
            downloaded: 0,
            left: 0,
            uploaded: 0,
            event: None,
            key: 0,
            num_want: 50,
            port: 6881,
        };

        let mut packet = Vec::with_capacity(98);
        packet.extend_from_slice(&connection_id.to_be_bytes()); // 8 bytes
        packet.extend_from_slice(&1u32.to_be_bytes()); // action: 4 bytes
        packet.extend_from_slice(&transaction_id.to_be_bytes()); // 4 bytes
        packet.extend_from_slice(&request.info_hash); // 20 bytes
        packet.extend_from_slice(&request.peer_id); // 20 bytes
        packet.extend_from_slice(&request.downloaded.to_be_bytes()); // 8 bytes
        packet.extend_from_slice(&request.left.to_be_bytes()); // 8 bytes
        packet.extend_from_slice(&request.uploaded.to_be_bytes()); // 8 bytes
        let event_value: u32 = match request.event {
            Some(Event::Started) => 2,
            Some(Event::Stopped) => 3,
            None => 0,
        };
        packet.extend_from_slice(&event_value.to_be_bytes()); // 4 bytes
        packet.extend_from_slice(&0u32.to_be_bytes()); // IP: 4 bytes
        packet.extend_from_slice(&request.key.to_be_bytes()); // 4 bytes
        packet.extend_from_slice(&request.num_want.to_be_bytes()); // 4 bytes
        packet.extend_from_slice(&request.port.to_be_bytes()); // 2 bytes

        assert_eq!(packet.len(), 98);
    }
}
