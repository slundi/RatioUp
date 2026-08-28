use std::sync::Arc;
use tokio::task::JoinSet;
use tokio::time::Duration;
use tracing::{debug, info, warn};

use crate::TORRENTS;
use crate::torrent::Torrent;

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
async fn announce_all_due(fallback_interval: u64) -> u64 {
    let keys: Vec<[u8; 20]> = TORRENTS.iter().map(|e| *e.key()).collect();
    let mut join_set: JoinSet<u64> = JoinSet::new();

    for key in keys {
        if let Some(entry) = TORRENTS.get(&key) {
            let arc = Arc::clone(entry.value());
            drop(entry); // release DashMap shard lock before awaiting
            join_set.spawn(async move {
                let mut t = arc.lock().await;
                if t.should_announce() {
                    crate::announcer::tracker::announce(&mut t, None).await;
                }
                time_until_announce(&t)
            });
        }
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

pub async fn run(wait_time: u64) {
    info!("Starting scheduler");
    loop {
        let next_interval = announce_all_due(wait_time).await;
        debug!("Next announce in {}s", next_interval);
        crate::json_output::write().await;
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
