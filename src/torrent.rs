// https://wiki.theory.org/BitTorrentSpecification#Metainfo_File_Structure
extern crate serde;
extern crate serde_bencode;
extern crate serde_bytes;
extern crate sha1;
extern crate lazy_static;
use std::{io::Read};

use regex::Regex;
use serde_bytes::ByteBuf;
use tracing::{info, error, warn};
use url::form_urlencoded::byte_serialize;
use serde::Serialize;
use serde_bencode::ser;
use sha1::{Digest, Sha1};
use hex::ToHex;

pub const EVENT_NONE: &str = "";
//pub const EVENT_COMPLETED: &str = "completed"; //not used because we do not download for now
pub const EVENT_STARTED: &str = "started";
pub const EVENT_STOPPED: &str = "stopped";

const TORRENT_INFO_INTERVAL: u64 =1800; //1800s = 30min

lazy_static::lazy_static! {
    static ref RE_COMPLETE:   Regex = Regex::new("8:completei(\\d+)e").unwrap();
    static ref RE_INCOMPLETE: Regex = Regex::new("10:incompletei(\\d+)e").unwrap();
    static ref RE_INTERVAL:   Regex = Regex::new("8:intervali(\\d+)e").unwrap();
    //static ref RE_MIN_INTERVAL:   Regex = Regex::new("12:min intervali(\\d+)e").unwrap();
    //TODO: get torrent_id?
}

/// The tracker responds with "text/plain" document consisting of a bencoded dictionary
#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct FailureTrackerResponse {
    /// If present, then no other keys may be present. The value is a human-readable error message as to why the request failed
    #[serde(rename = "failure reason")] pub reason: String,
}

#[derive(Debug, PartialEq, Deserialize, Clone)]
pub struct Peer {
    /// A string of length 20 which this peer uses as its id. This field will be `None` for compact peer info.
    pub id: Option<String>,
    /// peer's IP address either IPv6 (hexed) or IPv4 (dotted quad) or DNS name (string)
    pub ip: String,
    /// peer's port number
    pub port: i64,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub struct OkTrackerResponse {
    /// (new, optional) Similar to failure reason, but the response still gets processed normally. The warning message is shown just like an error.
    #[serde(default, rename = "warning message")] pub warning_message: Option<String>,
    /// Interval in seconds that the client should wait between sending regular requests to the tracker
    pub interval: i64,
    /// (optional) Minimum announce interval. If present clients must not reannounce more frequently than this.
    #[serde(default, rename = "min interval")] pub min_interval: Option<i64>,
    /// A string that the client should send back on its next announcements. If absent and a previous announce sent a tracker id, do not discard the old value; keep using it.
    pub tracker_id: Option<String>,
    /// number of peers with the entire file, i.e. seeders
    pub complete: i64,
    /// number of non-seeder peers, aka "leechers"
    pub incomplete: i64,
    /// (dictionary model) The value is a list of dictionaries, each with the following keys.
    /// peers: (binary model) Instead of using the dictionary model described above, the peers value may be a string consisting of multiples of 6 bytes. First 4 bytes are the IP address and last 2 bytes are the port number. All in network (big endian) notation.
    #[serde(default, skip_deserializing)] peers: Option<u8>,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub enum TrackerResponse {
    Success {
        /// (new, optional) Similar to failure reason, but the response still gets processed normally. The warning message is shown just like an error.
        #[serde(default, rename = "warning message")] warning_message: Option<String>,
        /// Interval in seconds that the client should wait between sending regular requests to the tracker
        interval: i64,
        /// (optional) Minimum announce interval. If present clients must not reannounce more frequently than this.
        #[serde(default, rename = "min interval")] min_interval: Option<i64>,
        /// A string that the client should send back on its next announcements. If absent and a previous announce sent a tracker id, do not discard the old value; keep using it.
        tracker_id: Option<String>,
        /// number of peers with the entire file, i.e. seeders
        complete: i64,
        /// number of non-seeder peers, aka "leechers"
        incomplete: i64,
        /// (dictionary model) The value is a list of dictionaries, each with the following keys.
        /// peers: (binary model) Instead of using the dictionary model described above, the peers value may be a string consisting of multiples of 6 bytes. First 4 bytes are the IP address and last 2 bytes are the port number. All in network (big endian) notation.
        #[serde(default, skip_serializing)] peers: Option<u8>,
    },
    Failure {
        /// If present, then no other keys may be present. The value is a human-readable error message as to why the request failed
        #[serde(rename = "failure reason")] reason: String,
    }
}

pub fn _from_response(data: Vec<u8>, _encoding: &str) -> Result<TrackerResponse, serde_bencode::Error> {
    return serde_bencode::de::from_bytes::<TrackerResponse>(&data);
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Node(String, i64);

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct File {
    /// a list containing one or more string elements that together represent the path and filename. Each element in the list corresponds to 
    /// either a directory name or (in the case of the final element) the filename. For example, a the file "dir1/dir2/file.ext" would 
    /// consist of three string elements: "dir1", "dir2", and "file.ext". This is encoded as a bencoded list of strings such as l4:dir14:dir28:file.exte
    pub path: Vec<String>,
    /// length of the file in bytes (integer)
    pub length: i64,
    #[serde(default)] md5sum: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Info {
    pub name: String,
    pieces: ByteBuf,
    #[serde(rename = "piece length")] pub piece_length: i64,
    #[serde(default)] md5sum: Option<String>,
    #[serde(default)] pub length: Option<i64>,
    #[serde(default)] pub files: Option<Vec<File>>,
    #[serde(default)] pub private: Option<u8>,
    #[serde(default)] pub path: Option<Vec<String>>,
    #[serde(default, rename = "root hash")] pub root_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Torrent {
    pub info: Info,
    #[serde(default)] pub announce: Option<String>,
    #[serde(default)] nodes: Option<Vec<Node>>,
    #[serde(default)] pub encoding: Option<String>,
    #[serde(default)] httpseeds: Option<Vec<String>>,
    /// http://bittorrent.org/beps/bep_0012.html
    #[serde(default, rename = "announce-list")] pub announce_list: Option<Vec<Vec<String>>>,
    #[serde(default, rename = "creation date")] pub creation_date: Option<i64>,
    #[serde(rename = "comment")] pub comment: Option<String>,
    #[serde(default, rename = "created by")] pub created_by: Option<String>,
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
        if self.files().is_none() { return self.info.length.unwrap_or_default() as usize; }
        let mut total_size = 0;
        if let Some(files) = self.files() {
            for file in files { total_size += file.length; }
        }
        total_size as usize
    }

    pub fn info_hash(&self) -> Option<Vec<u8>> {
        let info = ser::to_bytes(&self.info);
        if info.is_err() {return None;}
        return Some(Sha1::digest(&info.unwrap()).to_vec());
    }
}

/// Store only essential information
#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct BasicTorrent {
    /// the filename. This is purely advisory. (string)
    pub name: String,
    /// (optional) free-form textual comments of the author (string)
    comment: String,
    /// (optional) name and version of the program used to create the .torrent (string)
    created_by: String,
    /// The announce URL of the tracker
    pub announce: Option<String>, 
    /// (optional) this is an extention to the official specification, offering backwards-compatibility. (list of lists of strings). http://bittorrent.org/beps/bep_0012.html
    announce_list: Option<Vec<Vec<String>>>,
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
    pub path: String,
    /// If we have to virtually download the torrent first, it is the downloaded size in bytes
    pub downloaded: usize,
    /// Total of fake uploaded data since the start of RatioUp
    pub uploaded: usize,
    /// Last announce to the tracker
    #[serde(skip_serializing)] pub last_announce: std::time::Instant,
    /// URL encoded hash thet is used to build the tracker query
    #[serde(skip_serializing)] pub info_hash_urlencoded: String,
    /// Number of seeders, it is used on the web UI
    pub seeders: u16,
    /// Number of leechers, it is used on the web UI
    pub leechers: u16,
    /// It is the next upload speed that will be announced. It is also used for UI display.
    pub next_upload_speed: u32,
    /// It is the next download speed that will be announced. It allows to end a complete event earlier than the normal interval, It is also used for UI display.
    pub next_download_speed: u32,
    /// Tracker announce URLs built from the config and the torrent. Some variables are still there (key, left, downloaded, uploaded, event)
    #[serde(skip)] pub urls: Vec<String>,
}

impl BasicTorrent {
    /// Called after a torrent is added to RatioUp or when RatioUp started (load torrents)
    /// It prepares the annonce query by replacing variables (port, numwant, ...) with the computed values
    pub fn prepare_urls(&mut self, query: String, port: u16, peer_id: String, numwant: u16) {
        let mut url= String::new();
        if self.announce.as_ref().is_some() {
            url = self.announce.clone().unwrap();
            url.push('?');
            url.push_str(&query);
        }
        url = url.replace("{peerid}", &peer_id).replace("{infohash}", &self.info_hash_urlencoded)
                 .replace("{numwant}", numwant.to_string().as_str()).replace("{port}", port.to_string().as_str());
        let _ = &self.urls.push(url);
    }
    /// Build the announce URLs for the listed trackers in the torrent file. FOR NOW IT DOES NOT HANDLE MULTIPLE URLS!
    pub fn build_urls(&mut self, event: &str, key: String) -> Vec<String> {
        tracing::info!("Torrent: {}", self.name);
        //compute downloads and uploads
        let elapsed: usize = if event == EVENT_STARTED {0} else {self.last_announce.elapsed().as_secs() as usize};
        let uploaded: usize = self.next_upload_speed as usize * elapsed;
        let mut downloaded: usize = self.next_download_speed as usize * elapsed;
        if self.length <= self.downloaded + downloaded {downloaded = self.length - self.downloaded;} //do not download more thant the torrent size
        self.downloaded += downloaded;

        //build URL list
        let mut urls: Vec<String> = Vec::new();
        let url = self.urls[0].replace("{infohash}", &self.info_hash_urlencoded).replace("{key}", &key)
                 .replace("{uploaded}", uploaded.to_string().as_str())
                 .replace("{downloaded}", downloaded.to_string().as_str()).replace("{left}", (self.length - self.downloaded).to_string().as_str())
                 .replace("{event}", event);
        tracing::info!("\tDownloaded: {} \t Uploaded: {} \t Annonce at: {}", byte_unit::Byte::from_bytes(downloaded as u128).get_appropriate_unit(true).to_string(), byte_unit::Byte::from_bytes(uploaded as u128).get_appropriate_unit(true).to_string(), url);
        urls.push(url);
        return urls;
    }

    pub fn announce(&mut self, event: &str, request: ureq::Request) -> u64 {
        let mut interval: u64 = 0;
        match request.call() {
            Ok(resp) => {
                let code = resp.status();
                let mut bytes: Vec<u8> = Vec::with_capacity(2048);
                resp.into_reader().take(1024).read_to_end(&mut bytes).expect("Cannot read response");
                //we start to check if the tracker has returned an error message, if yes, we will reannounce later
                let response = serde_bencode::de::from_bytes::<FailureTrackerResponse>(&bytes.clone());
                if response.is_ok() {
                    tracing::warn!("Announce error from the tracker: {}", response.unwrap().reason);
                    return TORRENT_INFO_INTERVAL;
                }
                let rawdata = String::from_utf8_lossy(&bytes);
                info!("RESPONSE: {:?}", rawdata);
                //dirty map with regex, because binary on response prevent the parsing
                let x = RE_COMPLETE.captures(&rawdata);
                self.seeders = if x.is_some() {x.unwrap().get(1).unwrap().as_str().parse().unwrap()} else {0};
                let x = RE_INCOMPLETE.captures(&rawdata);
                self.leechers = if x.is_some() {x.unwrap().get(1).unwrap().as_str().parse().unwrap()} else {0};
                let x = RE_INTERVAL.captures(&rawdata);
                interval = if x.is_some() {x.unwrap().get(1).unwrap().as_str().parse().unwrap()} else {120};
                info!("\tSeeders: {}\tLeechers: {}\t\t\tInterval: {:?}s", self.seeders, self.leechers, interval);
                if code != actix_web::http::StatusCode::OK {info!("\tResponse: code={}\tdata={:?}", code, response);}
                if event != EVENT_STOPPED {return TORRENT_INFO_INTERVAL;}
            }
            Err(ureq::Error::Status(code, response)) => {warn!("\tUnexpected server response status: {}\t{:?}", code, response); } //the server returned an unexpected status code (such as 400, 500 etc)
            Err(_) => {if event != EVENT_STOPPED {error!("I/O error while announcing");}}
        }
        return interval;
    }
}

/// Load essential data from a parsed torrent using the full parsed torrent file. It reduces the RAM use to have smaller data
pub fn from_torrent(torrent: Torrent, path: String) -> BasicTorrent {
    let hash_bytes = torrent.info_hash().expect("Cannot get torrent info hash");
    let hash = hash_bytes.encode_hex::<String>();
    //let hash = hash_bytes.???;
    let private = if torrent.info.private.is_some() && torrent.info.private == Some(1) {true} else {false};
    let size = torrent.total_size();
    let mut t= BasicTorrent {path: path, name: torrent.info.name, announce: torrent.announce.clone(), announce_list: torrent.announce_list.clone(), info_hash_urlencoded: String::with_capacity(64),
        comment: String::new(), length: size, created_by: String::new(), last_announce: std::time::Instant::now(), urls: Vec::new(),
        info_hash: hash, piece_length: torrent.info.piece_length as usize, private: private, files: None, downloaded: size, uploaded: 0,
        seeders: 0, leechers: 0, next_upload_speed: 0, next_download_speed: 0};
    t.info_hash_urlencoded = byte_serialize(&hash_bytes).collect();
    if torrent.info.files.is_some() {
        let files = torrent.info.files.unwrap();
        let mut list : Vec<File> = Vec::with_capacity(files.len());
        for f in files {
            list.push(File {length: f.length, path: f.path, md5sum: None});
        }
        t.files = Some(list);
    }
    return t;
}

pub fn from_file(path: String) -> Result<Torrent, serde_bencode::Error> {
    let data=std::fs::read(path).expect("Cannot read torrent file");
    return serde_bencode::de::from_bytes::<Torrent>(&data);
}
