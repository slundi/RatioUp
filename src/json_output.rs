use std::{fs, path::Path};

use crate::{STARTED, TORRENTS};
use tracing::error;

/// Check if the given output file is writable.
pub fn writable(path: &str) -> bool {
    if path.ends_with('/') {
        error!("OUTPUT is a path, not a file");
        return false;
    }
    let p = Path::new(path);
    let parent = p.parent().unwrap();
    if !parent.is_dir() {
        error!("Directory {:?} does not exist", parent.to_str());
        return false;
    }
    let md = fs::metadata(parent).unwrap();
    !md.permissions().readonly()
}

/// Write a session file with torrent and its stats
pub async fn write() {
    let config = crate::CONFIG.get().unwrap();
    if let Some(path) = config.output_stats.clone() {
        let mut data = String::with_capacity(4096);

        // fill data in struct
        let started = *STARTED.get().unwrap();
        data.push_str("{\"started\":\"");
        data.push_str(&started.to_rfc3339());

        data.push_str("\", \"torrents\": [\n");
        {
            let torrents = TORRENTS.read().await;
            for m in torrents.iter() {
                data.push_str(&m.lock().await.to_json());
            }
        }
        data.push_str("]}");
        if let Err(e) = tokio::fs::write(path, data.as_bytes()).await {
            error!("Cannot write stat file: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::json_output::writable;
    #[test]
    fn test_writable() {
        assert!(writable("/dev/null"));

        // case with non writable folder
        let unwritable = "/tmp/unwritable";
        let p = std::path::Path::new(unwritable);
        if !p.is_dir() {
            std::fs::create_dir(unwritable).unwrap();
        }
        let md = std::fs::metadata(unwritable).unwrap();
        let mut permissions = md.permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(unwritable, permissions).unwrap();
        assert!(!writable("/tmp/unwritable/ratioup.json"));

        // case when folder does not exists
        assert!(!writable("/aze/rty/uio/pqs/ratioup.json"));
    }
}
