//! Announce use cases: contacting trackers for one torrent, for all torrents at
//! startup and shutdown, and the periodic scheduler loop.

use std::sync::Arc;

use tokio::task::JoinSet;
use tokio::time::Duration;
use tracing::{debug, info, warn};

use crate::announcer::tracker::{Event, announce_http};
use crate::announcer::udp::announce_udp;
use crate::context::AppContext;
use crate::service::stats::StatsService;
use crate::torrent::Torrent;
use crate::utils::format_bytes_u64;

/// Use cases that talk to trackers.
pub struct AnnounceService<'ctx> {
    ctx: &'ctx AppContext,
}

impl<'ctx> AnnounceService<'ctx> {
    pub fn new(ctx: &'ctx AppContext) -> Self {
        Self { ctx }
    }

    /// Announce one torrent to every tracker URL it lists.
    ///
    /// The upload speed reported in the following announce is picked first.
    /// The tracker may not be contacted more often than the minimum interval
    /// returned in the first announce response.
    pub async fn announce(&self, torrent: &mut Torrent, event: Option<Event>) {
        let config = &self.ctx.config;
        // TODO: prepare announce (uploaded and downloaded if applicable)
        torrent.compute_speeds(config.min_upload_rate, config.max_upload_rate);
        let client = self.ctx.client.read().await;
        debug!("Torrent has {} url(s)", torrent.urls.len());
        for url in torrent.urls.clone() {
            debug!("\t{}", url);
            if url.to_lowercase().starts_with("udp://") {
                announce_udp(&url, torrent, &client, config, event).await;
            } else {
                announce_http(&self.ctx.http, config, &url, torrent, &client, event).await;
            }
        }
        info!(
            "Announced: interval={}, event={:?}, downloaded=0, uploaded={}, seeders={}, leechers={}, torrent={}",
            torrent.interval,
            event,
            torrent.uploaded,
            torrent.seeders,
            torrent.leechers,
            torrent.name
        );
    }

    /// Announce every torrent with the STARTED event.
    ///
    /// Returns the smallest interval given by the trackers (`u64::MAX` without torrents).
    pub async fn start_all(&self) -> u64 {
        info!("Announcing torrent(s) with STARTED event");
        let mut wait_time = u64::MAX;
        for handle in self.ctx.torrents.snapshot() {
            let mut torrent = handle.lock().await;
            self.announce(&mut torrent, Some(Event::Started)).await;
            wait_time = wait_time.min(torrent.interval);
            debug!("Time: {}", wait_time);
        }
        wait_time
    }

    /// Announce every torrent with the STOPPED event and log the session summary.
    ///
    /// Returns the total uploaded during the session, in bytes.
    pub async fn stop_all(&self) -> u64 {
        info!("Announcing torrent(s) with STOPPED event");
        let mut total_uploaded: u64 = 0;
        for handle in self.ctx.torrents.snapshot() {
            let mut torrent = handle.lock().await;
            self.announce(&mut torrent, Some(Event::Stopped)).await;
            total_uploaded += torrent.uploaded;
            info!(
                "Torrent \"{}\": uploaded={}, seeders={}, leechers={}, errors={}",
                torrent.name,
                format_bytes_u64(torrent.uploaded),
                torrent.seeders,
                torrent.leechers,
                torrent.error_count
            );
        }
        info!(
            "Session total: {} torrents, uploaded={}",
            self.ctx.torrents.len(),
            format_bytes_u64(total_uploaded)
        );
        total_uploaded
    }
}

/// Add jitter (±5%) to an interval to prevent thundering herd effect.
/// Multiple torrents with similar intervals will announce at slightly different times.
fn add_jitter(interval: u64) -> u64 {
    if interval < 20 {
        // Don't add jitter to very short intervals
        return interval;
    }
    // Calculate 5% of the interval
    let jitter_range = interval / 20; // 5%
    // Random offset between -jitter_range and +jitter_range
    let offset = fastrand::u64(0..=jitter_range * 2);
    interval.saturating_sub(jitter_range).saturating_add(offset)
}

/// Seconds remaining until this torrent is due for its next announce.
fn time_until_announce(torrent: &Torrent) -> u64 {
    let elapsed = torrent.last_announce.elapsed().as_secs();
    torrent.effective_interval().saturating_sub(elapsed)
}

/// Announce all due torrents concurrently and return the number of seconds
/// until the next torrent is due. Falls back to `fallback_interval` when
/// there are no torrents or every torrent has a zero interval.
///
/// Each torrent is announced in its own task, hence the owned context.
async fn announce_due(ctx: &Arc<AppContext>, fallback_interval: u64) -> u64 {
    let mut join_set: JoinSet<u64> = JoinSet::new();

    for handle in ctx.torrents.snapshot() {
        let task_ctx = Arc::clone(ctx);
        join_set.spawn(async move {
            let mut torrent = handle.lock().await;
            if torrent.should_announce() {
                AnnounceService::new(&task_ctx)
                    .announce(&mut torrent, None)
                    .await;
            }
            time_until_announce(&torrent)
        });
    }

    let mut min_secs = u64::MAX;
    while let Some(result) = join_set.join_next().await {
        match result {
            Ok(secs) => min_secs = min_secs.min(secs),
            Err(e) => warn!("Announce task panicked: {e}"),
        }
    }

    if min_secs == u64::MAX || min_secs == 0 {
        fallback_interval
    } else {
        add_jitter(min_secs)
    }
}

/// Scheduler loop: announce due torrents, refresh the stats file, sleep until the next one is due.
pub async fn run_scheduler(ctx: Arc<AppContext>, wait_time: u64) {
    info!("Starting scheduler");
    loop {
        let next_interval = announce_due(&ctx, wait_time).await;
        debug!("Next announce in {}s", next_interval);
        StatsService::new(&ctx).write().await;
        tokio::time::sleep(Duration::from_secs(next_interval)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_torrent(interval: u64, min_interval: Option<u64>) -> Torrent {
        Torrent {
            name: String::from("test"),
            urls: vec![String::from("http://t.example.com/announce")],
            length: 1024,
            private: false,
            uploaded: 0,
            last_announce: std::time::Instant::now(),
            info_hash: [0u8; 20],
            info_hash_urlencoded: String::new(),
            seeders: 0,
            leechers: 0,
            next_upload_speed: 0,
            interval,
            error_count: 0,
            encoding: None,
            min_interval,
            tracker_id: None,
            source_path: None,
        }
    }

    // --- time_until_announce ---

    #[test]
    fn test_time_until_announce_full_interval_remaining() {
        // Fresh torrent: elapsed ≈ 0 → remaining ≈ interval
        let t = make_torrent(1800, None);
        let remaining = time_until_announce(&t);
        assert!(
            (1799..=1800).contains(&remaining),
            "expected ≈1800, got {remaining}"
        );
    }

    #[test]
    fn test_time_until_announce_zero_interval() {
        let t = make_torrent(0, None);
        assert_eq!(time_until_announce(&t), 0);
    }

    #[test]
    fn test_time_until_announce_min_interval_dominates() {
        // min_interval(3600) > interval(1800) → effective = 3600
        let t = make_torrent(1800, Some(3600));
        let remaining = time_until_announce(&t);
        assert!(
            (3599..=3600).contains(&remaining),
            "expected ≈3600, got {remaining}"
        );
    }

    #[test]
    fn test_time_until_announce_saturates_at_zero() {
        // interval=0, min_interval=None → saturating_sub(big, 0) = 0, not negative
        let mut t = make_torrent(0, None);
        // Backdate last_announce by simulating an old announce
        t.last_announce = std::time::Instant::now() - std::time::Duration::from_secs(9999);
        assert_eq!(time_until_announce(&t), 0);
    }

    // --- AnnounceService (torrents without tracker URL → no network) ---

    fn offline_torrent(hash_byte: u8, interval: u64, uploaded: u64) -> Torrent {
        let mut torrent = make_torrent(interval, None);
        torrent.info_hash = [hash_byte; 20];
        torrent.urls.clear();
        torrent.uploaded = uploaded;
        torrent
    }

    #[tokio::test]
    async fn test_announce_without_urls_only_updates_speed() {
        let config = crate::config::Config {
            min_upload_rate: 100,
            max_upload_rate: 200,
            ..crate::config::Config::default()
        };
        let ctx = AppContext::new(config);
        let mut torrent = offline_torrent(1, 1800, 0);
        torrent.seeders = 1;
        torrent.leechers = 1;
        AnnounceService::new(&ctx)
            .announce(&mut torrent, None)
            .await;
        assert!((100..200).contains(&torrent.next_upload_speed));
        assert_eq!(torrent.error_count, 0);
    }

    #[tokio::test]
    async fn test_start_all_empty_repository() {
        let ctx = AppContext::new(crate::config::Config::default());
        assert_eq!(AnnounceService::new(&ctx).start_all().await, u64::MAX);
    }

    #[tokio::test]
    async fn test_start_all_returns_smallest_interval() {
        let ctx = AppContext::new(crate::config::Config::default());
        ctx.torrents.insert_if_absent(offline_torrent(1, 1800, 0));
        ctx.torrents.insert_if_absent(offline_torrent(2, 900, 0));
        assert_eq!(AnnounceService::new(&ctx).start_all().await, 900);
    }

    #[tokio::test]
    async fn test_stop_all_empty_repository() {
        let ctx = AppContext::new(crate::config::Config::default());
        assert_eq!(AnnounceService::new(&ctx).stop_all().await, 0);
    }

    #[tokio::test]
    async fn test_stop_all_returns_session_total_and_keeps_torrents() {
        let ctx = AppContext::new(crate::config::Config::default());
        ctx.torrents.insert_if_absent(offline_torrent(1, 1800, 100));
        ctx.torrents.insert_if_absent(offline_torrent(2, 1800, 250));
        assert_eq!(AnnounceService::new(&ctx).stop_all().await, 350);
        // Stopping announces does not remove torrents from the repository.
        assert_eq!(ctx.torrents.len(), 2);
    }

    #[tokio::test]
    async fn test_announce_due_announces_due_torrent_and_falls_back() {
        let config = crate::config::Config {
            min_upload_rate: 100,
            max_upload_rate: 200,
            ..crate::config::Config::default()
        };
        let ctx = Arc::new(AppContext::new(config));
        // interval 0 → always due; remaining time 0 → fallback interval is used.
        let mut torrent = offline_torrent(1, 0, 0);
        torrent.seeders = 1;
        torrent.leechers = 1;
        ctx.torrents.insert_if_absent(torrent);

        assert_eq!(announce_due(&ctx, 42).await, 42);
        // The announce ran: an upload speed was picked from the configured range.
        let speed = ctx.torrents.snapshot()[0].lock().await.next_upload_speed;
        assert!((100..200).contains(&speed), "speed {speed} not announced");
    }

    // --- announce_due (no torrent due → no network) ---

    #[tokio::test]
    async fn test_announce_due_empty_repository_uses_fallback() {
        let ctx = Arc::new(AppContext::new(crate::config::Config::default()));
        assert_eq!(announce_due(&ctx, 42).await, 42);
    }

    #[tokio::test]
    async fn test_announce_due_returns_time_until_next_torrent() {
        let ctx = Arc::new(AppContext::new(crate::config::Config::default()));
        let mut soon = make_torrent(600, None);
        soon.info_hash = [1u8; 20];
        let mut later = make_torrent(1800, None);
        later.info_hash = [2u8; 20];
        ctx.torrents.insert_if_absent(soon);
        ctx.torrents.insert_if_absent(later);

        // Neither torrent is due: the smallest remaining time (≈600s ± 5% jitter) wins.
        let next = announce_due(&ctx, 42).await;
        assert!((569..=630).contains(&next), "expected ≈600, got {next}");
        // Nothing was announced, so no error was recorded.
        for handle in ctx.torrents.snapshot() {
            assert_eq!(handle.lock().await.error_count, 0);
        }
    }

    // --- parallel execution via JoinSet ---

    #[tokio::test]
    async fn test_joinset_tasks_run_concurrently() {
        use std::sync::Arc as StdArc;
        use std::sync::atomic::{AtomicU32, Ordering};

        const TASK_COUNT: u32 = 5;
        const TASK_SLEEP_MS: u64 = 60;

        let peak_concurrent = StdArc::new(AtomicU32::new(0));
        let running = StdArc::new(AtomicU32::new(0));

        let mut join_set: JoinSet<u32> = JoinSet::new();
        for _ in 0..TASK_COUNT {
            let peak = StdArc::clone(&peak_concurrent);
            let run = StdArc::clone(&running);
            join_set.spawn(async move {
                let current = run.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(current, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(TASK_SLEEP_MS)).await;
                run.fetch_sub(1, Ordering::SeqCst);
                current
            });
        }

        let start = std::time::Instant::now();
        while join_set.join_next().await.is_some() {}
        let elapsed_ms = start.elapsed().as_millis();

        // Serial execution would take TASK_COUNT × TASK_SLEEP_MS = 300 ms.
        // Parallel execution should finish in roughly TASK_SLEEP_MS ≈ 60 ms.
        // Allow 3× headroom for slow CI.
        assert!(
            elapsed_ms < (TASK_COUNT as u128 * TASK_SLEEP_MS as u128) / 2,
            "Tasks should run in parallel; {TASK_COUNT} × {TASK_SLEEP_MS}ms took {elapsed_ms}ms"
        );
        assert!(
            peak_concurrent.load(Ordering::SeqCst) >= 2,
            "Expected at least 2 tasks to overlap"
        );
    }

    // --- add_jitter (existing) ---

    #[test]
    fn test_add_jitter_short_interval() {
        assert_eq!(add_jitter(10), 10);
        assert_eq!(add_jitter(19), 19);
        assert_eq!(add_jitter(0), 0);
    }

    #[test]
    fn test_add_jitter_bounds() {
        let interval = 1000u64;
        let min_expected = 950;
        let max_expected = 1050;
        for _ in 0..100 {
            let result = add_jitter(interval);
            assert!(
                result >= min_expected && result <= max_expected,
                "Jitter {} out of bounds [{}, {}]",
                result,
                min_expected,
                max_expected
            );
        }
    }

    #[test]
    fn test_add_jitter_typical_tracker_interval() {
        let interval = 1800u64;
        let min_expected = 1710;
        let max_expected = 1890;
        for _ in 0..100 {
            let result = add_jitter(interval);
            assert!(
                result >= min_expected && result <= max_expected,
                "Jitter {} out of bounds [{}, {}]",
                result,
                min_expected,
                max_expected
            );
        }
    }
}
