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

pub fn percent_encoding(input: &[u8]) -> String {
    let mut encoded_string = String::new();
    let hex_chars = [
        '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F',
    ];

    for &byte in input {
        // Vérifie si l'octet est un caractère "unreserved" (RFC 3986)
        if byte.is_ascii_digit()
            || byte.is_ascii_lowercase()
            || byte.is_ascii_uppercase()
            || byte == b'.'
            || byte == b'-'
            || byte == b'_'
            || byte == b'~'
        {
            encoded_string.push(byte as char); // add as char
        } else {
            // encode to %XX
            encoded_string.push('%');
            encoded_string.push(hex_chars[((byte >> 4) & 0xf) as usize]);
            encoded_string.push(hex_chars[(byte & 0xf) as usize]);
        }
    }
    encoded_string
}

pub fn get_sha1(input: &[u8]) -> [u8; 20] {
    let mut m = sha1_smol::Sha1::new();
    m.update(input);
    m.digest().bytes()
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

    #[test]
    fn test_percent_encoding_example() {
        let input =
            b"\x12\x34\x56\x78\x9a\xbc\xde\xf1\x23\x45\x67\x89\xab\xcd\xef\x12\x34\x56\x78\x9a";
        let expected = "%124Vx%9A%BC%DE%F1%23Eg%89%AB%CD%EF%124Vx%9A";

        let result = percent_encoding(input);
        assert_eq!(result, expected);

        let input_unreserved = b"abc.DEF-012_~";
        assert_eq!(percent_encoding(input_unreserved), "abc.DEF-012_~");

        let input_reserved = b"Hello World! #@$%^&";
        assert_eq!(
            percent_encoding(input_reserved),
            "Hello%20World%21%20%23%40%24%25%5E%26"
        );

        let input_non_ascii = "éàç".as_bytes(); // En UTF-8: [0xC3, 0xA9, 0xC3, 0xE0, 0xC3, 0xA7]
        assert_eq!(percent_encoding(input_non_ascii), "%C3%A9%C3%A0%C3%A7");

        let input_null = b"null\x00byte";
        assert_eq!(percent_encoding(input_null), "null%00byte");
    }

    #[test]
    fn test_sha1() {
        let input = b"Hello World!";
        let mut m = sha1_smol::Sha1::new();
        m.update(input);
        let sha1 = get_sha1(input);
        let digest = m.digest();
        assert_eq!(digest.bytes(), sha1);
        
        println!("SHA: {digest}");
        assert_eq!(
            sha1,
            *b"\x2e\xf7\xbd\xe6\x08\xce\x54\x04\xe9\x7d\x5f\x04\x2f\x95\xf8\x9f\x1c\x23\x28\x71" 
        );
    }

    // [181, 7, 198, 150, 79, 250, 63, 170, 170, 26, 163, 172, 45, 66, 45, 57, 169, 201, 226, 70] => should be b507c6964ffa3faaaa1aa3ac2d422d39a9c9e246
}
