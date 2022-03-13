// https://wiki.theory.org/BitTorrentSpecification#Metainfo_File_Structure
extern crate serde;

use serde::Serialize;
use lava_torrent::torrent::v1::Torrent;

#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct File {
    path: String, //Vec<String>,
    length: usize, //i64,
}

/// Store only essential information
#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct BasicTorrent {
    pub path: String,
    name: String, comment: String, created_by: String,
    pub announce: Option<String>,  announce_list: Option<Vec<Vec<String>>>,
    //creation_date?
    pub info_hash: String,
    piece_length: usize, length: usize,
    files: Option<Vec<File>>,
    pub private: bool,
    pub active: bool,
    pub downloaded: usize,
}

impl BasicTorrent {
    /// Load essential data from a parsed torrent using the lava_torrent lib
    pub fn from_torrent(torrent: Torrent, path: String) -> BasicTorrent {
        let hash = torrent.info_hash();
        let private = torrent.is_private();
        let mut t= BasicTorrent {path: path, name: torrent.name, announce: torrent.announce.clone(), announce_list: torrent.announce_list.clone(),
            comment: String::new(), active: true, length: torrent.length as usize, created_by: String::new(),
            info_hash: hash, piece_length: torrent.piece_length as usize, private: private, files: None, downloaded: torrent.length as usize};
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
