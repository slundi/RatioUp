use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{self, error, info, warn};
use utils::format_bytes;

use crate::config::Config;
use crate::context::AppContext;
use crate::service::announce::{AnnounceService, run_scheduler};
use crate::service::torrents::{TorrentService, prepare_dir};

pub mod bencode;
mod config;
mod context;
mod infra;
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

/// What the CLI arguments ask the program to do.
#[derive(Debug, PartialEq)]
enum CliAction {
    /// Run the daemon, with an optional config file path
    Run(Option<PathBuf>),
    /// Print the version and exit
    Version,
}

/// Parse CLI args (program name already skipped). Only a config file or a
/// version request can be there.
fn parse_cli_args(args: impl IntoIterator<Item = String>) -> CliAction {
    let mut args = args.into_iter();

    // Manually parse arguments
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-V" | "--version" => return CliAction::Version,
            "-c" | "--config" => {
                if let Some(path_str) = args.next() {
                    return CliAction::Run(Some(PathBuf::from(path_str)));
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
    CliAction::Run(None)
}

fn get_config_from_xdg() -> Option<PathBuf> {
    let xdg = xdg::BaseDirectories::with_prefix("RatioUp");
    match xdg.place_config_file("config.toml") {
        Ok(path) => return Some(path),
        Err(error) => tracing::error!("Cannot create config file: {error}"),
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
    let mut config_path: Option<PathBuf> = match parse_cli_args(std::env::args().skip(1)) {
        CliAction::Version => {
            println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
            return;
        }
        CliAction::Run(path) => path,
    };
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
        Err(error) => {
            error!(
                "Cannot read torrent directory {}: {error}",
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
                Err(error) => {
                    warn!("Cannot create PID file: {error}");
                    None
                }
            }
        }
        Err(error) => {
            warn!("Cannot create PID file: {error}");
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

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn test_parse_cli_args_version() {
        assert_eq!(parse_cli_args(args(&["--version"])), CliAction::Version);
        assert_eq!(parse_cli_args(args(&["-V"])), CliAction::Version);
        assert_eq!(
            parse_cli_args(args(&["--unknown", "--version"])),
            CliAction::Version
        );
    }

    #[test]
    fn test_parse_cli_args_config() {
        assert_eq!(
            parse_cli_args(args(&["-c", "my.toml"])),
            CliAction::Run(Some(PathBuf::from("my.toml")))
        );
        assert_eq!(
            parse_cli_args(args(&["--config", "my.toml"])),
            CliAction::Run(Some(PathBuf::from("my.toml")))
        );
    }

    #[test]
    fn test_parse_cli_args_defaults_to_run_without_config() {
        assert_eq!(parse_cli_args(args(&[])), CliAction::Run(None));
        assert_eq!(parse_cli_args(args(&["--unknown"])), CliAction::Run(None));
        assert_eq!(parse_cli_args(args(&["--config"])), CliAction::Run(None));
    }

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
