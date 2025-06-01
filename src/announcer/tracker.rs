// https://xbtt.sourceforge.net/udp_tracker_protocol.html
// based on https://github.com/billyb2/cratetorrent/blob/master/cratetorrent/src/tracker.rs

use std::io::Read;

use fake_torrent_client::Client;
use tracing::{debug, error, info, warn};

use crate::torrent::CleansedTorrent;
use crate::{CLIENT, TORRENTS};

pub const URL_ENCODE_RESERVED: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'~')
    .remove(b'.');

// enum ErrorCode {
//     /// Client request was not a HTTP GET
//     InvalidRequestType = 100,
//     MissingInfosash = 101,
//     MissingPeerId = 102,
//     MissingPort = 103,
//     /// infohash is not 20 bytes long.
//     InvalidInfohash = 150,
//     /// peerid is not 20 bytes long
//     InvalidPeerId = 151,
//     /// Client requested more peers than allowed by tracker
//     InvalidNumwant = 152,
//     /// info_hash not found in the database. Sent only by trackers that do not automatically include new hashes into the database.
//     InfohashNotFound = 200,
//     /// Client sent an eventless request before the specified time.
//     ClientSentEventlessRequest = 500,
//     GenericError = 900,
// }

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

pub fn announce_stopped() {
    // TODO: compute uploaded and downloaded then announce
    let list = TORRENTS.read().expect("Cannot get torrent list");
    for m in list.iter() {
        let mut t = m.lock().unwrap();
        t.interval = announce(&mut t, Some(Event::Stopped));
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
pub fn announce(torrent: &mut CleansedTorrent, event: Option<Event>) -> u64 {
    let mut interval = 4_294_967_295u64;
    // TODO: prepare announce (uploaded and downloaded if applicable)
    torrent.compute_speeds();
    if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
        debug!("Torrent has {} url(s)", torrent.urls.len());
        for url in torrent.urls.clone() {
            if url.to_lowercase().starts_with("udp://") {
                warn!("UDP tracker not supported (yet): cannot announce");
                // interval = futures::executor::block_on(announce_udp(&url, torrent, client, event));
            } else {
                interval = announce_http(&url, torrent, client, event);
            }
        }
        info!(
            "Anounced: interval={}, event={:?}, downloaded={}, uploaded={}, seeders={}, leechers={}, torrent={}",
            torrent.interval,
            event,
            torrent.downloaded,
            torrent.uploaded,
            torrent.seeders,
            torrent.leechers,
            torrent.name
        );
    }
    interval
}

/// Check which torrents need to be announced and call the announce fuction when applicable
pub fn check_and_announce() {
    let list = TORRENTS.read().expect("Cannot get torrent list");
    for m in list.iter() {
        let mut t = m.lock().unwrap();
        if t.shound_announce() {
            announce(&mut t, None);
        }
    }
}

fn announce_http(
    url: &str,
    torrent: &mut CleansedTorrent,
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
                    torrent.last_announce = std::time::Instant::now();
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
            // TODO: check response code
        }
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
pub fn build_url(
    url: &str,
    torrent: &mut CleansedTorrent,
    event: Option<Event>,
    key: String,
) -> String {
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
    let client = (*CLIENT.read().expect("Cannot read client"))
        .clone()
        .unwrap();
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
        byte_unit::Byte::from_u128(downloaded as u128)
            .unwrap()
            .get_appropriate_unit(byte_unit::UnitType::Decimal)
            .to_string(),
        byte_unit::Byte::from_u128(uploaded as u128)
            .unwrap()
            .get_appropriate_unit(byte_unit::UnitType::Decimal)
            .to_string()
    );
    info!("\tAnnonce at: {}", url);
    result
}

#[cfg(test)]
mod tests {
    // use super::*;
}
