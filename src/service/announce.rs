//! Announce use cases: contacting trackers for one torrent, for all torrents at
//! startup and shutdown, and the periodic scheduler loop.

use std::sync::Arc;

use tokio::task::JoinSet;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};

use crate::config::Config;
use crate::context::AppContext;
use crate::infra::tracker::{
    AnnounceError, AnnounceRequest, AnnounceResponse, DEFAULT_NUMWANT, Event,
};
use crate::service::stats::StatsService;
use crate::torrent::Torrent;
use crate::utils::format_bytes_u64;

/// Bytes to report as uploaded since the last announce (none on STARTED).
fn uploaded_since_last_announce(torrent: &Torrent, event: Option<Event>) -> u64 {
    if event == Some(Event::Started) {
        return 0;
    }
    u64::from(torrent.next_upload_speed) * torrent.last_announce.elapsed().as_secs()
}

fn build_request(torrent: &Torrent, event: Option<Event>, config: &Config) -> AnnounceRequest {
    AnnounceRequest {
        info_hash: torrent.info_hash,
        info_hash_urlencoded: torrent.info_hash_urlencoded.clone(),
        uploaded: uploaded_since_last_announce(torrent, event),
        event,
        port: config.port,
        numwant: config.numwant.unwrap_or(DEFAULT_NUMWANT),
    }
}

/// Update the torrent after a successful announce of `uploaded` bytes.
fn apply_response(torrent: &mut Torrent, uploaded: u64, response: AnnounceResponse) {
    if let Some(warning) = response.warning {
        warn!("Announce with warning: {warning:?}");
    }
    if let Some(interval) = response.interval {
        torrent.interval = interval;
    }
    if let Some(min_interval) = response.min_interval {
        torrent.min_interval = Some(min_interval);
    }
    if let Some(tracker_id) = response.tracker_id {
        torrent.tracker_id = Some(tracker_id);
    }
    if let Some(seeders) = response.seeders {
        torrent.seeders = seeders;
    }
    if let Some(leechers) = response.leechers {
        torrent.leechers = leechers;
    }
    torrent.uploaded += uploaded;
    torrent.last_announce = std::time::Instant::now();
    torrent.error_count = 0;
}

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
            debug!(
                "Announce {:?} to {url}: time since last announce {}s, interval {}s",
                event,
                torrent.last_announce.elapsed().as_secs(),
                torrent.interval
            );
            let request = build_request(torrent, event, config);
            match self.ctx.transport.announce(&url, &request, &client).await {
                Ok(response) => apply_response(torrent, request.uploaded, response),
                Err(failure) => {
                    match &failure {
                        AnnounceError::Rejected(_) => {
                            error!("Cannot announce {} to {url}: {failure}", torrent.name);
                        }
                        AnnounceError::Transport(_) => {
                            warn!("Cannot announce {} to {url}: {failure}", torrent.name);
                        }
                    }
                    torrent.error_count += 1;
                }
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
            Err(error) => warn!("Announce task panicked: {error}"),
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
        let torrent = make_torrent(1800, None);
        let remaining = time_until_announce(&torrent);
        assert!(
            (1799..=1800).contains(&remaining),
            "expected ≈1800, got {remaining}"
        );
    }

    #[test]
    fn test_time_until_announce_zero_interval() {
        let torrent = make_torrent(0, None);
        assert_eq!(time_until_announce(&torrent), 0);
    }

    #[test]
    fn test_time_until_announce_min_interval_dominates() {
        // min_interval(3600) > interval(1800) → effective = 3600
        let torrent = make_torrent(1800, Some(3600));
        let remaining = time_until_announce(&torrent);
        assert!(
            (3599..=3600).contains(&remaining),
            "expected ≈3600, got {remaining}"
        );
    }

    #[test]
    fn test_time_until_announce_saturates_at_zero() {
        // interval=0, min_interval=None → saturating_sub(big, 0) = 0, not negative
        let mut torrent = make_torrent(0, None);
        // Backdate last_announce by simulating an old announce
        torrent.last_announce = std::time::Instant::now() - std::time::Duration::from_secs(9999);
        assert_eq!(time_until_announce(&torrent), 0);
    }

    // --- AnnounceService with a fake transport ---

    use crate::infra::tracker::testing::FakeTransport;

    fn fake_ctx(fake: FakeTransport) -> AppContext {
        let config = crate::config::Config {
            port: 6881,
            numwant: Some(25),
            min_upload_rate: 100,
            max_upload_rate: 200,
            ..crate::config::Config::default()
        };
        AppContext::with_transport(config, Box::new(fake))
    }

    fn online_torrent(urls: &[&str]) -> Torrent {
        let mut torrent = make_torrent(1800, None);
        torrent.urls = urls.iter().map(|url| url.to_string()).collect();
        torrent.info_hash_urlencoded = String::from("%01%02");
        torrent
    }

    #[tokio::test]
    async fn test_announce_success_applies_tracker_response() {
        let fake = FakeTransport::default().answer(Ok(AnnounceResponse {
            interval: Some(900),
            min_interval: Some(600),
            tracker_id: Some(String::from("tid")),
            seeders: Some(4),
            leechers: Some(9),
            warning: Some(String::from("slow down")),
        }));
        let ctx = fake_ctx(fake);
        let mut torrent = online_torrent(&["http://t.example.com/announce"]);
        torrent.error_count = 3;

        AnnounceService::new(&ctx)
            .announce(&mut torrent, None)
            .await;

        assert_eq!(torrent.interval, 900);
        assert_eq!(torrent.min_interval, Some(600));
        assert_eq!(torrent.tracker_id.as_deref(), Some("tid"));
        assert_eq!(torrent.seeders, 4);
        assert_eq!(torrent.leechers, 9);
        assert_eq!(torrent.error_count, 0);
    }

    #[tokio::test]
    async fn test_announce_started_sends_zero_uploaded_and_config_values() {
        let fake = FakeTransport::default();
        let calls = fake.calls();
        let ctx = fake_ctx(fake);
        let mut torrent = online_torrent(&["http://t.example.com/announce"]);
        torrent.next_upload_speed = 1000;
        torrent.last_announce = std::time::Instant::now() - Duration::from_secs(60);

        AnnounceService::new(&ctx)
            .announce(&mut torrent, Some(Event::Started))
            .await;

        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        let (url, request) = &calls[0];
        assert_eq!(url, "http://t.example.com/announce");
        assert_eq!(request.event, Some(Event::Started));
        assert_eq!(request.uploaded, 0);
        assert_eq!(request.port, 6881);
        assert_eq!(request.numwant, 25);
        assert_eq!(request.info_hash_urlencoded, "%01%02");
        assert_eq!(torrent.uploaded, 0);
    }

    #[tokio::test]
    async fn test_announce_reports_and_accumulates_upload_since_last_announce() {
        let fake = FakeTransport::default();
        let calls = fake.calls();
        let ctx = fake_ctx(fake);
        // No leechers → the upload speed is kept as is (deterministic).
        let mut torrent = online_torrent(&["udp://t.example.com:6969/announce"]);
        torrent.next_upload_speed = 1000;
        torrent.uploaded = 500;
        torrent.last_announce = std::time::Instant::now() - Duration::from_secs(10);

        AnnounceService::new(&ctx)
            .announce(&mut torrent, None)
            .await;

        let reported = calls.lock().unwrap()[0].1.uploaded;
        assert!(
            (10_000..=11_000).contains(&reported),
            "expected ≈10s × 1000 B/s, got {reported}"
        );
        assert_eq!(torrent.uploaded, 500 + reported);
    }

    #[tokio::test]
    async fn test_announce_failures_count_errors_and_keep_state() {
        let fake = FakeTransport::default()
            .answer(Err(AnnounceError::Rejected(String::from("unregistered"))))
            .answer(Err(AnnounceError::Transport(String::from("timeout"))));
        let ctx = fake_ctx(fake);
        let mut torrent = online_torrent(&["http://a.example.com/announce"]);
        torrent.next_upload_speed = 1000;
        torrent.last_announce = std::time::Instant::now() - Duration::from_secs(10);
        let before = torrent.last_announce;

        let service = AnnounceService::new(&ctx);
        service.announce(&mut torrent, None).await;
        service.announce(&mut torrent, None).await;

        assert_eq!(torrent.error_count, 2);
        assert_eq!(torrent.uploaded, 0);
        assert_eq!(torrent.interval, 1800);
        assert_eq!(torrent.last_announce, before);
    }

    #[tokio::test]
    async fn test_announce_contacts_every_url() {
        let fake = FakeTransport::default()
            .answer(Err(AnnounceError::Transport(String::from("down"))))
            .answer(Ok(AnnounceResponse {
                interval: Some(1200),
                ..AnnounceResponse::default()
            }));
        let calls = fake.calls();
        let ctx = fake_ctx(fake);
        let mut torrent = online_torrent(&[
            "http://a.example.com/announce",
            "udp://b.example.com:6969/announce",
        ]);

        AnnounceService::new(&ctx)
            .announce(&mut torrent, None)
            .await;

        let urls: Vec<String> = calls
            .lock()
            .unwrap()
            .iter()
            .map(|(url, _)| url.clone())
            .collect();
        assert_eq!(
            urls,
            [
                "http://a.example.com/announce",
                "udp://b.example.com:6969/announce"
            ]
        );
        // The second tracker succeeded: its interval wins and errors are reset.
        assert_eq!(torrent.interval, 1200);
        assert_eq!(torrent.error_count, 0);
    }

    #[tokio::test]
    async fn test_start_and_stop_all_send_events_to_every_torrent() {
        let fake = FakeTransport::default()
            .answer(Ok(AnnounceResponse {
                interval: Some(700),
                ..AnnounceResponse::default()
            }))
            .answer(Ok(AnnounceResponse {
                interval: Some(1500),
                ..AnnounceResponse::default()
            }));
        let calls = fake.calls();
        let ctx = fake_ctx(fake);
        let mut first = online_torrent(&["http://a.example.com/announce"]);
        first.info_hash = [1u8; 20];
        let mut second = online_torrent(&["http://b.example.com/announce"]);
        second.info_hash = [2u8; 20];
        ctx.torrents.insert_if_absent(first);
        ctx.torrents.insert_if_absent(second);

        let service = AnnounceService::new(&ctx);
        assert_eq!(service.start_all().await, 700);
        service.stop_all().await;

        let events: Vec<Option<Event>> = calls
            .lock()
            .unwrap()
            .iter()
            .map(|(_, request)| request.event)
            .collect();
        assert_eq!(
            events,
            [
                Some(Event::Started),
                Some(Event::Started),
                Some(Event::Stopped),
                Some(Event::Stopped)
            ]
        );
    }

    #[tokio::test]
    async fn test_announce_due_uses_interval_from_tracker() {
        let fake = FakeTransport::default().answer(Ok(AnnounceResponse {
            interval: Some(300),
            ..AnnounceResponse::default()
        }));
        let ctx = Arc::new(fake_ctx(fake));
        let mut torrent = online_torrent(&["http://a.example.com/announce"]);
        torrent.interval = 0; // due now
        ctx.torrents.insert_if_absent(torrent);

        let next = announce_due(&ctx, 42).await;
        assert!(
            (285..=315).contains(&next),
            "expected ≈300 ± 5%, got {next}"
        );
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
