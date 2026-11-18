//! A small CBOR reader for the owner panel ceremonies.
//!
//! The ceremonies carry two CBOR documents: the attestation object of a
//! registration and the COSE public key inside it. Both are tiny and use a
//! narrow slice of the format, so the reader below covers exactly that
//! slice and rejects everything else instead of guessing.

/// A CBOR value.
#[derive(Debug, Clone, PartialEq)]
pub enum CborValue {
    Unsigned(u64),
    Negative(i128),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<CborValue>),
    Map(Vec<(CborValue, CborValue)>),
    Tag(u64, Box<CborValue>),
    Simple(u8),
    Float(f64),
}

impl CborValue {
    /// The value as an integer, for both the positive and the negative form.
    pub fn as_integer(&self) -> Option<i128> {
        match self {
            Self::Unsigned(value) => Some(*value as i128),
            Self::Negative(value) => Some(*value),
            _ => None,
        }
    }

    /// The value as a byte string.
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::Bytes(value) => Some(value),
            _ => None,
        }
    }

    /// The value as a text string.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    /// The value as a map.
    pub fn as_map(&self) -> Option<&[(CborValue, CborValue)]> {
        match self {
            Self::Map(entries) => Some(entries),
            _ => None,
        }
    }
}

/// The reader over the encoded document.
struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self) -> Result<u8, String> {
        let byte = *self
            .data
            .get(self.position)
            .ok_or_else(|| "CBOR_TRUNCATED".to_string())?;
        self.position += 1;
        Ok(byte)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| "CBOR_LENGTH_OVERFLOW".to_string())?;
        if end > self.data.len() {
            return Err("CBOR_TRUNCATED".to_string());
        }
        let slice = &self.data[self.position..end];
        self.position = end;
        Ok(slice)
    }

    /// Read the argument of a head byte.
    ///
    /// The length of a string, an array or a map is either written directly,
    /// in one, two, four or eight bytes, or it is indefinite, which this
    /// reader refuses.
    fn argument(&mut self, additional: u8) -> Result<u64, String> {
        match additional {
            0..=23 => Ok(additional as u64),
            24 => Ok(self.byte()? as u64),
            25 => {
                let bytes = self.take(2)?;
                Ok(u16::from_be_bytes([bytes[0], bytes[1]]) as u64)
            }
            26 => {
                let bytes = self.take(4)?;
                Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as u64)
            }
            27 => {
                let bytes = self.take(8)?;
                let mut value = [0u8; 8];
                value.copy_from_slice(bytes);
                Ok(u64::from_be_bytes(value))
            }
            _ => Err("CBOR_LENGTH_INVALID".to_string()),
        }
    }

    fn value(&mut self) -> Result<CborValue, String> {
        let initial = self.byte()?;
        let major = initial >> 5;
        let additional = initial & 0x1f;
        match major {
            0 => Ok(CborValue::Unsigned(self.argument(additional)?)),
            1 => {
                let value = self.argument(additional)? as i128;
                Ok(CborValue::Negative(-1 - value))
            }
            2 => {
                let length = self.argument(additional)? as usize;
                Ok(CborValue::Bytes(self.take(length)?.to_vec()))
            }
            3 => {
                let length = self.argument(additional)? as usize;
                let bytes = self.take(length)?;
                let text = String::from_utf8(bytes.to_vec())
                    .map_err(|_| "CBOR_TEXT_INVALID".to_string())?;
                Ok(CborValue::Text(text))
            }
            4 => {
                let length = self.argument(additional)?;
                let mut items = Vec::new();
                for _ in 0..length {
                    items.push(self.value()?);
                }
                Ok(CborValue::Array(items))
            }
            5 => {
                let length = self.argument(additional)?;
                let mut entries = Vec::new();
                for _ in 0..length {
                    let key = self.value()?;
                    let value = self.value()?;
                    entries.push((key, value));
                }
                Ok(CborValue::Map(entries))
            }
            6 => {
                let tag = self.argument(additional)?;
                let inner = self.value()?;
                Ok(CborValue::Tag(tag, Box::new(inner)))
            }
            7 => match additional {
                20..=24 => Ok(CborValue::Simple(additional)),
                25 => {
                    let bytes = self.take(2)?;
                    Ok(CborValue::Float(half_to_f64(u16::from_be_bytes([
                        bytes[0], bytes[1],
                    ]))))
                }
                26 => {
                    let bytes = self.take(4)?;
                    Ok(CborValue::Float(
                        f32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f64,
                    ))
                }
                27 => {
                    let bytes = self.take(8)?;
                    let mut value = [0u8; 8];
                    value.copy_from_slice(bytes);
                    Ok(CborValue::Float(f64::from_be_bytes(value)))
                }
                _ => Err("CBOR_SIMPLE_INVALID".to_string()),
            },
            _ => Err("CBOR_MAJOR_INVALID".to_string()),
        }
    }
}

/// Convert a half precision float into a double.
fn half_to_f64(bits: u16) -> f64 {
    let sign = if bits & 0x8000 != 0 { -1.0f64 } else { 1.0f64 };
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let mantissa = (bits & 0x03ff) as f64;
    match exponent {
        0 => sign * mantissa * 2f64.powi(-24),
        31 => {
            if mantissa == 0.0 {
                sign * f64::INFINITY
            } else {
                f64::NAN
            }
        }
        _ => sign * (mantissa + 1024.0) * 2f64.powi(exponent - 25),
    }
}

/// Decode one CBOR document.
pub fn decode(data: &[u8]) -> Result<CborValue, String> {
    let mut reader = Reader { data, position: 0 };
    let value = reader.value()?;
    if reader.position != data.len() {
        return Err("CBOR_TRAILING_BYTES".to_string());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_map_of_integers_and_bytes() {
        // {1: 2, 3: -7, -2: h'0102'}
        let document = [0xa3, 0x01, 0x02, 0x03, 0x26, 0x21, 0x42, 0x01, 0x02];
        let value = decode(&document).expect("the document decodes");
        let entries = value.as_map().expect("a map");
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].0.as_integer(), Some(1));
        assert_eq!(entries[0].1.as_integer(), Some(2));
        assert_eq!(entries[1].0.as_integer(), Some(3));
        assert_eq!(entries[1].1.as_integer(), Some(-7));
        assert_eq!(entries[2].0.as_integer(), Some(-2));
        assert_eq!(entries[2].1.as_bytes(), Some(&[1u8, 2u8][..]));
    }

    #[test]
    fn decodes_a_text_keyed_map() {
        // {"authData": h'010203'}
        let document = [
            0xa1, 0x68, 0x61, 0x75, 0x74, 0x68, 0x44, 0x61, 0x74, 0x61, 0x43, 0x01, 0x02, 0x03,
        ];
        let value = decode(&document).expect("the document decodes");
        let entries = value.as_map().expect("a map");
        assert_eq!(entries[0].0.as_text(), Some("authData"));
        assert_eq!(entries[0].1.as_bytes(), Some(&[1u8, 2u8, 3u8][..]));
    }

    #[test]
    fn refuses_a_truncated_document() {
        assert!(decode(&[0xa1, 0x01]).is_err());
        assert!(decode(&[]).is_err());
        assert!(
            decode(&[0xa1, 0x01, 0x02, 0x03]).is_err(),
            "trailing bytes are refused"
        );
    }

    #[test]
    fn refuses_an_indefinite_length() {
        assert!(decode(&[0x5f]).is_err());
        assert!(decode(&[0x7f]).is_err());
    }
}
