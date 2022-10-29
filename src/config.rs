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


// impl crate::Config {
//     /// Get the HTTP request with the bittorrent client headers (user-agent, accept, accept-encoding, accept-language)
//     pub fn get_http_request(&self, url: &str) -> ureq::Request {
//         let mut agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(60));
//         if !self.user_agent.is_empty() {agent = agent.user_agent(&self.user_agent);}
//         let mut req = agent.build().get(url);
//         if !self.accept.is_empty() {req = req.set("accept", &self.accept);}
//         if !self.accept_encoding.is_empty() {req = req.set("accept-encoding", &self.accept_encoding);}
//         if !self.accept_language.is_empty() {req = req.set("accept-language", &self.accept_language);}
//         req.timeout(std::time::Duration::from_secs(90))
//     }
// }

// pub fn get_config(path: &str) -> Config {
//     //key generator
//     if v["keyGenerator"]["refreshEvery"].is_u64() {cfg.key_refresh_every = v["keyGenerator"]["refreshEvery"].as_u64().unwrap() as u16;}
//     cfg
// }
