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

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a BencodeValue::ByteString
    fn bstring(s: &[u8]) -> BencodeValue {
        BencodeValue::ByteString(s.to_vec())
    }

    // Helper to create a BencodeValue::Integer
    fn bint(i: i64) -> BencodeValue {
        BencodeValue::Integer(i)
    }

    // --- Decoder Tests ---

    #[test]
    fn test_decode_integer_positive() {
        let mut decoder = BencodeDecoder::new(b"i42e");
        assert_eq!(decoder.decode().unwrap(), bint(42));
    }

    #[test]
    fn test_decode_integer_negative() {
        let mut decoder = BencodeDecoder::new(b"i-42e");
        assert_eq!(decoder.decode().unwrap(), bint(-42));
    }

    #[test]
    fn test_decode_integer_zero() {
        let mut decoder = BencodeDecoder::new(b"i0e");
        assert_eq!(decoder.decode().unwrap(), bint(0));
    }

    #[test]
    fn test_decode_byte_string() {
        let mut decoder = BencodeDecoder::new(b"4:spam");
        assert_eq!(decoder.decode().unwrap(), bstring(b"spam"));
    }

    #[test]
    fn test_decode_byte_string_empty() {
        let mut decoder = BencodeDecoder::new(b"0:");
        assert_eq!(decoder.decode().unwrap(), bstring(b""));
    }

    #[test]
    fn test_decode_byte_string_binary() {
        // Binary data that isn't valid UTF-8
        let mut decoder = BencodeDecoder::new(b"3:\xff\xfe\xfd");
        assert_eq!(decoder.decode().unwrap(), bstring(b"\xff\xfe\xfd"));
    }

    #[test]
    fn test_decode_list_empty() {
        let mut decoder = BencodeDecoder::new(b"le");
        assert_eq!(decoder.decode().unwrap(), BencodeValue::List(vec![]));
    }

    #[test]
    fn test_decode_list_integers() {
        let mut decoder = BencodeDecoder::new(b"li1ei2ei3ee");
        assert_eq!(
            decoder.decode().unwrap(),
            BencodeValue::List(vec![bint(1), bint(2), bint(3)])
        );
    }

    #[test]
    fn test_decode_list_mixed() {
        let mut decoder = BencodeDecoder::new(b"l4:spami42ee");
        assert_eq!(
            decoder.decode().unwrap(),
            BencodeValue::List(vec![bstring(b"spam"), bint(42)])
        );
    }

    #[test]
    fn test_decode_list_nested() {
        let mut decoder = BencodeDecoder::new(b"lli1ei2eeli3ei4eee");
        assert_eq!(
            decoder.decode().unwrap(),
            BencodeValue::List(vec![
                BencodeValue::List(vec![bint(1), bint(2)]),
                BencodeValue::List(vec![bint(3), bint(4)])
            ])
        );
    }

    #[test]
    fn test_decode_dictionary_empty() {
        let mut decoder = BencodeDecoder::new(b"de");
        assert_eq!(
            decoder.decode().unwrap(),
            BencodeValue::Dictionary(BTreeMap::new())
        );
    }

    #[test]
    fn test_decode_dictionary_simple() {
        let mut decoder = BencodeDecoder::new(b"d3:bar4:spam3:fooi42ee");
        let result = decoder.decode().unwrap();
        if let BencodeValue::Dictionary(dict) = result {
            assert_eq!(dict.get(b"bar".as_ref()), Some(&bstring(b"spam")));
            assert_eq!(dict.get(b"foo".as_ref()), Some(&bint(42)));
        } else {
            panic!("Expected dictionary");
        }
    }

    #[test]
    fn test_decode_dictionary_nested() {
        let mut decoder = BencodeDecoder::new(b"d5:innerd3:keyi123eee");
        let result = decoder.decode().unwrap();
        if let BencodeValue::Dictionary(dict) = result {
            if let Some(BencodeValue::Dictionary(inner)) = dict.get(b"inner".as_ref()) {
                assert_eq!(inner.get(b"key".as_ref()), Some(&bint(123)));
            } else {
                panic!("Expected inner dictionary");
            }
        } else {
            panic!("Expected dictionary");
        }
    }

    #[test]
    fn test_decode_tracker_response() {
        // Typical tracker response
        let data = b"d8:completei15e10:incompletei3e8:intervali1800ee";
        let mut decoder = BencodeDecoder::new(data);
        let result = decoder.decode().unwrap();
        if let BencodeValue::Dictionary(dict) = result {
            assert_eq!(dict.get(b"complete".as_ref()), Some(&bint(15)));
            assert_eq!(dict.get(b"incomplete".as_ref()), Some(&bint(3)));
            assert_eq!(dict.get(b"interval".as_ref()), Some(&bint(1800)));
        } else {
            panic!("Expected dictionary");
        }
    }

    #[test]
    fn test_decode_failure_reason() {
        let data = b"d14:failure reason17:Torrent not founde";
        let mut decoder = BencodeDecoder::new(data);
        let result = decoder.decode().unwrap();
        if let BencodeValue::Dictionary(dict) = result {
            assert_eq!(
                dict.get(b"failure reason".as_ref()),
                Some(&bstring(b"Torrent not found"))
            );
        } else {
            panic!("Expected dictionary");
        }
    }

    #[test]
    fn test_decode_error_unexpected_end() {
        let mut decoder = BencodeDecoder::new(b"i42");
        assert!(matches!(
            decoder.decode(),
            Err(BencodeDecoderError::UnexpectedEndOfInput)
        ));
    }

    #[test]
    fn test_decode_error_invalid_format() {
        let mut decoder = BencodeDecoder::new(b"x");
        assert!(matches!(
            decoder.decode(),
            Err(BencodeDecoderError::InvalidFormat)
        ));
    }

    #[test]
    fn test_decode_error_string_truncated() {
        let mut decoder = BencodeDecoder::new(b"10:short");
        assert!(matches!(
            decoder.decode(),
            Err(BencodeDecoderError::UnexpectedEndOfInput)
        ));
    }

    // --- Encoder Tests ---

    #[test]
    fn test_encode_integer() {
        let mut buffer = Vec::new();
        encode_bencode_value(&bint(42), &mut buffer).unwrap();
        assert_eq!(buffer, b"i42e");
    }

    #[test]
    fn test_encode_integer_negative() {
        let mut buffer = Vec::new();
        encode_bencode_value(&bint(-123), &mut buffer).unwrap();
        assert_eq!(buffer, b"i-123e");
    }

    #[test]
    fn test_encode_byte_string() {
        let mut buffer = Vec::new();
        encode_bencode_value(&bstring(b"hello"), &mut buffer).unwrap();
        assert_eq!(buffer, b"5:hello");
    }

    #[test]
    fn test_encode_byte_string_empty() {
        let mut buffer = Vec::new();
        encode_bencode_value(&bstring(b""), &mut buffer).unwrap();
        assert_eq!(buffer, b"0:");
    }

    #[test]
    fn test_encode_list() {
        let mut buffer = Vec::new();
        let list = BencodeValue::List(vec![bint(1), bstring(b"two")]);
        encode_bencode_value(&list, &mut buffer).unwrap();
        assert_eq!(buffer, b"li1e3:twoe");
    }

    #[test]
    fn test_encode_dictionary() {
        let mut buffer = Vec::new();
        let mut dict = BTreeMap::new();
        dict.insert(b"cow".to_vec(), bstring(b"moo"));
        dict.insert(b"spam".to_vec(), bstring(b"eggs"));
        encode_bencode_value(&BencodeValue::Dictionary(dict), &mut buffer).unwrap();
        // BTreeMap ensures keys are sorted: "cow" < "spam"
        assert_eq!(buffer, b"d3:cow3:moo4:spam4:eggse");
    }

    // --- Roundtrip Tests ---

    #[test]
    fn test_roundtrip_integer() {
        let original = bint(999);
        let mut buffer = Vec::new();
        encode_bencode_value(&original, &mut buffer).unwrap();
        let mut decoder = BencodeDecoder::new(&buffer);
        assert_eq!(decoder.decode().unwrap(), original);
    }

    #[test]
    fn test_roundtrip_string() {
        let original = bstring(b"test data");
        let mut buffer = Vec::new();
        encode_bencode_value(&original, &mut buffer).unwrap();
        let mut decoder = BencodeDecoder::new(&buffer);
        assert_eq!(decoder.decode().unwrap(), original);
    }

    #[test]
    fn test_roundtrip_complex() {
        let mut inner_dict = BTreeMap::new();
        inner_dict.insert(b"pieces".to_vec(), bstring(b"\x00\x01\x02\x03"));
        inner_dict.insert(b"length".to_vec(), bint(12345));

        let mut outer_dict = BTreeMap::new();
        outer_dict.insert(b"announce".to_vec(), bstring(b"http://tracker.example.com"));
        outer_dict.insert(b"info".to_vec(), BencodeValue::Dictionary(inner_dict));

        let original = BencodeValue::Dictionary(outer_dict);
        let mut buffer = Vec::new();
        encode_bencode_value(&original, &mut buffer).unwrap();

        let mut decoder = BencodeDecoder::new(&buffer);
        assert_eq!(decoder.decode().unwrap(), original);
    }
}
