use std::time::Duration;

use actix::prelude::*;
use actix::Actor;
use log::debug;
use log::info;

use crate::CLIENT;
use crate::CONFIG;
use crate::TORRENTS;
use crate::tracker;
use crate::tracker::Event;

/// A cron that check every minutes if it needs to announce, stop or start a torrent
pub struct Scheduler;
impl Actor for Scheduler {
    type Context = Context<Self>; // https://docs.rs/actix/latest/actix/sync/struct.SyncArbiter.html
    fn started(&mut self, ctx: &mut Self::Context) {
        debug!("Scheduler started");
        if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
            if let Some(refresh_every) = client.key_refresh_every {
                ctx.run_interval(
                    Duration::from_secs(u64::try_from(refresh_every).unwrap()),
                    move |this, ctx| this.refresh_key(ctx),
                );
            }
        }
    }
    fn stopped(&mut self, ctx: &mut Self::Context) {
        debug!("Scheduler stopped");
        self.announce(ctx, Some(Event::Stopped));
    }
}
impl Scheduler {
    /// Build the announce query and perform it in another thread
    fn announce(&self, ctx: &mut Context<Self>, event: Option<Event>) {
        debug!("Announcing");
        if let Some(client) = &*CLIENT.read().expect("Cannot read client") {
            let config = CONFIG.get().expect("Cannot read configuration");
            let list = &mut *TORRENTS.write().expect("Cannot get torrent list");
            let mut available_download_speed: u32 = config.max_download_rate;
            let mut available_upload_speed: u32 = config.max_upload_rate;
            let mut next_announce = 4_294_967_295u32;
            // send queries to trackers
            for t in list {
                // TODO: client.annouce(t, client);
                let mut interval: u64 = 4_294_967_295;
                if !t.shound_announce() {
                    next_announce = next_announce.min(t.interval.try_into().unwrap());
                    continue;
                }
                // let url = &t.build_urls(event.clone(), client.key.clone())[0];
                // let query = client.get_query();
                // let agent = ureq::AgentBuilder::new()
                //     .timeout(std::time::Duration::from_secs(60))
                //     .user_agent(&client.user_agent);
                // let mut req = agent
                //     .build()
                //     .get(url)
                //     .timeout(std::time::Duration::from_secs(90));
                // req = query
                //     .1
                //     .into_iter()
                //     .fold(req, |req, header| req.set(&header.0, &header.1));
                interval = interval.min(tracker::announce(t, client.clone(), event));
                // interval = t.announce(event, req);
                //compute the download and upload speed
                available_upload_speed -=
                    t.uploaded(config.min_upload_rate, available_upload_speed);
                available_download_speed -=
                    t.uploaded(config.min_upload_rate, available_download_speed);
                t.uploaded += (interval as usize) * (t.next_upload_speed as usize);
                if t.length < t.downloaded + (t.next_download_speed as usize * interval as usize) {
                    //compute next interval to for an EVENT_COMPLETED
                    let t: u64 =
                        (t.length - t.downloaded).div_euclid(t.next_download_speed as usize) as u64;
                    ctx.run_later(Duration::from_secs(t + 5), move |this, ctx| {
                        this.announce(ctx, Some(Event::Completed));
                    });
                } else {
                    ctx.run_later(Duration::from_secs(interval), move |this, ctx| {
                        this.announce(ctx, None);
                    });
                }
            }
            // TODO: schedule next announce
        }
    }

    fn refresh_key(&self, _ctx: &mut Context<Self>) {
        info!("Refreshing key");
        if let Some(client) = &mut *CLIENT.write().expect("Cannot read client") {
            client.generate_key();
        }
    }
}
