// https://wiki.theory.org/BitTorrentSpecification#Metainfo_File_Structure
extern crate serde;
use url::form_urlencoded::byte_serialize;
use serde::Serialize;


#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct File {
    /// a list containing one or more string elements that together represent the path and filename. Each element in the list corresponds to 
    /// either a directory name or (in the case of the final element) the filename. For example, a the file "dir1/dir2/file.ext" would 
    /// consist of three string elements: "dir1", "dir2", and "file.ext". This is encoded as a bencoded list of strings such as l4:dir14:dir28:file.exte
    path: String, //Vec<String>,
    /// length of the file in bytes (integer)
    length: usize, //i64,
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
    ///If the torrent is active: so we announce it at the defined interval
    pub active: bool,
    /// If we have to virtually download the torrent first, it is the downloaded size in bytes
    pub downloaded: usize,
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
}

impl BasicTorrent {
    /// Load essential data from a parsed torrent using the lava_torrent lib
    pub fn from_torrent(torrent: lava_torrent::torrent::v1::Torrent, path: String) -> BasicTorrent {
        let hash = torrent.info_hash();
        let hash_bytes = torrent.info_hash_bytes();
        let private = torrent.is_private();
        let mut t= BasicTorrent {path: path, name: torrent.name, announce: torrent.announce.clone(), announce_list: torrent.announce_list.clone(), info_hash_urlencoded: String::with_capacity(64),
            comment: String::new(), active: true, length: torrent.length as usize, created_by: String::new(), last_announce: std::time::Instant::now(),
            info_hash: hash, piece_length: torrent.piece_length as usize, private: private, files: None, downloaded: torrent.length as usize,
            seeders: 0, leechers: 0, next_upload_speed: 0, next_download_speed: 0};
        t.info_hash_urlencoded = byte_serialize(&hash_bytes).collect();
        if torrent.files.is_some() {
            let files = torrent.files.unwrap();
            let mut list : Vec<File> = Vec::with_capacity(files.len());
            for f in files {
                list.push(File {path: f.path.into_os_string().into_string().expect("Cannot get a file in the torrent"), length: f.length as usize});
            }
            t.files = Some(list);
        }
        return t;
    }
}
