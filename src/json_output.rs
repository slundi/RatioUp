use std::{fs, path::Path};

use tracing::error;

/// Check if the given output file is writable.
pub fn writable(path: &str) -> bool {
    if path.ends_with('/') {
        error!("OUTPUT is a path, not a file");
        return false;
    }
    let p = Path::new(path);
    let Some(parent) = p.parent() else {
        error!("Output path has no parent directory");
        return false;
    };
    if !parent.is_dir() {
        error!("Directory {:?} does not exist", parent.to_str());
        return false;
    }
    match fs::metadata(parent) {
        Ok(md) => !md.permissions().readonly(),
        Err(e) => {
            error!("Cannot read directory metadata: {e}");
            false
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

    #[test]
    fn test_writable_trailing_slash_is_rejected() {
        // A path ending with '/' is a directory reference, not a file — always false.
        assert!(!writable("/tmp/"));
    }

    #[test]
    fn test_writable_nonexistent_parent_is_rejected() {
        assert!(!writable("/nonexistent_dir_xyz/output.json"));
    }
}
