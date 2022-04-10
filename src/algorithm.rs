#![allow(unused)]

use rand::Rng;

const HASH_SYMBOLS: &str = "abcdef0123456789ABCDEF";

pub fn hash(length: usize, no_leading_zero: bool, uppercase: Option<bool>) -> String {
    let mut rng=rand::thread_rng();
    let mut h=String::with_capacity(length);
    while h.len() < length {
        let i: usize = rng.gen_range(0usize..15usize);
        if i==0 && no_leading_zero {continue;}
        if uppercase==None || uppercase.unwrap() {h.push(HASH_SYMBOLS.chars().nth(i+6).unwrap());}
        else {h.push(HASH_SYMBOLS.chars().nth(i).unwrap());}
    }
    h
}

/// Generate a string from a regex pattern
pub fn regex(pattern: String) -> String {
    let mut rng=rand::thread_rng();
    let gen = rand_regex::Regex::compile(&pattern, 64).unwrap();
    (&mut rng).sample_iter(&gen).nth(64).unwrap()
}

/// Return a hex string from a random integer between 1 and 2147483647
pub fn digit_range_transformed_to_hex_without_leading_zero() -> String {
    let mut rng = rand::thread_rng();
    return format!("{:x}", rng.gen_range(1u32..2147483647u32)).to_uppercase();
}

/// Used for some peer ID generation
pub fn random_pool_with_checksum(length: usize, prefix: &str, characters_pool: &str) -> String {
    if prefix.is_empty() {panic!("algorithm prefix must not be null or empty.");}
    if characters_pool.is_empty() {panic!("algorithm character pool must not be null or empty.");}
    let mut out=String::with_capacity(length);
    out.push_str(prefix);
    let mut rng=rand::thread_rng();
    //TODO:
    let length:usize=rng.gen_range(10usize..50usize);
    let mut h=String::with_capacity(length);
    while h.len() < length {

    }
    //out.push_str(&format!("{:x}", ));
    out
}

//******************************************* TESTS
#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    #[test]
    fn is_hash_ok() {
        assert_eq!(hash(8, false, None).len(), 8usize);
        let h=hash(8, true,  None);
        assert_eq!(h.len(), 8usize);
        assert!(!h.starts_with('0'));
    }
    #[test]
    fn is_hash_case_ok() {
        let re_uc=Regex::new(r"[A-Z0-9]{64}").unwrap();
        let re_lc=Regex::new(r"[a-z0-9]{64}").unwrap();
        assert!(re_uc.is_match(&hash(64, false, None)));
        assert!(re_uc.is_match(&hash(64, false, Some(true))));
        assert!(re_lc.is_match(&hash(64, false, Some(false))));
    }
    #[test]
    fn is_regex_ok() {
        let mut re=Regex::new("-lt0D60-[\u{0001}-\u{00ff}]{12}").unwrap();
        assert!(re.is_match(&regex("-lt0D60-[\u{0001}-\u{00ff}]{12}".to_owned())));
        re=Regex::new("-AZ5750-[a-zA-Z0-9]{12}").unwrap();
        assert!(re.is_match(&regex("-AZ5750-[a-zA-Z0-9]{12}".to_owned())))
    }
    #[test]
    fn is_digit_range_to_hex_ok() {
        let re=Regex::new(r"[A-Z0-9]+").unwrap();
        assert!(re.is_match(&digit_range_transformed_to_hex_without_leading_zero()));
    }
}