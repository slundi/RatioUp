//! Tracker-facing types shared by the HTTP and UDP transports, and the
//! [`TrackerTransport`] abstraction the announce service depends on.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use fake_torrent_client::Client;
use tracing::{error, warn};
use url::{Host, Url};

use crate::infra::http::HttpTransport;
use crate::infra::udp::UdpTransport;

/// Number of peers requested when `numwant` is not configured.
pub const DEFAULT_NUMWANT: u16 = 80;

// pub fn print_request_error(code: u16) {
//     match code {
//         100 => error!("100 Invalid request, not a GET"),
//         101 => error!("101 Info hash is missing"),
//         102 => error!("102 Peer ID is missing"),
//         103 => error!("103 Port is missing"),
//         150 => error!("150 Info hash is not 20 bytes long"),
//         151 => error!("151 Invalid peer ID"),
//         152 => error!("152 Invalid numwant: requested more peers than allowed by tracker"),
//         // Sent only by trackers that do not automatically include new hashes into the database.
//         200 => error!("200 info_hash not found in the database"),
//         500 => error!("500 Client sent an eventless request before the specified time"),
//         900 => error!("500 Generic error"),
//         _ => warn!("Unknown error code: {code}"),
//     }
// }

/// The optional announce event.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// The first request to tracker must include this value.
    Started = 2,
    // /// Must be sent to the tracker when the client becomes a seeder. Must not be
    // /// present if the client started as a seeder.
    // Completed,
    /// Must be sent to tracker if the client is shutting down gracefully.
    Stopped,
}

/// Check if the tracker URL is supported.
/// Supports HTTP, HTTPS, and UDP schemes.
/// Rejects .local TLDs (mDNS).
pub fn is_supported_url(url_str: &str) -> bool {
    let parsed_url = match Url::parse(url_str) {
        Ok(url) => url,
        Err(error) => {
            error!("Unable to parse URL: {url_str} {error}");
            return false;
        }
    };

    let host = match parsed_url.host() {
        Some(host) => host,
        None => {
            error!("No host in tracker URL: {url_str}");
            return false;
        }
    };

    // Check supported schemes
    let scheme = parsed_url.scheme();
    if scheme != "http" && scheme != "https" && scheme != "udp" {
        warn!("Unsupported tracker scheme: {}", scheme);
        return false;
    }

    // UDP has no standard default port; a missing port will cause the announce to
    // silently connect to the wrong destination, so reject early.
    if scheme == "udp" && parsed_url.port().is_none() {
        warn!("UDP tracker URL has no explicit port, rejecting: {url_str}");
        return false;
    }

    match host {
        Host::Domain(domain_str) => {
            // For ".local", a simple split is sufficient, as ".local" is not a "public" TLD managed by the public
            // suffix list, but a pseudo-TLD for mDNS.
            let parts: Vec<&str> = domain_str.split('.').collect();
            if let Some(tld_candidate) = parts.last() {
                *tld_candidate != "local"
            } else {
                // no dot in domain, ex: "localhost" or just "myserver"
                warn!("Skipping, no dot in domain: {url_str}");
                false
            }
        }
        // IP addresses are supported
        Host::Ipv4(_) | Host::Ipv6(_) => true,
    }
}

/// What is sent to a tracker, independently of the protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnnounceRequest {
    pub info_hash: [u8; 20],
    /// URL-encoded info hash, used by HTTP trackers.
    pub info_hash_urlencoded: String,
    /// Bytes reported as uploaded since the last announce.
    pub uploaded: u64,
    pub event: Option<Event>,
    pub port: u16,
    pub numwant: u16,
}

/// What a tracker answered. Fields a protocol does not provide are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnnounceResponse {
    pub interval: Option<u64>,
    pub min_interval: Option<u64>,
    pub tracker_id: Option<String>,
    pub seeders: Option<u16>,
    pub leechers: Option<u16>,
    pub warning: Option<String>,
}

/// Why an announce failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnnounceError {
    /// The tracker answered but refused the announce (failure reason).
    Rejected(String),
    /// The tracker could not be reached or its answer could not be understood.
    Transport(String),
}

impl fmt::Display for AnnounceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnnounceError::Rejected(reason) => write!(formatter, "rejected by tracker: {reason}"),
            AnnounceError::Transport(reason) => write!(formatter, "transport error: {reason}"),
        }
    }
}

impl std::error::Error for AnnounceError {}

/// Boxed future returned by [`TrackerTransport::announce`], so the trait stays object safe.
pub type AnnounceFuture<'a> =
    Pin<Box<dyn Future<Output = Result<AnnounceResponse, AnnounceError>> + Send + 'a>>;

/// Sends one announce to one tracker URL.
pub trait TrackerTransport: Send + Sync + fmt::Debug {
    fn announce<'a>(
        &'a self,
        url: &'a str,
        request: &'a AnnounceRequest,
        client: &'a Client,
    ) -> AnnounceFuture<'a>;
}

/// Production transport: dispatches on the URL scheme (`udp://` or HTTP(S)).
#[derive(Debug)]
pub struct NetworkTransport {
    http: HttpTransport,
    udp: UdpTransport,
}

impl NetworkTransport {
    pub fn new(http: HttpTransport, udp: UdpTransport) -> Self {
        Self { http, udp }
    }
}

impl TrackerTransport for NetworkTransport {
    fn announce<'a>(
        &'a self,
        url: &'a str,
        request: &'a AnnounceRequest,
        client: &'a Client,
    ) -> AnnounceFuture<'a> {
        if url.to_lowercase().starts_with("udp://") {
            self.udp.announce(url, request, client)
        } else {
            self.http.announce(url, request, client)
        }
    }
}

/// Test double recording every announce and replaying scripted answers.
#[cfg(test)]
pub mod testing {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// One recorded announce: the URL and the request sent to it.
    pub type Call = (String, AnnounceRequest);

    #[derive(Debug, Default)]
    pub struct FakeTransport {
        answers: Mutex<VecDeque<Result<AnnounceResponse, AnnounceError>>>,
        calls: Arc<Mutex<Vec<Call>>>,
    }

    impl FakeTransport {
        /// Queue the answer of the next announce. Once the queue is empty,
        /// announces succeed with an empty response.
        pub fn answer(self, result: Result<AnnounceResponse, AnnounceError>) -> Self {
            self.answers.lock().unwrap().push_back(result);
            self
        }

        /// Shared handle on the recorded calls, usable after the fake is moved into a context.
        pub fn calls(&self) -> Arc<Mutex<Vec<Call>>> {
            Arc::clone(&self.calls)
        }
    }

    impl TrackerTransport for FakeTransport {
        fn announce<'a>(
            &'a self,
            url: &'a str,
            request: &'a AnnounceRequest,
            _client: &'a Client,
        ) -> AnnounceFuture<'a> {
            self.calls
                .lock()
                .unwrap()
                .push((url.to_string(), request.clone()));
            let answer = self
                .answers
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(AnnounceResponse::default()));
            Box::pin(async move { answer })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_url() {
        // HTTP and HTTPS
        assert!(is_supported_url("http://localhost/?param=test"));
        assert!(is_supported_url("https://localhost/?param=test"));
        assert!(is_supported_url("http://another-host/?param=test"));
        assert!(is_supported_url("http://some-host.tld/?param=test"));
        assert!(is_supported_url("https://some-host.tld/?param=test"));

        // UDP is now supported
        assert!(is_supported_url("udp://tracker.example.com:1337/announce"));
        assert!(is_supported_url("udp://udp-host.tld:6969/announce"));

        // .local TLD should be rejected
        assert!(!is_supported_url("http://myserver.local/announce"));
        assert!(!is_supported_url("udp://tracker.local:6969/announce"));

        // IP addresses are supported
        assert!(is_supported_url("http://192.168.1.1:8080/announce"));
        assert!(is_supported_url("udp://192.168.1.1:6969/announce"));

        // Unsupported schemes
        assert!(!is_supported_url("wss://tracker.example.com/announce"));

        // UDP without explicit port — silently using port 80 would connect to the wrong host
        assert!(!is_supported_url("udp://tracker.example.com/announce"));
        assert!(!is_supported_url("udp://192.168.1.1/announce"));

        // Malformed / unparsable
        assert!(!is_supported_url("not-a-url"));
        assert!(!is_supported_url(""));
    }

    #[test]
    fn test_announce_error_display() {
        assert_eq!(
            AnnounceError::Rejected(String::from("unregistered torrent")).to_string(),
            "rejected by tracker: unregistered torrent"
        );
        assert_eq!(
            AnnounceError::Transport(String::from("timeout")).to_string(),
            "transport error: timeout"
        );
    }

    #[tokio::test]
    async fn test_network_transport_dispatches_on_scheme() {
        let client = Client::default();
        let transport =
            NetworkTransport::new(HttpTransport::new("test-agent"), UdpTransport::default());
        let request = AnnounceRequest {
            info_hash: [1u8; 20],
            info_hash_urlencoded: String::new(),
            uploaded: 0,
            event: None,
            port: 6881,
            numwant: 50,
        };

        // UDP (any case): the UDP transport rejects a URL without port before any I/O.
        for url in [
            "udp://tracker.example.com/announce",
            "UDP://tracker.example.com/announce",
        ] {
            match transport.announce(url, &request, &client).await {
                Err(AnnounceError::Transport(reason)) => {
                    assert!(reason.contains("parse tracker URL"), "{url}: {reason}");
                }
                other => panic!("{url}: unexpected {other:?}"),
            }
        }

        // HTTP: the HTTP transport reports the connection failure instead.
        let address = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let url = format!("http://{address}/announce");
        match transport.announce(&url, &request, &client).await {
            Err(AnnounceError::Transport(reason)) => {
                assert!(!reason.contains("parse tracker URL"), "{url}: {reason}");
            }
            other => panic!("{url}: unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_fake_transport_records_calls_and_replays_answers() {
        let fake = testing::FakeTransport::default()
            .answer(Err(AnnounceError::Rejected(String::from("no"))));
        let calls = fake.calls();
        let client = Client::default();
        let request = AnnounceRequest {
            info_hash: [1u8; 20],
            info_hash_urlencoded: String::new(),
            uploaded: 0,
            event: None,
            port: 6881,
            numwant: 50,
        };

        let first = fake.announce("http://a/announce", &request, &client).await;
        let second = fake.announce("udp://b:1/announce", &request, &client).await;
        assert_eq!(first, Err(AnnounceError::Rejected(String::from("no"))));
        assert_eq!(second, Ok(AnnounceResponse::default()));
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1].0, "udp://b:1/announce");
    }
}
