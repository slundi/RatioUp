use tracing::{debug, info};

use crate::TORRENTS;
use tokio::time::Duration;

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

pub async fn run(wait_time: u64) {
    info!("Starting scheduler");
    loop {
        let next_interval = {
            let list = TORRENTS.read().await;
            // Compute minimum time until next announce across all torrents
            let mut min_interval = u64::MAX;
            for m in list.iter() {
                let mut t = m.lock().await;
                if t.should_announce() {
                    super::tracker::announce(&mut t, None).await;
                }
                // Always update min_interval based on time until next announce
                let elapsed = t.last_announce.elapsed().as_secs();
                let time_until_announce = t.interval.saturating_sub(elapsed);
                min_interval = u64::min(min_interval, time_until_announce);
            }
            // Ensure we don't sleep forever if no torrents or all have 0 interval
            if min_interval == u64::MAX || min_interval == 0 {
                wait_time
            } else {
                add_jitter(min_interval)
            }
        };
        debug!("Next announce in {}s", next_interval);
        crate::json_output::write().await;
        tokio::time::sleep(Duration::from_secs(next_interval)).await;
    }
}

// /// Build the announce query and perform it in another thread
// fn announce(event: Option<Event>) {
//     debug!("Announcing");
//     if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
//         let config = CONFIG.get().expect("Cannot read configuration");
//         let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
//         let mut available_download_speed: u32 = config.max_download_rate;
//         let mut available_upload_speed: u32 = config.max_upload_rate;
//         let mut next_announce = 4_294_967_295u32;
//         // send queries to trackers
//         for t in list {
//             // TODO: client.annouce(t, client);
//             let mut interval: u64 = 4_294_967_295;
//             if !t.should_announce() {
//                 next_announce = next_announce.min(t.interval.try_into().unwrap());
//                 continue;
//             }
//             // let url = &t.build_urls(event.clone(), client.key.clone())[0];
//             // let query = client.get_query();
//             // let agent = ureq::AgentBuilder::new()
//             //     .timeout(std::time::Duration::from_secs(60))
//             //     .user_agent(&client.user_agent);
//             // let mut req = agent
//             //     .build()
//             //     .get(url)
//             //     .timeout(std::time::Duration::from_secs(90));
//             // req = query
//             //     .1
//             //     .into_iter()
//             //     .fold(req, |req, header| req.set(&header.0, &header.1));
//             interval = interval.min(tracker::announce(t, event));
//             // interval = t.announce(event, req);
//             //compute the download and upload speed
//             available_upload_speed -= t.uploaded(config.min_upload_rate, available_upload_speed);
//             available_download_speed -=
//                 t.uploaded(config.min_upload_rate, available_download_speed);
//             t.uploaded += (interval as usize) * (t.next_upload_speed as usize);
//             // if t.length < t.downloaded + (t.next_download_speed as usize * interval as usize) {
//             //     //compute next interval to for an EVENT_COMPLETED
//             //     let t: u64 =
//             //         (t.length - t.downloaded).div_euclid(t.next_download_speed as usize) as u64;
//             //     ctx.run_later(Duration::from_secs(t + 5), move |this, ctx| {
//             //         this.announce(ctx, Some(Event::Completed));
//             //     });
//             // } else {
//             //     ctx.run_later(Duration::from_secs(interval), move |this, ctx| {
//             //         this.announce(ctx, None);
//             //     });
//             // }
//         }
//         // TODO: schedule next announce
//     }
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_jitter_short_interval() {
        // Short intervals should not have jitter
        assert_eq!(add_jitter(10), 10);
        assert_eq!(add_jitter(19), 19);
        assert_eq!(add_jitter(0), 0);
    }

    #[test]
    fn test_add_jitter_bounds() {
        // Test that jitter stays within ±5% bounds
        let interval = 1000u64;
        let min_expected = 950; // -5%
        let max_expected = 1050; // +5%

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
        // Typical tracker interval of 1800s (30 minutes)
        let interval = 1800u64;
        let min_expected = 1710; // -5%
        let max_expected = 1890; // +5%

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
