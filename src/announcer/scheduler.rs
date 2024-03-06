use crate::TORRENTS;

pub fn run(wait_time: u64) {
    let mut next_interval = wait_time;
    loop {
        let list = TORRENTS.read().expect("Cannot get torrent list");
        for m in list.iter() {
            let mut t = m.lock().unwrap();
            if t.shound_announce() {
                next_interval = u64::min(next_interval, super::tracker::announce(&mut t, None));
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(next_interval));
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
//             if !t.shound_announce() {
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
