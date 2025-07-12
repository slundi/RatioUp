// https://wiki.theory.org/BitTorrentSpecification#Metainfo_File_Structure
// https://wiki.theory.org/BitTorrent_Tracker_Protocol
use std::fmt;
use std::path::PathBuf;
use std::time::Instant;

use crate::announcer::tracker::is_supported_url;
use crate::bencode::{BencodeDecoder, BencodeDecoderError, BencodeValue, encode_bencode_value};
use crate::utils::{get_sha1, percent_encoding};

/// Errors that can occur when parsing a Torrent struct from Bencode.
#[derive(Debug)]
pub enum TorrentError {
    BencodeError(BencodeDecoderError),
    MissingField(&'static str),
    InvalidFieldType(&'static str),
    ParseError(String), // For general parsing issues (e.g., string to u64)
    Utf8ConversionError(&'static str),
}

// Implement the Display trait for TorrentError
impl fmt::Display for TorrentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TorrentError::BencodeError(e) => write!(f, "Bencode decoding error: {:?}", e),
            TorrentError::MissingField(field) => write!(f, "Missing required field: {}", field),
            TorrentError::InvalidFieldType(field) => write!(f, "Invalid type for field: {}", field),
            TorrentError::ParseError(msg) => write!(f, "Parsing error: {}", msg),
            TorrentError::Utf8ConversionError(field) => {
                write!(f, "UTF-8 conversion error for field: {}", field)
            }
        }
    }
}

// Convert BencodeDecoderError to TorrentError
impl From<BencodeDecoderError> for TorrentError {
    fn from(err: BencodeDecoderError) -> Self {
        TorrentError::BencodeError(err)
    }
}

// #[derive(Debug, PartialEq, Eq, Clone)]
// pub struct Peer {
//     /// A string of length 20 which this peer uses as its id. This field will be `None` for compact peer info.
//     pub id: Option<String>,
//     /// peer's IP address either IPv6 (hexed) or IPv4 (dotted quad) or DNS name (string)
//     pub ip: String,
//     /// peer's port number
//     pub port: i64,
// }

/// To only keep minimal torrent info in RAM. Info are ised in:
/// - the announcer (info hash, urls, name in log, sizes, downloaded, uploaded, interval, last_announce, seeders, leechers)
/// - web UI (info hash, name, size, downloaded, uploaded, seeders, leechers, is private, is a folder, path)
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Torrent {
    pub name: String,
    pub urls: Vec<String>, // aka. announce_list
    pub length: u64,
    pub private: bool,
    // pub info_hash: String,
    /// Total of fake uploaded data since the start of RatioUp
    pub uploaded: u64,
    /// Last announce to the tracker
    pub last_announce: std::time::Instant,
    pub info_hash: [u8; 20],
    /// URL encoded hash thet is used to build the tracker query
    pub info_hash_urlencoded: String,
    /// Number of seeders, it is used on the web UI
    pub seeders: u16,
    /// Number of leechers, it is used on the web UI
    pub leechers: u16,
    /// It is the next upload speed that will be announced. It is also used for UI display.
    pub next_upload_speed: u32,
    /// Current interval after the last annouce
    pub interval: u64,
    pub error_count: u16,
    // pub creation_date: Option<DateTime<Local>>,
    // pub comment: Option<String>,
    // pub created_by: Option<String>,
    pub encoding: Option<String>,

    // for tracker response
    /// (optional) Minimum announce interval. If present clients must not reannounce more frequently than this.
    pub min_interval: Option<u64>,
    /// A string that the client should send back on its next announcements. If absent and a previous announce sent a tracker id, do not discard the old value; keep using it.
    pub tracker_id: Option<String>,
}

impl Torrent {
    /// Tells if we can announce to tracker(s) depending on the last announce
    pub fn shound_announce(&self) -> bool {
        self.last_announce.elapsed().as_secs() >= self.interval
    }

    /// Tells if we can upload (need leechers)
    pub fn can_upload(&self) -> bool {
        (self.seeders > 0 && self.leechers > 0) || self.leechers > 1
    }

    pub fn uploaded(&mut self, min_speed: u32, available_speed: u32) -> u32 {
        if self.can_upload() && (0 < min_speed && min_speed <= available_speed) {
            self.next_upload_speed = fastrand::u32(min_speed..available_speed);
            self.next_upload_speed
        } else {
            0
        }
    }

    pub fn compute_speeds(&mut self) {
        let config = crate::CONFIG.get().unwrap();
        self.uploaded(config.min_upload_rate, config.max_upload_rate);
    }

    // /// Load essential data from a parsed torrent using the full parsed torrent file. It reduces the RAM use to have smaller data
    // pub fn from_torrent(torrent: Torrent) -> Self {
    //     let hash_bytes = torrent.info_hash().expect("Cannot get torrent info hash");
    //     let hash = hash_bytes.encode_hex::<String>();
    //     //let hash = hash_bytes.???;
    //     let private = torrent.info.private.is_some() && torrent.info.private == Some(1);
    //     let mut t = Self {
    //         name: torrent.info.name.clone(),
    //         info_hash_urlencoded: String::with_capacity(64),
    //         length: torrent.total_size,
    //         last_announce: std::time::Instant::now(),
    //         urls: Vec::new(),
    //         info_hash: hash,
    //         private,
    //         downloaded: torrent.total_size,
    //         uploaded: 0,
    //         seeders: 0,
    //         leechers: 0,
    //         next_upload_speed: 0,
    //         next_download_speed: 0,
    //         interval: 4_294_967_295,
    //         error_count: 0,
    //     };
    //     t.urls = torrent.get_urls();
    //     t.info_hash_urlencoded = percent_encoding::percent_encode(
    //         &hash_bytes,
    //         crate::announcer::tracker::URL_ENCODE_RESERVED,
    //     )
    //     .to_string();
    //     debug!("Torrent: {:?}", t);
    //     t
    // }

    pub fn from_file(path: PathBuf) -> Result<Self, TorrentError> {
        let data = std::fs::read(path).expect("Cannot read torrent file");
        Self::from_bencode_bytes(&data)
    }

    pub fn to_json(&self) -> String {
        let mut result = String::with_capacity(256);
        result.push_str("\t{\"name\": \"");
        result.push_str(&self.name.replace("\"", "\\\""));
        result.push_str("\", \"length\": ");
        result.push_str(&self.length.to_string());
        result.push_str(", \"private\": ");
        result.push_str(&self.private.to_string());
        result.push_str(", \"uploaded\": ");
        result.push_str(&self.uploaded.to_string());
        result.push_str(", \"seeders\": ");
        result.push_str(&self.seeders.to_string());
        result.push_str(", \"leechers\": ");
        result.push_str(&self.leechers.to_string());
        result.push_str(", \"next_upload_speed\": ");
        result.push_str(&self.next_upload_speed.to_string());
        result.push_str(", \"urls\": [");
        let count = self.urls.len();
        for (index, url) in self.urls.iter().enumerate() {
            result.push_str(&format!("\"{url}\""));
            if (index + 1) < count {
                result.push_str(", ");
            } else {
                result.push_str("]}\n");
            }
        }
        // TODO: add info hash?
        result
    }

    /// Parses a raw bencoded .torrent file byte slice into a Torrent struct.
    ///
    /// This function decodes the Bencode structure, extracts relevant fields,
    /// calculates the info hash, and initializes default values for other fields.
    ///
    /// # Arguments
    /// * `bencode_data` - A byte slice containing the full bencoded .torrent file content.
    ///
    /// # Returns
    /// A `Result` which is `Ok(Torrent)` on success or `Err(TorrentError)` on failure.
    pub fn from_bencode_bytes(bencode_data: &[u8]) -> Result<Self, TorrentError> {
        let mut decoder = BencodeDecoder::new(bencode_data);
        let top_level_dict = match decoder.decode()? {
            BencodeValue::Dictionary(dict) => dict,
            _ => {
                return Err(TorrentError::InvalidFieldType(
                    "Top-level is not a dictionary",
                ));
            }
        };

        // --- Extract announce URLs ---
        let mut urls = Vec::new();
        // Try to get 'announce-list' first (multi-tracker)
        if let Some(BencodeValue::List(announce_list_bencode)) =
            top_level_dict.get(b"announce-list".as_ref())
        {
            for tier in announce_list_bencode {
                if let BencodeValue::List(tier_urls) = tier {
                    for url_bencode in tier_urls {
                        if let BencodeValue::ByteString(url_bytes) = url_bencode {
                            let url_str = std::str::from_utf8(url_bytes)
                                .map_err(|_| {
                                    TorrentError::Utf8ConversionError("announce-list URL")
                                })?
                                .to_string();
                            if !urls.contains(&url_str) && is_supported_url(&url_str) {
                                // Avoid duplicates
                                urls.push(url_str);
                            }
                        }
                    }
                }
            }
        }

        // Try to get 'announce' (single tracker), add if not already in urls
        if let Some(BencodeValue::ByteString(announce_bytes)) =
            top_level_dict.get(b"announce".as_ref())
        {
            let announce_str = std::str::from_utf8(announce_bytes)
                .map_err(|_| TorrentError::Utf8ConversionError("announce URL"))?
                .to_string();
            if !urls.contains(&announce_str) && is_supported_url(&announce_str) {
                // Avoid duplicates
                urls.push(announce_str);
            }
        }

        if urls.is_empty() {
            return Err(TorrentError::MissingField("announce or announce-list"));
        }

        // --- Extract 'info' dictionary and calculate info_hash ---
        // `info_bytes_slice` is `&BencodeValue`
        let info_bytes_slice = top_level_dict
            .get(b"info".as_ref())
            .ok_or(TorrentError::MissingField("info"))?;

        // Ensure info_bytes_slice is indeed a dictionary before proceeding
        let info_dict_map = match info_bytes_slice {
            BencodeValue::Dictionary(dict) => dict, // `dict` here is `&BTreeMap`
            _ => return Err(TorrentError::InvalidFieldType("info is not a dictionary")),
        };

        let mut encoder_buf = Vec::new();
        // Pass the reference to the info dictionary directly to the encoder.
        // `info_bytes_slice` is already `&BencodeValue`.
        encode_bencode_value(info_bytes_slice, &mut encoder_buf)?;
        let info_bencoded_raw = encoder_buf;

        let info_hash: [u8; 20] = get_sha1(&info_bencoded_raw);
        let info_hash_urlencoded = percent_encoding(&info_hash);

        // --- Decode 'info' dictionary content ---
        // `info_dict_map` is already `&BTreeMap` from the match above, so we can use it directly.

        let name_bytes = info_dict_map
            .get(b"name".as_ref())
            .ok_or(TorrentError::MissingField("info.name"))?;
        let name = match name_bytes {
            BencodeValue::ByteString(b) => std::str::from_utf8(b)
                .map_err(|_| TorrentError::Utf8ConversionError("info.name"))?
                .to_string(),
            _ => return Err(TorrentError::InvalidFieldType("info.name")),
        };

        let mut total_length: u64 = 0;
        let mut is_private = false;
        let mut encoding_option: Option<String> = None;

        // Handle 'length' for single-file torrents
        if let Some(BencodeValue::Integer(len)) = info_dict_map.get(b"length".as_ref()) {
            if *len < 0 {
                return Err(TorrentError::ParseError(
                    "info.length is negative".to_string(),
                ));
            }
            total_length = *len as u64;
        }

        // Handle 'files' for multi-file torrents
        if let Some(BencodeValue::List(files)) = info_dict_map.get(b"files".as_ref()) {
            total_length = 0; // Reset if 'files' is present, sum up
            for file_entry in files {
                if let BencodeValue::Dictionary(file_dict) = file_entry {
                    if let Some(BencodeValue::Integer(file_len)) = file_dict.get(b"length".as_ref())
                    {
                        if *file_len < 0 {
                            return Err(TorrentError::ParseError(
                                "file.length is negative".to_string(),
                            ));
                        }
                        total_length += *file_len as u64;
                    } else {
                        return Err(TorrentError::MissingField(
                            "file.length in multi-file torrent",
                        ));
                    }
                } else {
                    return Err(TorrentError::InvalidFieldType(
                        "file entry in multi-file torrent",
                    ));
                }
            }
        }

        // Handle 'private' flag
        if let Some(BencodeValue::Integer(private_val)) = info_dict_map.get(b"private".as_ref()) {
            is_private = *private_val == 1;
        }

        // Handle 'encoding'
        if let Some(BencodeValue::ByteString(encoding_bytes)) =
            top_level_dict.get(b"encoding".as_ref())
        {
            encoding_option = Some(
                std::str::from_utf8(encoding_bytes)
                    .map_err(|_| TorrentError::Utf8ConversionError("encoding"))?
                    .to_string(),
            );
        }

        Ok(Torrent {
            name,
            urls,
            length: total_length,
            private: is_private,
            uploaded: 0,                   // Default value
            last_announce: Instant::now(), // Default value
            info_hash,
            info_hash_urlencoded,
            seeders: 0,           // Default value
            leechers: 0,          // Default value
            next_upload_speed: 0, // Default value
            interval: 0,          // Default value
            error_count: 0,       // Default value
            encoding: encoding_option,
            min_interval: None, // Default value (from tracker response, not torrent file)
            tracker_id: None,   // Default value (from tracker response, not torrent file)
        })
    }
}

// TODO: test tracker response "with d8:completei0e10:downloadedi0e10:incompletei1e8:intervali1922e12:min intervali961e5:peers6:<3A><><EFBFBD>m<EFBFBD><6D>e"
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_can_download_or_upload() {
        let mut t = Torrent {
            name: String::from("Test torrent"),
            length: 262144,
            private: false,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: [0; 20],
            info_hash_urlencoded: String::from("01234567"),
            seeders: 0,
            leechers: 1,
            next_upload_speed: 0,
            interval: 1800,
            urls: Vec::with_capacity(0),
            error_count: 0,
            encoding: None,
            min_interval: None,
            tracker_id: None,
        };
        assert!(!t.can_upload());
        t.leechers = 5;
        assert!(t.can_upload());
        t.leechers = 0;
        t.seeders = 1;
        assert!(!t.can_upload());
        t.seeders = 4;
        t.leechers = 8;
        assert!(t.can_upload());
    }

    #[test]
    fn test_get_average_speeds() {
        let mut t = Torrent {
            name: String::from("Test torrent"),
            length: 262144,
            private: false,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: [0; 20],
            info_hash_urlencoded: String::from("01234567"),
            seeders: 4,
            leechers: 16,
            next_upload_speed: 0,
            interval: 1800,
            urls: Vec::with_capacity(0),
            error_count: 0,
            encoding: None,
            min_interval: None,
            tracker_id: None,
        };
        let speed = t.uploaded(16, 64);
        assert!(speed > 0);
        t.interval = 1;
        std::thread::sleep(std::time::Duration::from_secs(2));
        assert!((16..=64).contains(&speed));
        let speed = t.uploaded(16, 64);
        assert!((16..=64).contains(&speed));
    }
}
