//use lava_torrent::torrent::v1::Torrent;

extern crate serde;
extern crate serde_bencode;

use serde_bytes::ByteBuf;

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Node(String, i64);

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct File {
    path: Vec<String>,
    length: i64,
    //#[serde(default)] md5sum: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct Info {
    name: String,
    //#[serde(skip_serializing)] pieces: ByteBuf,
    #[serde(rename = "piece length")] piece_length: i64,
    //#[serde(default)] md5sum: Option<String>,
    /// Total torrent size?
    #[serde(default)] length: Option<i64>,
    /// Files in the torrent
    #[serde(default)] files: Option<Vec<File>>,
    #[serde(default)] pub private: Option<u8>,
    #[serde(default)] path: Option<Vec<String>>,
    #[serde(default, rename = "root hash")] root_hash: Option<String>,
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
}

impl  Torrent {
    pub fn is_private(&self) -> bool {
        if self.info.private.is_some() && self.info.private != Some(0) {return true;}
        false
    }
}
