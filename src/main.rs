use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{self, error, info, warn};
use utils::format_bytes;

use crate::config::Config;
use crate::context::AppContext;
use crate::service::announce::{AnnounceService, run_scheduler};
use crate::service::torrents::{TorrentService, prepare_dir};

mod announcer;
pub mod bencode;
mod config;
mod context;
pub mod json_output;
mod repository;
mod service;
pub mod torrent;
mod utils;
mod watcher;

async fn run_key_renewer(ctx: Arc<AppContext>, refresh_every: u16) {
    loop {
        ctx.client.write().await.generate_key();
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
                warn!("Unknown argument: {}, ignoring", other_arg);
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
        .with_max_level(tracing::Level::INFO)
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

    let ctx = Arc::new(AppContext::new(config));

    // schedule client refresh key if applicable
    let key_refresh_every = ctx.client.read().await.key_refresh_every;
    if let Some(refresh_every) = key_refresh_every {
        tokio::spawn(run_key_renewer(Arc::clone(&ctx), refresh_every));
    }

    let torrent_dir = ctx.config.torrent_dir.clone();
    prepare_dir(&torrent_dir).await;
    match TorrentService::new(&ctx).load_dir(&torrent_dir) {
        Ok(0) => {
            info!("No torrent, exiting");
            return;
        }
        Ok(_) => {}
        Err(e) => {
            error!(
                "Cannot read torrent directory {}: {e}",
                torrent_dir.display()
            );
            return;
        }
    }
    let pid_file: Option<PathBuf> = if ctx.config.use_pid_file {
        write_pid_file().await
    } else {
        None
    };
    let wait_time = AnnounceService::new(&ctx).start_all().await;

    // Start file watcher for dynamic torrent management
    tokio::spawn(watcher::watch_directory(Arc::clone(&ctx), torrent_dir));

    let shutdown_ctx = Arc::clone(&ctx);
    tokio::spawn(async move {
        // graceful exit when Ctrl + C / SIGINT
        tokio::signal::ctrl_c().await.unwrap();
        info!("Exiting...");
        AnnounceService::new(&shutdown_ctx).stop_all().await;
        remove_pid_file(pid_file).await;
        std::process::exit(0);
    });

    run_scheduler(ctx, wait_time).await;
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

    #[tokio::test]
    async fn test_remove_pid_file_deletes_file() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("ratio_up.pid");
        std::fs::write(&pid_file, b"1234").unwrap();

        remove_pid_file(Some(pid_file.clone())).await;
        assert!(!pid_file.exists());
    }

    #[tokio::test]
    async fn test_remove_pid_file_none_or_missing_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        remove_pid_file(None).await;
        remove_pid_file(Some(dir.path().join("missing.pid"))).await;
    }
}
