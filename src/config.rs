use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;
use byte_unit::Byte;
use tracing::{info, error};
use rand::Rng;
use url::form_urlencoded::byte_serialize;
use fake_torrent_client;

//refresh interval
const NEVER: u8 = 0;
const TIMED_OR_AFTER_STARTED_ANNOUNCE: u8 = 1;
const TORRENT_VOLATILE: u8 = 2;
const TORRENT_PERSISTENT: u8 = 3;
