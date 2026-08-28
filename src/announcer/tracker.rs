use crate::bencode::{BencodeDecoder, BencodeValue};
use crate::torrent::Torrent;
use crate::{CLIENT, CONFIG, TORRENTS};
use fake_torrent_client::Client;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use url::{Host, Url};

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The first request to tracker must include this value.
    Started = 2,
    // /// Must be sent to the tracker when the client becomes a seeder. Must not be
    // /// present if the client started as a seeder.
    // Completed,
    /// Must be sent to tracker if the client is shutting down gracefully.
    Stopped,
}

pub async fn announce_started() -> u64 {
    info!("Announcing torrent(s) with STARTED event");
    let keys: Vec<[u8; 20]> = TORRENTS.iter().map(|e| *e.key()).collect();
    let mut wait_time = u64::MAX;
    for key in keys {
        if let Some(entry) = TORRENTS.get(&key) {
            let arc = Arc::clone(entry.value());
            drop(entry);
            let mut t = arc.lock().await;
            announce(&mut t, Some(Event::Started)).await;
            wait_time = wait_time.min(t.interval);
            debug!("Time: {}", wait_time);
        }
    }
    wait_time
}

pub async fn announce_stopped() {
    info!("Announcing torrent(s) with STOPPED event");
    let keys: Vec<[u8; 20]> = TORRENTS.iter().map(|e| *e.key()).collect();
    let mut total_uploaded: u64 = 0;
    for key in keys {
        if let Some(entry) = TORRENTS.get(&key) {
            let arc = Arc::clone(entry.value());
            drop(entry);
            let mut t = arc.lock().await;
            announce(&mut t, Some(Event::Stopped)).await;
            total_uploaded += t.uploaded;
            info!(
                "Torrent \"{}\": uploaded={}, seeders={}, leechers={}, errors={}",
                t.name,
                crate::utils::format_bytes_u64(t.uploaded),
                t.seeders,
                t.leechers,
                t.error_count
            );
        }
    }
    info!(
        "Session total: {} torrents, uploaded={}",
        TORRENTS.len(),
        crate::utils::format_bytes_u64(total_uploaded)
    );
}

/// Check if the tracker URL is supported.
/// Supports HTTP, HTTPS, and UDP schemes.
/// Rejects .local TLDs (mDNS).
pub fn is_supported_url(url_str: &str) -> bool {
    let parsed_url = match Url::parse(url_str) {
        Ok(url) => url,
        Err(e) => {
            error!("Unable to parse URL: {url_str} {e}");
            return false;
        }
    };

    let host = match parsed_url.host() {
        Some(h) => h,
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

/// Sends an announce request to the tracker with the specified parameters.
///
/// This may be used by a torrent to request peers to download from and to
/// report statistics to the tracker.
///
/// # Important
///
/// The tracker may not be contacted more often than the minimum interval
/// returned in the first announce response.
pub async fn announce(torrent: &mut Torrent, event: Option<Event>) {
    // TODO: prepare announce (uploaded and downloaded if applicable)
    torrent.compute_speeds();
    if let Some(client) = &*CLIENT.read().await {
        debug!("Torrent has {} url(s)", torrent.urls.len());
        for url in torrent.urls.clone() {
            debug!("\t{}", url);
            if url.to_lowercase().starts_with("udp://") {
                crate::announcer::udp::announce_udp(&url, torrent, client, event).await;
            } else {
                announce_http(&url, torrent, client, event).await;
            }
        }
        info!(
            "Announced: interval={}, event={:?}, downloaded=0, uploaded={}, seeders={}, leechers={}, torrent={}",
            torrent.interval,
            event,
            torrent.uploaded,
            torrent.seeders,
            torrent.leechers,
            torrent.name
        );
    }
}

// /// Check which torrents need to be announced and call the announce function when applicable
// pub fn check_and_announce() {
//     let list = TORRENTS.read().expect("Cannot get torrent list");
//     for m in list.iter() {
//         let mut t = m.lock().unwrap();
//         if t.should_announce() {
//             announce(&mut t, None);
//         }
//     }
// }

/// Parsed fields from a tracker HTTP response (BEP 3).
#[derive(Debug, Default)]
struct TrackerHttpUpdate {
    failure_reason: Option<String>,
    warning_message: Option<String>,
    interval: Option<u64>,
    min_interval: Option<u64>,
    tracker_id: Option<String>,
    seeders: Option<u16>,
    leechers: Option<u16>,
}

/// Decode a bencoded tracker HTTP response into a `TrackerHttpUpdate`.
/// Returns `Err` when the bytes cannot be decoded or are not a dictionary.
fn parse_http_response(bytes: &[u8]) -> Result<TrackerHttpUpdate, String> {
    let mut decoder = BencodeDecoder::new(bytes);
    match decoder.decode() {
        Ok(BencodeValue::Dictionary(dict)) => {
            let mut update = TrackerHttpUpdate::default();

            if let Some(BencodeValue::ByteString(msg)) = dict.get(b"failure reason".as_ref()) {
                update.failure_reason = Some(
                    std::str::from_utf8(msg)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|_| format!("{msg:?}")),
                );
                return Ok(update);
            }

            if let Some(BencodeValue::ByteString(msg)) = dict.get(b"warning message".as_ref()) {
                update.warning_message = std::str::from_utf8(msg).ok().map(|s| s.to_string());
            }

            if let Some(BencodeValue::Integer(interval)) = dict.get(b"interval".as_ref()) {
                update.interval = Some(*interval as u64);
            }

            if let Some(BencodeValue::Integer(mi)) = dict.get(b"min interval".as_ref()) {
                update.min_interval = Some(*mi as u64);
            }

            if let Some(BencodeValue::ByteString(tid)) = dict.get(b"tracker_id".as_ref())
                && let Ok(s) = std::str::from_utf8(tid)
            {
                update.tracker_id = Some(s.to_string());
            }

            if let Some(BencodeValue::Integer(value)) = dict.get(b"complete".as_ref()) {
                update.seeders = Some(*value as u16);
            }

            if let Some(BencodeValue::Integer(value)) = dict.get(b"incomplete".as_ref()) {
                update.leechers = Some(*value as u16);
            }

            Ok(update)
        }
        Ok(_) => Err("response is not a dictionary".to_string()),
        Err(e) => Err(format!("{e:?}")),
    }
}

async fn announce_http(
    url: &str,
    torrent: &mut Torrent,
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

    let reqwest_client = crate::HTTP_CLIENT
        .get()
        .expect("HTTP client not initialized");

    let (_, headers_to_set) = client.get_query();
    let built_url = build_url(url, torrent, event, client);
    info!("Announce HTTP URL {built_url}");

    let mut request_builder = reqwest_client.get(&built_url);

    for (name, value) in headers_to_set {
        request_builder = request_builder.header(&name, &value);
    }

    match request_builder.send().await {
        Ok(resp) => {
            let status = resp.status().as_u16();
            info!(
                "\tTime since last announce: {}s \t interval: {}",
                torrent.last_announce.elapsed().as_secs(),
                torrent.interval
            );

            // read response body
            let bytes = match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    error!("Failed to read response bytes: {:?}", e);
                    return torrent.interval; // return current interval
                }
            };
            let bytes_vec = bytes.to_vec(); //convert Bytes to Vec<u8>

            // Bencode decoding
            debug!(
                "Tracker response: {:?}",
                String::from_utf8_lossy(&bytes_vec)
            );
            match parse_http_response(&bytes_vec) {
                Ok(update) => {
                    if let Some(reason) = update.failure_reason {
                        error!("Cannot announce: {:?}", reason);
                        torrent.error_count += 1;
                    } else {
                        if let Some(msg) = update.warning_message {
                            warn!("Announce with warning: {:?}", msg);
                        }
                        if let Some(interval) = update.interval {
                            torrent.interval = interval;
                        }
                        if let Some(mi) = update.min_interval {
                            torrent.min_interval = Some(mi);
                        }
                        if let Some(tracker_id) = update.tracker_id {
                            torrent.tracker_id = Some(tracker_id);
                        }
                        if let Some(seeders) = update.seeders {
                            torrent.seeders = seeders;
                        }
                        if let Some(leechers) = update.leechers {
                            torrent.leechers = leechers;
                        }
                        torrent.last_announce = std::time::Instant::now();
                        torrent.error_count = 0;
                    }
                }
                Err(e) => error!("Bad response with HTTP status {status}: {:?}", e),
            }
        }
        Err(err) => error!("Cannot announce: {:?}", err),
    }
    if let Some(min) = torrent.min_interval
        && min > torrent.interval
    {
        return min;
    }
    torrent.interval
}

/// Build the HTTP announce URLs for the listed trackers in the torrent file.
/// It prepares the announce query by replacing variables (port, numwant, ...) with the computed values
pub fn build_url(
    url: &str,
    torrent: &mut Torrent,
    event: Option<Event>,
    client: &Client,
) -> String {
    info!("Torrent {:?}: {}", event, torrent.name);
    //compute downloads and uploads
    let elapsed: u64 = if event == Some(Event::Started) {
        0
    } else {
        torrent.last_announce.elapsed().as_secs()
    };
    let uploaded: u64 = torrent.next_upload_speed as u64 * elapsed;

    let mut port = 55555u16;
    let mut numwant = 80u16;
    if let Some(config) = CONFIG.get() {
        port = config.port;
        if let Some(nw) = config.numwant {
            numwant = nw;
        }
    }
    let mut result = String::from(url);
    result.push(if result.contains('?') { '&' } else { '?' });
    result.push_str(&client.query);
    let result = result
        .replace("{infohash}", &torrent.info_hash_urlencoded)
        .replace("{key}", &client.key.to_string())
        .replace("{uploaded}", uploaded.to_string().as_str())
        .replace("{downloaded}", "0")
        .replace("{peerid}", &client.peer_id)
        .replace("{port}", &port.to_string())
        .replace("{numwant}", &numwant.to_string())
        .replace("ipv6={ipv6}", "")
        .replace("{left}", "0")
        .replace(
            "{event}",
            match event {
                Some(e) => match e {
                    Event::Started => "started",
                    // Event::Completed => "completed",
                    Event::Stopped => "stopped",
                },
                None => "",
            },
        );
    // info!(
    //     "\tUploaded: {}",
    //     byte_unit::Byte::from_u128(uploaded as u128)
    //         .unwrap()
    //         .get_appropriate_unit(byte_unit::UnitType::Decimal)
    //         .to_string()
    // );
    info!("\tAnnounce at: {}", url);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::torrent::Torrent;

    fn make_torrent() -> Torrent {
        Torrent {
            name: String::from("test"),
            urls: vec![String::from("http://tracker.example.com/announce")],
            length: 1024,
            private: false,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: [0u8; 20],
            info_hash_urlencoded: String::new(),
            seeders: 0,
            leechers: 0,
            next_upload_speed: 1024,
            interval: 1800,
            error_count: 0,
            encoding: None,
            min_interval: None,
            tracker_id: None,
            source_path: None,
        }
    }

    fn make_client() -> fake_torrent_client::Client {
        use std::str::FromStr;
        let mut client = fake_torrent_client::Client::default();
        client.build(
            fake_torrent_client::clients::ClientVersion::from_str("Transmission_3_00").unwrap(),
        );
        client
    }

    #[test]
    pub fn test_supported_url() {
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

    // --- parse_http_response ---

    #[test]
    fn test_parse_http_response_normal() {
        // interval=1800, min interval=900, complete=10, incomplete=5
        let bytes = b"d8:completei10e10:incompletei5e8:intervali1800e12:min intervali900ee";
        let update = parse_http_response(bytes).unwrap();
        assert_eq!(update.interval, Some(1800));
        assert_eq!(update.min_interval, Some(900));
        assert_eq!(update.seeders, Some(10));
        assert_eq!(update.leechers, Some(5));
        assert!(update.failure_reason.is_none());
        assert!(update.warning_message.is_none());
    }

    #[test]
    fn test_parse_http_response_failure_reason() {
        let bytes = b"d14:failure reason12:invalid hashe";
        let update = parse_http_response(bytes).unwrap();
        assert_eq!(update.failure_reason.as_deref(), Some("invalid hash"));
        // All other fields must remain None (early return on failure reason)
        assert!(update.interval.is_none());
    }

    #[test]
    fn test_parse_http_response_warning_message() {
        let bytes = b"d8:intervali1800e15:warning message9:slow downe";
        let update = parse_http_response(bytes).unwrap();
        assert!(update.warning_message.is_some());
        assert!(update.failure_reason.is_none());
        assert_eq!(update.interval, Some(1800));
    }

    #[test]
    fn test_parse_http_response_tracker_id() {
        let bytes = b"d8:intervali1800e10:tracker id6:abc123e";
        let update = parse_http_response(bytes).unwrap();
        assert_eq!(update.interval, Some(1800));
        // tracker_id key is "tracker_id" (underscore), not "tracker id" (space)
        assert!(update.tracker_id.is_none());
    }

    #[test]
    fn test_parse_http_response_non_dict() {
        let result = parse_http_response(b"i42e");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_http_response_invalid_bencode() {
        let result = parse_http_response(b"not bencode");
        assert!(result.is_err());
    }

    // --- build_url ---

    #[test]
    fn test_build_url_separator_without_query() {
        let client = make_client();
        let mut torrent = make_torrent();
        let url = build_url(
            "http://tracker.example.com/announce",
            &mut torrent,
            None,
            &client,
        );
        assert!(
            url.starts_with("http://tracker.example.com/announce?"),
            "expected '?' separator, got: {url}"
        );
    }

    #[test]
    fn test_build_url_separator_with_existing_query() {
        let client = make_client();
        let mut torrent = make_torrent();
        let url = build_url(
            "http://tracker.example.com/announce?passkey=secret",
            &mut torrent,
            None,
            &client,
        );
        assert!(
            url.starts_with("http://tracker.example.com/announce?passkey=secret&"),
            "expected '&' separator, got: {url}"
        );
    }

    #[test]
    fn test_build_url_started_event_uploaded_zero() {
        // For Event::Started, elapsed is forced to 0, so uploaded = speed * 0 = 0
        let client = make_client();
        let mut torrent = make_torrent();
        let url = build_url(
            "http://tracker.example.com/announce",
            &mut torrent,
            Some(Event::Started),
            &client,
        );
        assert!(
            url.contains("event=started"),
            "expected event=started in: {url}"
        );
        assert!(
            url.contains("uploaded=0"),
            "started event must report uploaded=0 in: {url}"
        );
    }

    #[test]
    fn test_build_url_stopped_event() {
        let client = make_client();
        let mut torrent = make_torrent();
        let url = build_url(
            "http://tracker.example.com/announce",
            &mut torrent,
            Some(Event::Stopped),
            &client,
        );
        assert!(
            url.contains("event=stopped"),
            "expected event=stopped in: {url}"
        );
    }

    #[test]
    fn test_build_url_no_placeholders_remain() {
        // After all replacements, no {…} tokens should remain
        let client = make_client();
        let mut torrent = make_torrent();
        let url = build_url(
            "http://tracker.example.com/announce",
            &mut torrent,
            Some(Event::Started),
            &client,
        );
        assert!(!url.contains('{'), "unreplaced placeholder in: {url}");
        assert!(!url.contains('}'), "unreplaced placeholder in: {url}");
    }
}
