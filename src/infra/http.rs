//! HTTP(S) tracker transport (BEP 3).

use fake_torrent_client::Client;
use tracing::{debug, info};

use crate::bencode::{BencodeDecoder, BencodeValue};
use crate::infra::tracker::{
    AnnounceError, AnnounceFuture, AnnounceRequest, AnnounceResponse, Event, TrackerTransport,
};

/// Timeout of one HTTP announce.
const HTTP_TIMEOUT_SECS: u64 = 60;

/// Announces over HTTP(S) with the user agent of the emulated client.
#[derive(Debug, Clone)]
pub struct HttpTransport {
    http: reqwest::Client,
}

impl HttpTransport {
    pub fn new(user_agent: &str) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .expect("Failed to build HTTP client");
        Self { http }
    }

    async fn send(
        &self,
        url: &str,
        request: &AnnounceRequest,
        client: &Client,
    ) -> Result<AnnounceResponse, AnnounceError> {
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

        let (_, headers_to_set) = client.get_query();
        let built_url = build_url(url, request, client);
        info!("Announce HTTP URL {built_url}");

        let mut request_builder = self.http.get(&built_url);
        for (name, value) in headers_to_set {
            request_builder = request_builder.header(&name, &value);
        }

        let response = request_builder
            .send()
            .await
            .map_err(|error| AnnounceError::Transport(error.to_string()))?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|error| {
            AnnounceError::Transport(format!("cannot read response body: {error}"))
        })?;
        debug!("Tracker response: {:?}", String::from_utf8_lossy(&bytes));

        parse_http_response(&bytes).map_err(|error| match error {
            AnnounceError::Transport(reason) => AnnounceError::Transport(format!(
                "bad response with HTTP status {status}: {reason}"
            )),
            rejected => rejected,
        })
    }
}

impl TrackerTransport for HttpTransport {
    fn announce<'a>(
        &'a self,
        url: &'a str,
        request: &'a AnnounceRequest,
        client: &'a Client,
    ) -> AnnounceFuture<'a> {
        Box::pin(self.send(url, request, client))
    }
}

/// Decode a bencoded tracker HTTP response.
///
/// A `failure reason` is reported as [`AnnounceError::Rejected`]; undecodable
/// bytes or a non-dictionary as [`AnnounceError::Transport`].
fn parse_http_response(bytes: &[u8]) -> Result<AnnounceResponse, AnnounceError> {
    let mut decoder = BencodeDecoder::new(bytes);
    let dict = match decoder.decode() {
        Ok(BencodeValue::Dictionary(dict)) => dict,
        Ok(_) => {
            return Err(AnnounceError::Transport(String::from(
                "response is not a dictionary",
            )));
        }
        Err(error) => return Err(AnnounceError::Transport(format!("{error:?}"))),
    };

    if let Some(BencodeValue::ByteString(msg)) = dict.get(b"failure reason".as_ref()) {
        let reason = std::str::from_utf8(msg)
            .map(str::to_string)
            .unwrap_or_else(|_| format!("{msg:?}"));
        return Err(AnnounceError::Rejected(reason));
    }

    let mut response = AnnounceResponse::default();
    if let Some(BencodeValue::ByteString(msg)) = dict.get(b"warning message".as_ref()) {
        response.warning = std::str::from_utf8(msg).ok().map(str::to_string);
    }
    if let Some(BencodeValue::Integer(interval)) = dict.get(b"interval".as_ref()) {
        response.interval = Some(*interval as u64);
    }
    if let Some(BencodeValue::Integer(min_interval)) = dict.get(b"min interval".as_ref()) {
        response.min_interval = Some(*min_interval as u64);
    }
    if let Some(BencodeValue::ByteString(tracker_id)) = dict.get(b"tracker_id".as_ref())
        && let Ok(text) = std::str::from_utf8(tracker_id)
    {
        response.tracker_id = Some(text.to_string());
    }
    if let Some(BencodeValue::Integer(value)) = dict.get(b"complete".as_ref()) {
        response.seeders = Some(*value as u16);
    }
    if let Some(BencodeValue::Integer(value)) = dict.get(b"incomplete".as_ref()) {
        response.leechers = Some(*value as u16);
    }
    Ok(response)
}

/// Build the HTTP announce URL: append the emulated client's query template to
/// the tracker URL and fill in its variables (port, numwant, ...).
pub fn build_url(url: &str, request: &AnnounceRequest, client: &Client) -> String {
    let mut result = String::from(url);
    result.push(if result.contains('?') { '&' } else { '?' });
    result.push_str(&client.query);
    result
        .replace("{infohash}", &request.info_hash_urlencoded)
        .replace("{key}", &client.key.to_string())
        .replace("{uploaded}", &request.uploaded.to_string())
        .replace("{downloaded}", "0")
        .replace("{peerid}", &client.peer_id)
        .replace("{port}", &request.port.to_string())
        .replace("{numwant}", &request.numwant.to_string())
        .replace("ipv6={ipv6}", "")
        .replace("{left}", "0")
        .replace(
            "{event}",
            match request.event {
                Some(Event::Started) => "started",
                // Some(Event::Completed) => "completed",
                Some(Event::Stopped) => "stopped",
                None => "",
            },
        )
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::task::JoinHandle;

    use super::*;

    const TEST_PORT: u16 = 51413;
    const TEST_NUMWANT: u16 = 42;

    fn make_request(uploaded: u64, event: Option<Event>) -> AnnounceRequest {
        AnnounceRequest {
            info_hash: [0u8; 20],
            info_hash_urlencoded: String::from("%AB%CD"),
            uploaded,
            event,
            port: TEST_PORT,
            numwant: TEST_NUMWANT,
        }
    }

    fn make_client() -> Client {
        use std::str::FromStr;
        let mut client = Client::default();
        client.build(
            fake_torrent_client::clients::ClientVersion::from_str("Transmission_3_00").unwrap(),
        );
        client
    }

    /// Serve exactly one HTTP request answering `body`.
    /// Returns the announce URL and a handle yielding the raw request received.
    async fn serve_once(body: &'static [u8]) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let handle = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 4096];
            let mut received = Vec::new();
            loop {
                let read = stream.read(&mut buffer).await.unwrap();
                received.extend_from_slice(&buffer[..read]);
                if read == 0 || received.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
            stream.shutdown().await.ok();
            String::from_utf8_lossy(&received).into_owned()
        });
        (format!("http://{address}/announce"), handle)
    }

    // --- parse_http_response ---

    #[test]
    fn test_parse_http_response_normal() {
        // interval=1800, min interval=900, complete=10, incomplete=5
        let bytes = b"d8:completei10e10:incompletei5e8:intervali1800e12:min intervali900ee";
        let response = parse_http_response(bytes).unwrap();
        assert_eq!(response.interval, Some(1800));
        assert_eq!(response.min_interval, Some(900));
        assert_eq!(response.seeders, Some(10));
        assert_eq!(response.leechers, Some(5));
        assert!(response.warning.is_none());
    }

    #[test]
    fn test_parse_http_response_failure_reason_is_rejected() {
        let bytes = b"d14:failure reason12:invalid hash8:intervali1800ee";
        assert_eq!(
            parse_http_response(bytes),
            Err(AnnounceError::Rejected(String::from("invalid hash")))
        );
    }

    #[test]
    fn test_parse_http_response_warning_message() {
        let bytes = b"d8:intervali1800e15:warning message9:slow downe";
        let response = parse_http_response(bytes).unwrap();
        assert_eq!(response.warning.as_deref(), Some("slow down"));
        assert_eq!(response.interval, Some(1800));
    }

    #[test]
    fn test_parse_http_response_tracker_id() {
        // tracker_id key is "tracker_id" (underscore), not "tracker id" (space)
        let spaced = parse_http_response(b"d8:intervali1800e10:tracker id6:abc123e").unwrap();
        assert!(spaced.tracker_id.is_none());
        let underscored = parse_http_response(b"d8:intervali1800e10:tracker_id6:abc123e").unwrap();
        assert_eq!(underscored.tracker_id.as_deref(), Some("abc123"));
    }

    #[test]
    fn test_parse_http_response_non_dict() {
        assert!(matches!(
            parse_http_response(b"i42e"),
            Err(AnnounceError::Transport(_))
        ));
    }

    #[test]
    fn test_parse_http_response_invalid_bencode() {
        assert!(matches!(
            parse_http_response(b"not bencode"),
            Err(AnnounceError::Transport(_))
        ));
    }

    // --- build_url ---

    #[test]
    fn test_build_url_separator_without_query() {
        let url = build_url(
            "http://tracker.example.com/announce",
            &make_request(0, None),
            &make_client(),
        );
        assert!(
            url.starts_with("http://tracker.example.com/announce?"),
            "expected '?' separator, got: {url}"
        );
    }

    #[test]
    fn test_build_url_separator_with_existing_query() {
        let url = build_url(
            "http://tracker.example.com/announce?passkey=secret",
            &make_request(0, None),
            &make_client(),
        );
        assert!(
            url.starts_with("http://tracker.example.com/announce?passkey=secret&"),
            "expected '&' separator, got: {url}"
        );
    }

    #[test]
    fn test_build_url_fills_request_values() {
        let url = build_url(
            "http://tracker.example.com/announce",
            &make_request(12345, Some(Event::Started)),
            &make_client(),
        );
        assert!(url.contains("event=started"), "event in: {url}");
        assert!(url.contains("uploaded=12345"), "uploaded in: {url}");
        assert!(url.contains("port=51413"), "port in: {url}");
        assert!(url.contains("numwant=42"), "numwant in: {url}");
        assert!(url.contains("info_hash=%AB%CD"), "info hash in: {url}");
    }

    #[test]
    fn test_build_url_stopped_event() {
        let url = build_url(
            "http://tracker.example.com/announce",
            &make_request(0, Some(Event::Stopped)),
            &make_client(),
        );
        assert!(url.contains("event=stopped"), "event in: {url}");
    }

    #[test]
    fn test_build_url_no_placeholders_remain() {
        let url = build_url(
            "http://tracker.example.com/announce",
            &make_request(0, Some(Event::Started)),
            &make_client(),
        );
        assert!(!url.contains('{'), "unreplaced placeholder in: {url}");
        assert!(!url.contains('}'), "unreplaced placeholder in: {url}");
    }

    // --- HttpTransport against a local server ---

    #[tokio::test]
    async fn test_http_transport_success() {
        let (url, server) = serve_once(b"d8:completei3e10:incompletei7e8:intervali900ee").await;
        let client = make_client();
        let transport = HttpTransport::new(&client.user_agent);

        let response = transport
            .announce(&url, &make_request(2048, Some(Event::Started)), &client)
            .await
            .unwrap();
        assert_eq!(response.interval, Some(900));
        assert_eq!(response.seeders, Some(3));
        assert_eq!(response.leechers, Some(7));

        let received = server.await.unwrap();
        assert!(received.starts_with("GET /announce?"), "got: {received}");
        assert!(received.contains("uploaded=2048"), "got: {received}");
        assert!(received.contains("event=started"), "got: {received}");
        let user_agent = format!("user-agent: {}", client.user_agent).to_lowercase();
        assert!(
            received.to_lowercase().contains(&user_agent),
            "emulated user agent missing: {received}"
        );
    }

    #[tokio::test]
    async fn test_http_transport_failure_reason_is_rejected() {
        let (url, server) = serve_once(b"d14:failure reason12:unregisterede").await;
        let client = make_client();
        let result = HttpTransport::new(&client.user_agent)
            .announce(&url, &make_request(0, None), &client)
            .await;
        assert_eq!(
            result,
            Err(AnnounceError::Rejected(String::from("unregistered")))
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_http_transport_bad_body_is_transport_error() {
        let (url, server) = serve_once(b"<html>not a tracker</html>").await;
        let client = make_client();
        let result = HttpTransport::new(&client.user_agent)
            .announce(&url, &make_request(0, None), &client)
            .await;
        match result {
            Err(AnnounceError::Transport(reason)) => {
                assert!(reason.contains("HTTP status 200"), "reason: {reason}");
            }
            other => panic!("unexpected result: {other:?}"),
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn test_http_transport_unreachable_is_transport_error() {
        // Bind then release a port so nothing listens on it.
        let address = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let client = make_client();
        let result = HttpTransport::new(&client.user_agent)
            .announce(
                &format!("http://{address}/announce"),
                &make_request(0, None),
                &client,
            )
            .await;
        assert!(matches!(result, Err(AnnounceError::Transport(_))));
    }
}
