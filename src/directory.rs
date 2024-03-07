use log::{error, info};

pub fn prepare_torrent_folder(directory: &String) {
    if !std::path::Path::new(directory).is_dir() {
        std::fs::create_dir_all(directory).unwrap_or_else(|_e| {
            error!("Cannot create torrent folder directory(ies)");
        });
        info!("Torrent directory created: {}", directory);
    }
    info!("Will load torrents from: {}", directory);
}

pub fn load_torrents(directory: &String) -> u64 {
    let paths = std::fs::read_dir(directory).expect("Cannot read torrent directory");
    let mut count = 0u16;
    let mut next_announce_time = 1800u64;
    for p in paths {
        let f = p
            .expect("Cannot get torrent path")
            .path()
            .into_os_string()
            .into_string()
            .expect("Cannot get file name");
        next_announce_time = u64::min(next_announce_time, crate::add_torrent(f));
        count += 1;
    }
    info!("{} torrent(s) loaded", count);
    next_announce_time
}
