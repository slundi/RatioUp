// https://wiki.theory.org/BitTorrentSpecification#Metainfo_File_Structure
extern crate serde;

use serde::Serialize;
use lava_torrent::torrent::v1::Torrent;

#[derive(Debug, Serialize, PartialEq, Clone)]
pub struct File {
    path: String, //Vec<String>,
    length: usize, //i64,
    //#[serde(default)] md5sum: Option<String>,
}

/*#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Node(String, i64);



#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Info {
    name: String,
    pieces: ByteBuf,
    #[serde(rename = "piece length", skip_serializing)] piece_length: i64,
    #[serde(default)] md5sum: Option<String>,
    /// Total torrent size?
    #[serde(default)] length: Option<i64>,
    /// Files in the torrent
    #[serde(default)] files: Option<Vec<File>>,
    #[serde(default)] pub private: Option<u8>,
    #[serde(default)] path: Option<Vec<String>>,
    #[serde(default, rename = "root hash")] root_hash: Option<String>,
    #[serde(skip)] pub info_hash: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Torrent {
    pub info: Info,
    #[serde(default)] announce: Option<String>,
    #[serde(default, skip_serializing)] nodes: Option<Vec<Node>>,
    #[serde(default)] encoding: Option<String>,
    #[serde(default, skip_serializing)] httpseeds: Option<Vec<String>>,
    #[serde(default, rename = "announce-list")] announce_list: Option<Vec<Vec<String>>>,
    #[serde(default, rename = "creation date")] creation_date: Option<i64>,
    #[serde(rename = "comment")] comment: Option<String>,
    #[serde(default, rename = "created by")] created_by: Option<String>,
    #[serde(skip)] pub active: bool,
}*/

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
    //pub fn is_private(&self) -> bool {return self.info.private.is_some() && self.info.private == Some(1);}
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
