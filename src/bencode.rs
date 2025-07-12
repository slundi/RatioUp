use std::collections::BTreeMap;
use std::str;

use crate::torrent::TorrentError;

#[derive(Debug, PartialEq)]
pub enum BencodeValue {
    Integer(i64),
    ByteString(Vec<u8>),
    List(Vec<BencodeValue>),
    Dictionary(BTreeMap<Vec<u8>, BencodeValue>),
}

/// Errors that can occur during Bencode decoding.
#[derive(Debug)]
pub enum BencodeDecoderError {
    InvalidFormat,
    UnexpectedEndOfInput,
    ParseIntError(String), // Store error message as String
    Utf8Error(String),     // Store error message as String
}

// Convert ParseIntError to BencodeDecoderError
impl From<std::num::ParseIntError> for BencodeDecoderError {
    fn from(err: std::num::ParseIntError) -> Self {
        BencodeDecoderError::ParseIntError(err.to_string())
    }
}

// Convert Utf8Error to BencodeDecoderError
impl From<std::str::Utf8Error> for BencodeDecoderError {
    fn from(err: std::str::Utf8Error) -> Self {
        BencodeDecoderError::Utf8Error(err.to_string())
    }
}

/// A Bencode decoder.
pub struct BencodeDecoder<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> BencodeDecoder<'a> {
    /// Creates a new decoder with the provided Bencode data.
    pub fn new(data: &'a [u8]) -> Self {
        BencodeDecoder { data, position: 0 }
    }

    /// Decodes the next Bencode value from the input.
    pub fn decode(&mut self) -> Result<BencodeValue, BencodeDecoderError> {
        self.peek_byte()
            .ok_or(BencodeDecoderError::UnexpectedEndOfInput)
            .and_then(|byte| match byte {
                b'i' => self.decode_integer(),
                b'0'..=b'9' => self.decode_byte_string(),
                b'l' => self.decode_list(),
                b'd' => self.decode_dictionary(),
                _ => Err(BencodeDecoderError::InvalidFormat),
            })
    }

    /// Peeks at the current byte without advancing the position.
    fn peek_byte(&self) -> Option<u8> {
        self.data.get(self.position).copied()
    }

    /// Advances the position and returns the current byte.
    fn consume_byte(&mut self) -> Option<u8> {
        if self.position < self.data.len() {
            let byte = self.data[self.position];
            self.position += 1;
            Some(byte)
        } else {
            None
        }
    }

    /// Reads until the next `delimiter` and returns the slice of data before it.
    /// Advances the position past the delimiter.
    fn read_until(&mut self, delimiter: u8) -> Result<&'a [u8], BencodeDecoderError> {
        let start = self.position;
        while self.position < self.data.len() {
            if self.data[self.position] == delimiter {
                let slice = &self.data[start..self.position];
                self.position += 1; // Consume the delimiter
                return Ok(slice);
            }
            self.position += 1;
        }
        Err(BencodeDecoderError::UnexpectedEndOfInput)
    }

    /// Decodes an integer value (i<integer>e).
    fn decode_integer(&mut self) -> Result<BencodeValue, BencodeDecoderError> {
        self.consume_byte(); // Consume 'i'
        let num_slice = self.read_until(b'e')?;
        let num_str = str::from_utf8(num_slice)?;
        let num: i64 = num_str.parse()?;
        Ok(BencodeValue::Integer(num))
    }

    /// Decodes a byte string (<length>:<string>).
    fn decode_byte_string(&mut self) -> Result<BencodeValue, BencodeDecoderError> {
        let len_slice = self.read_until(b':')?;
        let len_str = str::from_utf8(len_slice)?;
        let len: usize = len_str.parse()?;

        if self.position + len > self.data.len() {
            return Err(BencodeDecoderError::UnexpectedEndOfInput);
        }

        let start = self.position;
        self.position += len;
        Ok(BencodeValue::ByteString(
            self.data[start..self.position].to_vec(),
        ))
    }

    /// Decodes a list (l<element1><element2>...e).
    fn decode_list(&mut self) -> Result<BencodeValue, BencodeDecoderError> {
        self.consume_byte(); // Consume 'l'
        let mut list = Vec::new();
        while self
            .peek_byte()
            .ok_or(BencodeDecoderError::UnexpectedEndOfInput)?
            != b'e'
        {
            list.push(self.decode()?); // Recursive call
        }
        self.consume_byte(); // Consume 'e'
        Ok(BencodeValue::List(list))
    }

    /// Decodes a dictionary (d<key1><value1><key2><value2>...e).
    fn decode_dictionary(&mut self) -> Result<BencodeValue, BencodeDecoderError> {
        self.consume_byte(); // Consume 'd'
        let mut dict = BTreeMap::new();
        while self
            .peek_byte()
            .ok_or(BencodeDecoderError::UnexpectedEndOfInput)?
            != b'e'
        {
            let key = match self.decode_byte_string()? {
                // Keys must be ByteStrings
                BencodeValue::ByteString(b) => b,
                _ => return Err(BencodeDecoderError::InvalidFormat), // Key is not a string
            };
            let value = self.decode()?; // Recursive call
            dict.insert(key, value);
        }
        self.consume_byte(); // Consume 'e'
        Ok(BencodeValue::Dictionary(dict))
    }
}

// This is needed because our simple decoder doesn't give us the raw byte range of the 'info' dict.
// A more advanced parser might store byte ranges during parsing.
pub fn encode_bencode_value(
    value: &BencodeValue,
    buffer: &mut Vec<u8>,
) -> Result<(), TorrentError> {
    match value {
        BencodeValue::Integer(i) => {
            buffer.push(b'i');
            buffer.extend_from_slice(i.to_string().as_bytes());
            buffer.push(b'e');
        }
        BencodeValue::ByteString(s) => {
            buffer.extend_from_slice(s.len().to_string().as_bytes());
            buffer.push(b':');
            buffer.extend_from_slice(s);
        }
        BencodeValue::List(list) => {
            buffer.push(b'l');
            for item in list {
                encode_bencode_value(item, buffer)?;
            }
            buffer.push(b'e');
        }
        BencodeValue::Dictionary(dict) => {
            buffer.push(b'd');
            for (key, val) in dict.iter() {
                // Keys must be byte strings and sorted, BTreeMap ensures sorted iteration
                buffer.extend_from_slice(key.len().to_string().as_bytes());
                buffer.push(b':');
                buffer.extend_from_slice(key);
                encode_bencode_value(val, buffer)?;
            }
            buffer.push(b'e');
        }
    }
    Ok(())
}

// trait BencodeDictExt {
//     fn get_as_bytes(&self, key: &str) -> Option<&Vec<u8>>;
//     fn get_as_string(&self, key: &str) -> Option<String>;
//     fn get_as_integer(&self, key: &str) -> Option<i64>;
//     fn get_as_list(&self, key: &str) -> Option<&Vec<BencodeValue>>;
//     fn get_as_dict(&self, key: &str) -> Option<&BTreeMap<Vec<u8>, BencodeValue>>;
// }

// impl BencodeDictExt for BTreeMap<Vec<u8>, BencodeValue> {
//     fn get_as_bytes(&self, key: &str) -> Option<&Vec<u8>> {
//         self.get(key.as_bytes()).and_then(|v| {
//             if let BencodeValue::ByteString(b) = v {
//                 Some(b)
//             } else {
//                 None
//             }
//         })
//     }

//     fn get_as_string(&self, key: &str) -> Option<String> {
//         self.get_as_bytes(key)
//             .and_then(|b| str::from_utf8(b).ok().map(|s| s.to_string()))
//     }

//     fn get_as_integer(&self, key: &str) -> Option<i64> {
//         self.get(key.as_bytes()).and_then(|v| {
//             if let BencodeValue::Integer(i) = v {
//                 Some(*i)
//             } else {
//                 None
//             }
//         })
//     }

//     fn get_as_list(&self, key: &str) -> Option<&Vec<BencodeValue>> {
//         self.get(key.as_bytes()).and_then(|v| {
//             if let BencodeValue::List(l) = v {
//                 Some(l)
//             } else {
//                 None
//             }
//         })
//     }

//     fn get_as_dict(&self, key: &str) -> Option<&BTreeMap<Vec<u8>, BencodeValue>> {
//         self.get(key.as_bytes()).and_then(|v| {
//             if let BencodeValue::Dictionary(d) = v {
//                 Some(d)
//             } else {
//                 None
//             }
//         })
//     }
// }

#[cfg(test)]
mod tests {
    // use crate::{torrent::Torrent, utils::{get_sha1, percent_encoding}};

    // use super::*;

    // // Helper to create a simple BencodeValue::Dictionary from a Vec of key-value pairs
    // fn bdict(pairs: Vec<(&[u8], BencodeValue)>) -> BencodeValue {
    //     let mut map = BTreeMap::new();
    //     for (key, value) in pairs {
    //         map.insert(key.to_vec(), value);
    //     }
    //     BencodeValue::Dictionary(map)
    // }

    // // Helper to create a BencodeValue::List
    // fn blist(items: Vec<BencodeValue>) -> BencodeValue {
    //     BencodeValue::List(items)
    // }

    // // Helper to create a BencodeValue::ByteString
    // fn bstring(s: &[u8]) -> BencodeValue {
    //     BencodeValue::ByteString(s.to_vec())
    // }

    // // Helper to create a BencodeValue::Integer
    // fn bint(i: i64) -> BencodeValue {
    //     BencodeValue::Integer(i)
    // }

    // #[test]
    // fn test_decode_torrent_single_file() {
    //     // Example from a simplified single-file .torrent structure
    //     let bencode_data = b"d8:announce30:http://tracker.example.com/announce4:info29:d6:lengthi1234e4:name4:testee";
    //     let torrent = Torrent::from_bencode_bytes(bencode_data).unwrap();

    //     assert_eq!(torrent.name, "test");
    //     assert_eq!(torrent.urls, vec!["http://tracker.example.com/announce"]);
    //     assert_eq!(torrent.length, 1234);
    //     assert!(!torrent.private); // Default if not present
    //     assert_eq!(torrent.uploaded, 0); // Default
    //     assert_eq!(torrent.seeders, 0); // Default
    //     assert_eq!(torrent.leechers, 0); // Default
    //     assert_eq!(torrent.next_upload_speed, 0); // Default
    //     assert_eq!(torrent.interval, 0); // Default
    //     assert_eq!(torrent.error_count, 0); // Default
    //     assert_eq!(torrent.encoding, None); // Default

    //     // Test info_hash calculation (SHA1 of 'd6:lengthi1234e4:name4:testee')
    //     let expected_info_hash = get_sha1(b"d6:lengthi1234e4:name4:testee");
    //     assert_eq!(torrent.info_hash, expected_info_hash);
    //     assert_eq!(
    //         torrent.info_hash_urlencoded,
    //         percent_encoding(&expected_info_hash).to_string()
    //     );
    // }

    // #[test]
    // fn test_decode_torrent_multi_file() {
    //     // Simplified multi-file .torrent example
    //     let bencode_data = b"d8:announce30:http://tracker.example.com/announce4:info90:d5:filesl\
    //         d6:lengthi100e4:pathl4:file1ee\
    //         d6:lengthi200e4:pathl4:file2ee\
    //         e4:name4:my_filesee";
    //     let torrent = Torrent::from_bencode_bytes(bencode_data).unwrap();

    //     assert_eq!(torrent.name, "my_files");
    //     assert_eq!(torrent.urls, vec!["http://tracker.example.com/announce"]);
    //     assert_eq!(torrent.length, 300); // 100 + 200
    //     assert!(!torrent.private);

    //     // Test info_hash calculation
    //     let expected_info_hash_raw = b"d5:filesl\
    //         d6:lengthi100e4:pathl4:file1ee\
    //         d6:lengthi200e4:pathl4:file2ee\
    //         e4:name4:my_filesee";
    //     let expected_info_hash = get_sha1(expected_info_hash_raw);
    //     assert_eq!(torrent.info_hash, expected_info_hash);
    // }

    // #[test]
    // fn test_decode_torrent_announce_list() {
    //     let bencode_data = b"d8:announce30:http://tracker.example.com/announce13:announce-listll31:http://tracker1.com/announceel31:http://tracker2.com/announceee4:info29:d6:lengthi100e4:name4:testee";
    //     let torrent = Torrent::from_bencode_bytes(bencode_data).unwrap();

    //     assert_eq!(torrent.urls.len(), 3);
    //     assert!(
    //         torrent
    //             .urls
    //             .contains(&"http://tracker.example.com/announce".to_string())
    //     );
    //     assert!(
    //         torrent
    //             .urls
    //             .contains(&"http://tracker1.com/announce".to_string())
    //     );
    //     assert!(
    //         torrent
    //             .urls
    //             .contains(&"http://tracker2.com/announce".to_string())
    //     );
    //     assert_eq!(torrent.length, 100);
    // }

    // #[test]
    // fn test_decode_torrent_private_flag() {
    //     let bencode_data = b"d8:announce30:http://tracker.example.com/announce4:info24:d6:lengthi123e7:privatei1e4:name4:testee";
    //     let torrent = Torrent::from_bencode_bytes(bencode_data).unwrap();
    //     assert!(torrent.private);

    //     let bencode_data_public = b"d8:announce30:http://tracker.example.com/announce4:info24:d6:lengthi123e7:privatei0e4:name4:testee";
    //     let torrent_public = Torrent::from_bencode_bytes(bencode_data_public).unwrap();
    //     assert!(!torrent_public.private);
    // }

    // #[test]
    // fn test_decode_torrent_encoding() {
    //     let bencode_data = b"d8:announce30:http://tracker.example.com/announce8:encoding6:UTF-84:info16:d6:lengthi1234e4:name4:testee";
    //     let torrent = Torrent::from_bencode_bytes(bencode_data).unwrap();
    //     assert_eq!(torrent.encoding, Some("UTF-8".to_string()));
    // }

    // #[test]
    // fn test_decode_torrent_missing_info() {
    //     let bencode_data = b"d8:announce30:http://tracker.example.com/announcee";
    //     let err = Torrent::from_bencode_bytes(bencode_data).unwrap_err();
    //     if let TorrentError::MissingField(field) = err {
    //         assert_eq!(field, "info");
    //     } else {
    //         panic!("Expected MissingField error, got {:?}", err);
    //     }
    // }

    // #[test]
    // fn test_decode_torrent_invalid_name_type() {
    //     let bencode_data = b"d8:announce30:http://tracker.example.com/announce4:info16:d6:lengthi1234e4:namei123ee"; // name is an int
    //     let err = Torrent::from_bencode_bytes(bencode_data).unwrap_err();
    //     if let TorrentError::InvalidFieldType(field) = err {
    //         assert_eq!(field, "info.name");
    //     } else {
    //         panic!("Expected InvalidFieldType error, got {:?}", err);
    //     }
    // }
}
