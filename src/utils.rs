pub fn format_bytes(bytes: u32) -> String {
    const KB: u32 = 1024;
    const MB: u32 = KB * 1024;
    const GB: u32 = MB * 1024;
    // const TB: u32 = GB * 1024; // Note: TB here will exceed u32 max, but used for comparison logic

    if bytes < KB {
        format!("{} B", bytes)
    } else if bytes < MB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else if bytes < GB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    // } else if bytes < TB { // This condition will handle up to 4GB approx for u32 input
    //     format!("{:.1} GB", bytes as f64 / GB as f64)
    } else {
        // For u32, reaching TB is impossible (max u32 is 4,294,967,295 bytes ~ 4GB)
        // However, if the input type were larger (e.g., u64), this would be relevant.
        // For u32, we'll default to GB for very large values that technically exceed GB threshold
        // but not TB based on u32 max.
        format!("{:.1} GB", bytes as f64 / GB as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_kilobytes() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB"); // 1.5 * 1024
        assert_eq!(format_bytes(9999), "9.8 KB"); // rounded
        assert_eq!(format_bytes(1023999), "1000.0 KB"); // just bellow 1 MB
    }

    #[test]
    fn test_megabytes() {
        assert_eq!(format_bytes(1_048_576), "1.0 MB"); // 1024 * 1024
        assert_eq!(format_bytes(1_572_864), "1.5 MB"); // 1.5 * 1024 * 1024
        assert_eq!(format_bytes(50_000_000), "47.7 MB"); // some value
        assert_eq!(format_bytes(1_073_741_823), "1024.0 MB"); // just bellow 1 GB
    }

    #[test]
    fn test_gigabytes() {
        assert_eq!(format_bytes(1_073_741_824), "1.0 GB"); // 1024 * 1024 * 1024
        assert_eq!(format_bytes(2_147_483_648), "2.0 GB"); // 2 * 1 GB
        assert_eq!(format_bytes(4_000_000_000), "3.7 GB");
        assert_eq!(format_bytes(4_294_967_295), "4.0 GB"); // max u32
    }

    #[test]
    fn test_beyond_gigabytes_with_u32() {
        assert_eq!(format_bytes(u32::MAX), "4.0 GB");
    }
}
