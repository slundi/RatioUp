#![allow(non_snake_case)]

use fake_torrent_client::Client;
use std::path::PathBuf;
use tokio::sync::{Mutex, OnceCell, RwLock};
use tokio::time::Duration;
use tracing::{self, error, info, warn};
use utils::format_bytes;

use crate::announcer::scheduler::run as run_announcer;
use crate::config::Config;
use crate::torrent::Torrent;

mod announcer;
mod config;
mod directory;
pub mod json_output;
pub mod torrent;
mod utils;

static STARTED: OnceCell<chrono::DateTime<chrono::Utc>> = OnceCell::const_new();
static CONFIG: OnceCell<Config> = OnceCell::const_new();
static CLIENT: RwLock<Option<Client>> = RwLock::const_new(None);
static TORRENTS: RwLock<Vec<Mutex<Torrent>>> = RwLock::const_new(Vec::new()); // TODO: replace with mutex

async fn run_key_renewer(refresh_every: u16) {
    loop {
        if let Some(client) = &mut *CLIENT.write().await {
            client.generate_key();
        }
        // std::thread::sleep(Duration::from_secs(u64::from(refresh_every)));
        tokio::time::sleep(Duration::from_secs(u64::from(refresh_every))).await;
    }
}

/// Parse CLI args. Only a config file can be there.
fn parse_cli_args() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1); // Skip the program name

    // Manually parse arguments
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                if let Some(path_str) = args.next() {
                    return Some(PathBuf::from(path_str));
                } else {
                    tracing::error!("Missing value for -c/--config");
                }
            }
            // Handle other arguments or positional arguments here
            other_arg => {
                tracing::error!("Warning: Unknown argument: {}, Ignoring", other_arg);
            }
        }
    }
    None
}

fn get_config_from_xdg() -> Option<PathBuf> {
    let xdg = xdg::BaseDirectories::with_prefix("RatioUp");
    match xdg.place_config_file("config.toml") {
        Ok(path) => return Some(path),
        Err(e) => tracing::error!("Cannot create config file: {e}"),
    }
    None
}

#[tokio::main]
async fn main() {
    //configure logger
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_level(true)
        .with_target(false)
        .init();

    // get config path if possible
    let mut config_path: Option<PathBuf> = parse_cli_args();
    if config_path.is_none() {
        config_path = get_config_from_xdg();
    }

    // load config from file or default
    let config = if let Some(path) = config_path {
        tracing::info!("Loading configuration from {}", path.display());
        Config::load_from_file(&path).await
    } else {
        tracing::info!("Loading default configuration");
        Config::default()
    };

    info!(
        "Upload bandwidth: \u{2191} {} - {}",
        format_bytes(config.min_upload_rate),
        format_bytes(config.max_upload_rate)
    );

    if let Err(e) = CONFIG.set(config.clone()) {
        error!("Cannot set config: {e}");
        return;
    }
    if let Err(e) = STARTED.set(chrono::offset::Utc::now()) {
        error!("Cannot set start time: {e}");
        return;
    }

    // schedule client refresh key if applicable
    if let Some(refresh_every) = config::init_client(&config).await {
        let _ = std::thread::Builder::new()
            .name("ratioup-key-renewer".to_owned())
            .spawn(move || run_key_renewer(refresh_every));
    }

    directory::prepare_torrent_folder(config.torrent_dir.clone()).await;
    let count = directory::load_torrents(config.torrent_dir).await;
    if count == 0 {
        info!("No torrent, exiting");
        return;
    }
    let mut pid_file: Option<PathBuf> = None;
    if config.use_pid_file {
        // Create PID file
        pid_file = write_pid_file().await;
    }
    let wait_time = announcer::tracker::announce_started().await;

    tokio::spawn(async move {
        // graceful exit when Ctrl + C / SIGINT
        tokio::signal::ctrl_c().await.unwrap();
        info!("Exiting...");
        announcer::tracker::announce_stopped().await;
        if config.use_pid_file && pid_file.is_some() {
            remove_pid_file(pid_file).await;
        }
        std::process::exit(0);
    });

    run_announcer(wait_time).await;
}

async fn write_pid_file() -> Option<PathBuf> {
    match xdg::BaseDirectories::new().place_runtime_file("ratio_up.pid") {
        Ok(file) => {
            match tokio::fs::write(file.clone(), std::process::id().to_string().as_bytes()).await {
                Ok(_) => Some(file),
                Err(e) => {
                    warn!("Cannot create PID file: {e}");
                    None
                }
            }
        }
        Err(e) => {
            warn!("Cannot create PID file: {e}");
            None
        }
    }
}

async fn remove_pid_file(pid_file: Option<PathBuf>) {
    if let Some(path) = pid_file {
        let _ = tokio::fs::remove_file(path).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    /// Test if it creates the torrent directory and do not panic when it exists
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_torrent_directory() {
        let mut dir = env::temp_dir();
        dir.push("ratioup-test-torrents-dir");
        if dir.is_dir() {
            let _ = std::fs::remove_dir(dir.clone());
        }
        directory::prepare_torrent_folder(dir.clone()).await;
        assert!(dir.is_dir());
        directory::prepare_torrent_folder(dir).await;
    }
}
