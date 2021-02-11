use std::collections::HashMap;

enum Refresh {
    NEVER,
    TIMED_OR_AFTER_STARTED_ANNOUNCE,
    TORRENT_VOLATILE,
    TORRENT_PERSISTENT,
}
enum Key_Case {NONE, LOWER, UPPER}
enum Case {LOWER, UPPER}
enum Algorithm_Method {
    HASH_NO_LEADING_ZERO,
    HASH,
    DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES,
    REGEX,
    ///for peer ID
    /// RANDOM_POOL_WITH_CHECKSUM,
}

struct Algorithm {
    method: Algorithm_Method,
    ///for HASH_NO_LEADING_ZERO, HASH methods
    length: Option<u8>,
    ///for REGEX method
    pattern: Option<String>,
    ///for DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES
    inclusive_lower_bound: Option<u32>,
    ///for DIGIT_RANGE_TRANSFORMED_TO_HEX_WITHOUT_LEADING_ZEROES
    inclusive_upper_bound: Option<u32>,
    /// for RANDOM_POOL_WITH_CHECKSUM
    prefix: String,
    /// for RANDOM_POOL_WITH_CHECKSUM
    character_pool: String,
    /// for RANDOM_POOL_WITH_CHECKSUM
    base:u8,
}

struct Generator {
    algorithm: Algorithm,
    refresh_on: Refresh,
    should_url_encode: bool,
    refresh_every: u8,
}

struct URL_Encocer {
    encoding_exclusion_pattern: String,
    /// if the encoded hex string should be in upper case or no
    uppercase_encoded_hex: bool,
}

struct Client {
    key_generator: Generator,
    peer_id_generator = Generator,
    url_encoder = URL_Encocer,
    request_headers: HashMap<String, String>,
    num_want: u16,
    num_want_on_stom: u16,
}
