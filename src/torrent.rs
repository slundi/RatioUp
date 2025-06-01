// https://wiki.theory.org/BitTorrentSpecification#Metainfo_File_Structure
// https://wiki.theory.org/BitTorrent_Tracker_Protocol
extern crate serde;
extern crate serde_bencode;
extern crate serde_bytes;

use hex::ToHex;
use hmac_sha1_compact::Hash;
use tracing::debug;
use rand::Rng;
use serde::Serialize;
use serde_bencode::ser;
use serde_bytes::ByteBuf;

#[derive(Debug, PartialEq, Eq, Deserialize, Clone)]
pub struct Peer {
    /// A string of length 20 which this peer uses as its id. This field will be `None` for compact peer info.
    pub id: Option<String>,
    /// peer's IP address either IPv6 (hexed) or IPv4 (dotted quad) or DNS name (string)
    pub ip: String,
    /// peer's port number
    pub port: i64,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
struct Node(String, i64);

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct File {
    /// a list containing one or more string elements that together represent the path and filename. Each element in the list corresponds to
    /// either a directory name or (in the case of the final element) the filename. For example, a the file "dir1/dir2/file.ext" would
    /// consist of three string elements: "dir1", "dir2", and "file.ext". This is encoded as a bencoded list of strings such as l4:dir14:dir28:file.exte
    pub path: Vec<String>,
    /// length of the file in bytes (integer)
    pub length: i64,
    #[serde(default)]
    md5sum: Option<String>,
}

impl File {
    pub fn get_path_with_separator(&self) -> String {
        let mut result = String::new();
        for s in self.path.iter() {
            result.push('/');
            result.push_str(s);
        }
        result
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Default)]
pub struct Info {
    /// the filename. This is purely advisory. (string)
    pub name: String,
    pieces: ByteBuf,
    #[serde(rename = "piece length")]
    pub piece_length: i64,
    #[serde(default)]
    md5sum: Option<String>,
    #[serde(default)]
    pub length: Option<i64>,
    /// a list of dictionaries, one for each file.
    #[serde(default)]
    pub files: Option<Vec<File>>,
    #[serde(default)]
    pub private: Option<u8>,
    #[serde(default)]
    pub path: Option<Vec<String>>,
    #[serde(default, rename = "root hash")]
    pub root_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Default)]
pub struct Torrent {
    pub info: Info,
    /// The announce URL of the tracker
    #[serde(default)]
    pub announce: Option<String>,
    #[serde(default)]
    nodes: Option<Vec<Node>>,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    httpseeds: Option<Vec<String>>,
    /// (optional) this is an extention to the official specification, offering backwards-compatibility.
    /// (list of lists of strings). http://bittorrent.org/beps/bep_0012.html
    #[serde(default, rename = "announce-list")]
    pub announce_list: Option<Vec<Vec<String>>>,
    #[serde(default, rename = "creation date")]
    pub creation_date: Option<i64>,
    /// (optional) free-form textual comments of the author (string)
    #[serde(rename = "comment")]
    pub comment: Option<String>,
    /// (optional) name and version of the program used to create the .torrent (string)
    #[serde(default, rename = "created by")]
    pub created_by: Option<String>,
}

impl Torrent {
    pub fn files(&self) -> &Option<Vec<File>> {
        &self.info.files
    }
    pub fn _num_files(&self) -> usize {
        match self.files() {
            Some(f) => f.len(),
            None => 1,
        }
    }
    pub fn total_size(&self) -> usize {
        if self.files().is_none() {
            return self.info.length.unwrap_or_default() as usize;
        }
        let mut total_size = 0;
        if let Some(files) = self.files() {
            for file in files {
                total_size += file.length;
            }
        }
        total_size as usize
    }

    pub fn info_hash(&self) -> Option<Vec<u8>> {
        let result = ser::to_bytes(&self.info);
        if let Ok(info) = result {
            return Some(Hash::hash(&info).to_vec());
        }
        None
    }

    pub fn get_urls(&self) -> Vec<String> {
        let mut urls: Vec<String> = Vec::new();
        if let Some(url) = self.announce.clone() {
            urls.push(url);
        }
        if let Some(al) = self.announce_list.clone() {
            for v in al {
                for s in v {
                    if !s.is_empty() && !urls.iter().any(|value| value == &s) {
                        urls.push(s);
                    }
                }
            }
        }
        urls
    }
}

/// To only keep minimal torrent info in RAM. Info are ised in:
/// - the announcer (info hash, urls, name in log, sizes, downloaded, uploaded, interval, last_announce, seeders, leechers)
/// - web UI (info hash, name, size, downloaded, uploaded, seeders, leechers, is private, is a folder, path)
#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct CleansedTorrent {
    pub name: String,
    pub urls: Vec<String>,
    pub length: usize,
    pub private: bool,
    /// If the torrent contains many files, not just one
    pub folder: bool,
    pub info_hash: String,
    /// Path to the torrent file
    pub path: String,
    /// If we have to virtually download the torrent first, it is the downloaded size in bytes
    pub downloaded: usize,
    /// Total of fake uploaded data since the start of RatioUp
    pub uploaded: usize,
    /// Last announce to the tracker
    #[serde(skip_serializing)]
    pub last_announce: std::time::Instant,
    /// URL encoded hash thet is used to build the tracker query
    #[serde(skip_serializing)]
    pub info_hash_urlencoded: String,
    /// Number of seeders, it is used on the web UI
    pub seeders: u16,
    /// Number of leechers, it is used on the web UI
    pub leechers: u16,
    /// It is the next upload speed that will be announced. It is also used for UI display.
    pub next_upload_speed: u32,
    /// It is the next download speed that will be announced. It allows to end a complete event earlier than the normal interval, It is also used for UI display.
    pub next_download_speed: u32,
    /// Current interval after the last annouce
    #[serde(skip)]
    pub interval: u64,
    #[serde(skip)]
    pub error_count: u16,
}

/// Store only essential information
#[derive(Debug, Serialize, PartialEq, Eq, Clone)]
pub struct BasicTorrent {
    /// the filename. This is purely advisory. (string)
    pub name: String,
    //creation_date? (optional) the creation time of the torrent, in standard UNIX epoch format (integer, seconds since 1-Jan-1970 00:00:00 UTC)
    /// urlencoded 20-byte SHA1 hash of the value of the info key from the Metainfo file. Note that the value will be a bencoded dictionary, given the definition of the info key above.
    pub info_hash: String,
    /// number of bytes in each piece (integer)
    piece_length: usize,
    /// length of the file in bytes (integer)
    pub length: usize,
    /// a list of dictionaries, one for each file.
    files: Option<Vec<File>>,
    /// (optional) this field is an integer. If it is set to "1", the client MUST publish its presence to get other peers ONLY via the trackers explicitly described
    /// in the metainfo file. If this field is set to "0" or is not present, the client may obtain peer from other means, e.g. PEX peer exchange, dht. Here, "private"
    /// may be read as "no external peer source".
    ///
    /// - NOTE: There is much debate surrounding private trackers.
    /// - The official request for a specification change is here: http://bittorrent.org/beps/bep_0027.html
    /// - Azureus/Vuze was the first client to respect private trackers, see their wiki (http://wiki.vuze.com/w/Private_torrent) for more details.
    pub private: bool,

    //Fields used by RatioUp
    /// Path to the torrent file
    #[serde(skip_serializing)]
    pub path: String,
    /// If we have to virtually download the torrent first, it is the downloaded size in bytes
    pub downloaded: usize,
    /// Total of fake uploaded data since the start of RatioUp
    pub uploaded: usize,
    /// Last announce to the tracker
    #[serde(skip_serializing)]
    pub last_announce: std::time::Instant,
    /// Number of seeders, it is used on the web UI
    pub seeders: u16,
    /// Number of leechers, it is used on the web UI
    pub leechers: u16,
    /// It is the next upload speed that will be announced. It is also used for UI display.
    pub next_upload_speed: u32,
    /// It is the next download speed that will be announced. It allows to end a complete event earlier than the normal interval, It is also used for UI display.
    pub next_download_speed: u32,
    /// Tracker announce URLs built from the config and the torrent. Some variables are still there (key, left, downloaded, uploaded, event)
    #[serde(skip)]
    pub urls: Vec<String>,
    #[serde(skip)]
    pub error_count: u16,
}

impl CleansedTorrent {
    /// Tells if we can announce to tracker(s) depending on the last announce
    pub fn shound_announce(&self) -> bool {
        self.last_announce.elapsed().as_secs() >= self.interval
    }

    /// Tells if we can upload (need leechers)
    pub fn can_upload(&self) -> bool {
        (self.seeders > 0 && self.leechers > 0) || self.leechers > 1
    }

    /// Tells if we can download (need leechers or seeders)
    pub fn can_download(&self) -> bool {
        self.seeders > 0 || self.leechers > 1
    }

    pub fn downloaded(&mut self, min_speed: u32, available_speed: u32) -> u32 {
        if self.can_download() {
            self.next_download_speed = rand::rng().random_range(min_speed..available_speed);
            self.next_download_speed
        } else {
            0
        }
    }

    pub fn uploaded(&mut self, min_speed: u32, available_speed: u32) -> u32 {
        if self.can_upload() {
            self.next_upload_speed = rand::rng().random_range(min_speed..available_speed);
            self.next_upload_speed
        } else {
            0
        }
    }

    pub fn compute_speeds(&mut self) {
        let config = crate::CONFIG.get().unwrap();
        self.downloaded(config.min_download_rate, config.max_download_rate);
        self.uploaded(config.min_upload_rate, config.max_upload_rate);
    }

    /// Load essential data from a parsed torrent using the full parsed torrent file. It reduces the RAM use to have smaller data
    pub fn from_torrent(torrent: Torrent, path: String) -> CleansedTorrent {
        let hash_bytes = torrent.info_hash().expect("Cannot get torrent info hash");
        let hash = hash_bytes.encode_hex::<String>();
        //let hash = hash_bytes.???;
        let private = torrent.info.private.is_some() && torrent.info.private == Some(1);
        let size = torrent.total_size();
        let mut folder = false;
        if let Some(files) = torrent.info.files.clone() {
            folder = !files.is_empty();
        }
        let mut t = CleansedTorrent {
            path,
            name: torrent.info.name.clone(),
            info_hash_urlencoded: String::with_capacity(64),
            length: size,
            last_announce: std::time::Instant::now(),
            urls: Vec::new(),
            info_hash: hash,
            private,
            folder,
            downloaded: size,
            uploaded: 0,
            seeders: 0,
            leechers: 0,
            next_upload_speed: 0,
            next_download_speed: 0,
            interval: 4_294_967_295,
            error_count: 0,
        };
        t.urls = torrent.get_urls();
        t.info_hash_urlencoded = percent_encoding::percent_encode(
            &hash_bytes,
            crate::announcer::tracker::URL_ENCODE_RESERVED,
        )
        .to_string();
        debug!("Torrent: {:?}", t);
        t
    }
}

impl BasicTorrent {
    /// Load torrent data required for a detailled view in the web UI
    pub fn from_torrent(torrent: Torrent, path: String) -> BasicTorrent {
        let hash_bytes = torrent.info_hash().expect("Cannot get torrent info hash");
        let hash = hash_bytes.encode_hex::<String>();
        //let hash = hash_bytes.???;
        let private = torrent.info.private.is_some() && torrent.info.private == Some(1);
        let size = torrent.total_size();
        let mut t = BasicTorrent {
            path,
            name: torrent.info.name.clone(),
            length: size,
            urls: Vec::new(),
            info_hash: hash,
            piece_length: torrent.info.piece_length as usize,
            private,
            files: None,
            downloaded: size,
            uploaded: 0,
            seeders: 0,
            leechers: 0,
            next_upload_speed: 0,
            next_download_speed: 0,
            error_count: 0,
            last_announce: std::time::Instant::now(),
        };
        t.urls = torrent.get_urls();
        if let Some(files) = torrent.info.files {
            let mut list: Vec<File> = Vec::with_capacity(files.len());
            for f in files {
                list.push(File {
                    length: f.length,
                    path: f.path,
                    md5sum: None,
                });
            }
            t.files = Some(list);
        }
        debug!("Torrent: {:?}", t);
        t
    }
}

pub fn from_file(path: String) -> Result<Torrent, serde_bencode::Error> {
    let data = std::fs::read(path).expect("Cannot read torrent file");
    serde_bencode::de::from_bytes::<Torrent>(&data)
}

// TODO: test tracker response "with d8:completei0e10:downloadedi0e10:incompletei1e8:intervali1922e12:min intervali961e5:peers6:<3A><><EFBFBD>m<EFBFBD><6D>e"
#[cfg(test)]
mod tests {
    use crate::torrent::CleansedTorrent;

    #[test]
    fn test_can_download_or_upload() {
        let mut t = CleansedTorrent {
            name: String::from("Test torrent"),
            length: 262144,
            private: false,
            folder: false,
            path: String::from("torrents/linuxmint-21.2-mate-64bit.iso.torrent"),
            downloaded: 262144,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: String::from("01234567"),
            info_hash_urlencoded: String::from("01234567"),
            seeders: 0,
            leechers: 1,
            next_upload_speed: 0,
            next_download_speed: 0,
            interval: 1800,
            urls: Vec::with_capacity(0),
            error_count: 0,
        };
        assert!(!t.can_download());
        assert!(!t.can_upload());
        t.leechers = 5;
        assert!(t.can_download());
        assert!(t.can_upload());
        t.leechers = 0;
        t.seeders = 1;
        assert!(t.can_download());
        assert!(!t.can_upload());
        t.seeders = 4;
        t.leechers = 8;
        assert!(t.can_download());
        assert!(t.can_upload());
    }

    #[test]
    fn test_get_average_speeds() {
        let mut t = CleansedTorrent {
            name: String::from("Test torrent"),
            length: 262144,
            private: false,
            folder: false,
            path: String::from("torrents/linuxmint-21.2-mate-64bit.iso.torrent"),
            downloaded: 262144,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: String::from("01234567"),
            info_hash_urlencoded: String::from("01234567"),
            seeders: 4,
            leechers: 16,
            next_upload_speed: 0,
            next_download_speed: 0,
            interval: 1800,
            urls: Vec::with_capacity(0),
            error_count: 0,
        };
        let speed = t.downloaded(16, 64);
        assert!(speed > 0);
        let speed = t.uploaded(16, 64);
        assert!(speed > 0);
        t.interval = 1;
        std::thread::sleep(std::time::Duration::from_secs(2));
        let speed = t.downloaded(16, 64);
        assert!((16..=64).contains(&speed));
        let speed = t.uploaded(16, 64);
        assert!((16..=64).contains(&speed));
    }
}
