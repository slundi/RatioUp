// https://wiki.theory.org/BitTorrentSpecification#Metainfo_File_Structure
// https://wiki.theory.org/BitTorrent_Tracker_Protocol
use bendy::decoding::{FromBencode, Object};
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::{debug, error};

use crate::announcer::tracker::is_supprted_url;
use crate::utils::percent_encoding;

type BendyResult<T> = Result<T, bendy::decoding::Error>;

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Peer {
    /// A string of length 20 which this peer uses as its id. This field will be `None` for compact peer info.
    pub id: Option<String>,
    /// peer's IP address either IPv6 (hexed) or IPv4 (dotted quad) or DNS name (string)
    pub ip: String,
    /// peer's port number
    pub port: i64,
}

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

    pub fn from_file(path: PathBuf) -> Result<Self, bendy::decoding::Error> {
        let data = std::fs::read(path).expect("Cannot read torrent file");
        Self::from_bencode(&data)
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
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct File {
    pub length: u64,
    pub md5sum: Option<String>,
    pub path: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Info {
    pub name: String,
    pub files: Vec<File>,
    pub piece_length: u32,
    pub pieces: Vec<[u8; 20]>,
    pub private: bool,
}

// #[derive(Debug, Default, PartialEq, Eq)]
// pub struct Torrent {
//     pub info: Info,
//     pub announce_list: Vec<String>,
//     // pub creation_date: Option<DateTime<Local>>,
//     // pub comment: Option<String>,
//     // pub created_by: Option<String>,
//     pub encoding: Option<String>,
//     pub total_size: u64,
// }

impl FromBencode for Torrent {
    fn decode_bencode_object(object: Object) -> BendyResult<Self>
    where
        Self: Sized,
    {
        let mut announce_list = HashSet::new();
        // let mut creation_date = None;
        // let mut comment = None;
        // let mut created_by = None;
        let mut encoding = None;
        let mut total_size = 0u64;
        let mut valid_info = false;
        let mut name = String::with_capacity(0);
        let mut private = false;
        let mut info_hash_urlencoded = String::with_capacity(0);

        let mut dict = object.try_into_dictionary()?;
        while let Some(pair) = dict.next_pair()? {
            match pair {
                (b"info", value) => {
                    let bytes = value.try_into_dictionary()?.into_raw()?;

                    let hash: [u8; 20] = crate::utils::get_sha1(bytes);
                    debug!("Hash: {:?}", String::from_utf8_lossy(&hash));

                    let mut decoder: bendy::decoding::Decoder = bendy::decoding::Decoder::new(bytes);

                    // let info2: Info = Info::decode_bencode_object(decoder.next_object()?.unwrap().)?;
                    // for file in &info2.files {
                    //     total_size += file.length;
                    // }
                    // name = info2.name;
                    // private = info2.private;
                    tracing::debug!("Info hash: {:?}", hash);
                    info_hash_urlencoded = percent_encoding(&hash).to_string();
                    valid_info = true;
                }
                (b"announce", value) => {
                    announce_list.insert(String::decode_bencode_object(value)?);
                }
                (b"announce-list", value) => {
                    let mut list_raw = value.try_into_list()?;
                    while let Some(value) = list_raw.next_object()? {
                        let mut tier_list = value.try_into_list()?;
                        while let Some(value) = tier_list.next_object()? {
                            announce_list.insert(String::decode_bencode_object(value)?);
                        }
                    }
                }
                // (b"creation date", value) => {
                //     creation_date = Some(
                //         Local
                //             .timestamp_opt(i64::decode_bencode_object(value)?, 0)
                //             .unwrap(),
                //     )
                // }
                // (b"comment", value) => comment = Some(String::decode_bencode_object(value)?),
                // (b"created by", value) => created_by = Some(String::decode_bencode_object(value)?),
                (b"encoding", value) => encoding = Some(String::decode_bencode_object(value)?),
                _ => {}
            }
        }

        if !valid_info {
            error!("Decoding Error: Missing info dictionary");
            std::process::exit(1);
        }
        let mut urls: Vec<String> = Vec::with_capacity(announce_list.len());
        // TODO: skip UDP and local URLs
        for url in announce_list.into_iter() {
            if is_supprted_url(&url) {
                urls.push(url);
            }
        }

        Ok(Self {
            urls,
            // creation_date,
            // comment,
            // created_by,
            encoding,
            length: total_size,
            name,
            info_hash: [0; 20],
            info_hash_urlencoded,
            last_announce: std::time::Instant::now(),
            private,
            uploaded: 0,
            seeders: 0,
            leechers: 0,
            next_upload_speed: 0,
            interval: 4_294_967_295,
            error_count: 0,
            min_interval: None,
            tracker_id: None,
        })
    }
}

impl FromBencode for Info {
    fn decode_bencode_object(object: Object) -> BendyResult<Self>
    where
        Self: Sized,
    {
        let mut name: Option<String> = None;
        let mut files: Option<Vec<File>> = None;

        let mut length: Option<u64> = None;
        let mut md5sum: Option<String> = None;
        let mut private = false;

        let mut piece_length: Option<u32> = None;
        let mut pieces_raw: Option<Vec<u8>> = None;

        let mut dict = object.try_into_dictionary()?;
        while let Some(pair) = dict.next_pair()? {
            match pair {
                (b"piece length", value) => piece_length = Some(u32::decode_bencode_object(value)?),
                (b"pieces", value) => pieces_raw = Some(value.try_into_bytes()?.to_vec()),
                (b"name", value) => name = Some(String::decode_bencode_object(value)?),
                (b"files", value) => {
                    files = Some(Vec::decode_bencode_object(value)?);
                    // files = Some(value.list_or_else(|obj| {
                    //     // obj.try_into_bytes()?
                    //     Vec::with_capacity(0)
                    // }));
                }
                (b"length", value) => length = Some(u64::decode_bencode_object(value)?),
                (b"md5sum", value) => md5sum = Some(String::decode_bencode_object(value)?),
                (b"private", value) => {
                    private = u8::decode_bencode_object(value)? == 1;
                }
                _ => {}
            }
        }

        if piece_length.is_none() || pieces_raw.is_none() {
            return Err(bendy::decoding::Error::missing_field(
                "piece length or pieces",
            ));
        }
        let pl = piece_length.unwrap();
        let raw = pieces_raw.unwrap();
        if raw.len() % 20 != 0 {
            return Err(bendy::decoding::Error::missing_field(
                "Invalid length for pieces",
            ));
        }
        let mut pieces = vec![];
        for chunk in raw.chunks_exact(20) {
            let mut arr = [0u8; 20];
            arr.copy_from_slice(chunk);
            pieces.push(arr);
        }

        let name = name.expect("Decoding Error: Missing name from torrent info");

        if let Some(files) = files {
            Ok(Self {
                name,
                files,
                piece_length: pl,
                pieces,
                private,
            })
        } else {
            // single-file torrent: use the name as the file path
            Ok(Self {
                name: name.clone(),
                files: vec![File {
                    length: length.expect("Decoding Error: Missing file length"),
                    md5sum,
                    path: PathBuf::from(name.clone()),
                }],
                // files: Vec::with_capacity(0),
                piece_length: pl,
                pieces,
                private,
            })
        }
    }
}

impl FromBencode for File {
    fn decode_bencode_object(object: Object) -> BendyResult<Self>
    where
        Self: Sized,
    {
        let mut length = None;
        let mut md5sum = None;
        // let mut path = PathBuf::new();

        let mut dict = object.try_into_dictionary()?;
        while let Some(pair) = dict.next_pair()? {
            match pair {
                (b"length", value) => length = Some(u64::decode_bencode_object(value)?),
                (b"md5sum", value) => md5sum = Some(String::decode_bencode_object(value)?),
                // FIXME:
                // (b"path", value) => {debug!("File");
                //     path = Vec::decode_bencode_object(value)?
                //         .into_iter()
                //         .map(|bytes| String::from_utf8(bytes).unwrap())
                //         .collect()
                // }
                _ => {}
            }
        }

        let length = length.expect("Decoding Error: File missing length");
        // debug!("\t{:?} {length}  {}", md5sum, path.display());

        Ok(Self {
            length,
            md5sum,
            path: PathBuf::new(),
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
