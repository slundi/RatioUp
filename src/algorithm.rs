use rand::{SeedableRng, Rng};

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
    println!("{}", h);
    return h;
}

pub fn regex(pattern: &str) -> String {
    /*let mut gen=Generator::new(pattern, , DEFAULT_MAX_REPEAT).unwrap();
    let mut buffer = vec![];
    gen.generate(&mut buffer).unwrap();
    return String::from_utf8(buffer).unwrap();*/
    let mut rng=rand::thread_rng();
    let gen = rand_regex::Regex::compile(pattern, 100).unwrap();
    let out = (&mut rng).sample_iter(&gen).nth(64).unwrap();
    return out;
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
        assert_eq!(h.chars().nth(0).unwrap()=='0', false);
    }
    #[test]
    fn is_hash_case_ok() {
        let re_uc=Regex::new(r"[A-Z0-9]{64}").unwrap();
        let re_lc=Regex::new(r"[a-z0-9]{64}").unwrap();
        assert_eq!(re_uc.is_match(&hash(64, false, None)), true);
        assert_eq!(re_uc.is_match(&hash(64, false, Some(true))), true);
        assert_eq!(re_lc.is_match(&hash(64, false, Some(false))), true);
    }
    #[test]
    fn is_regex_ok() {
        let mut re=Regex::new("-lt0D60-[\u{0001}-\u{00ff}]{12}").unwrap();
        assert_eq!(re.is_match(&regex("-lt0D60-[\u{0001}-\u{00ff}]{12}")), true);
        re=Regex::new("-AZ5750-[a-zA-Z0-9]{12}").unwrap();
        assert_eq!(re.is_match(&regex("-AZ5750-[a-zA-Z0-9]{12}")), true)
    }
}