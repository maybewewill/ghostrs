use crate::error::ProtoError;
use bytes::{BufMut, Bytes, BytesMut};

pub trait BufExt: bytes::Buf {
    fn try_get_u8(&mut self) -> Result<u8, ProtoError> {
        if self.remaining() < 1 {
            return Err(ProtoError::Truncated {
                need: 1,
                have: self.remaining(),
            });
        }
        Ok(self.get_u8())
    }

    fn try_get_u16_le(&mut self) -> Result<u16, ProtoError> {
        if self.remaining() < 2 {
            return Err(ProtoError::Truncated {
                need: 2,
                have: self.remaining(),
            });
        }
        Ok(self.get_u16_le())
    }

    fn try_get_u32_le(&mut self) -> Result<u32, ProtoError> {
        if self.remaining() < 4 {
            return Err(ProtoError::Truncated {
                need: 4,
                have: self.remaining(),
            });
        }
        Ok(self.get_u32_le())
    }

    fn try_get_bytes(&mut self, n: usize) -> Result<Bytes, ProtoError> {
        if self.remaining() < n {
            return Err(ProtoError::Truncated {
                need: n,
                have: self.remaining(),
            });
        }
        Ok(self.copy_to_bytes(n))
    }

    /// Reads a NUL-terminated string. Non-UTF8 bytes are replaced, never panics.
    fn try_get_cstring(&mut self) -> Result<String, ProtoError> {
        let mut out = Vec::new();
        loop {
            if self.remaining() == 0 {
                return Err(ProtoError::UnterminatedString);
            }
            let b = self.get_u8();
            if b == 0 {
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            out.push(b);
        }
    }
}

impl<T: bytes::Buf + ?Sized> BufExt for T {}

pub fn put_cstring(buf: &mut BytesMut, s: &str) {
    buf.put_slice(s.as_bytes());
    buf.put_u8(0);
}

/// Battle.net statstring encoding: each group of 7 bytes is prefixed by a mask
/// byte whose bit (i+1) is set when payload byte i was even; every payload byte
/// Battle.net statstring encoding matching GHost++ UTIL_EncodeStatString and Warcraft III:
/// Each group of 7 bytes is prefixed by a mask byte.
/// When payload byte was even, it is incremented by 1 and the mask bit is 0.
/// When payload byte was odd, it is kept as-is and the mask bit is set to 1.
pub fn encode_statstring(raw: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(raw.len() + raw.len() / 7 + 1);
    let mut mask = 1u8;

    for i in 0..raw.len() {
        let byte = raw[i];
        if byte % 2 == 0 {
            result.push(byte.wrapping_add(1));
        } else {
            result.push(byte);
            mask |= 1 << ((i % 7) + 1);
        }

        if i % 7 == 6 || i == raw.len() - 1 {
            let insert_pos = result.len() - 1 - (i % 7);
            result.insert(insert_pos, mask);
            mask = 1;
        }
    }

    result
}

pub fn decode_statstring(enc: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(enc.len());
    let mut mask = 0u8;

    for i in 0..enc.len() {
        if i % 8 == 0 {
            mask = enc[i];
        } else {
            if (mask & (1 << (i % 8))) == 0 {
                result.push(enc[i].wrapping_sub(1));
            } else {
                result.push(enc[i]);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{Bytes, BytesMut};

    #[test]
    fn cstring_roundtrip() {
        let mut buf = BytesMut::new();
        put_cstring(&mut buf, "PlayerOne");
        put_cstring(&mut buf, "");
        assert_eq!(buf.len(), 10 + 1);

        let mut b = buf.freeze();
        assert_eq!(b.try_get_cstring().unwrap(), "PlayerOne");
        assert_eq!(b.try_get_cstring().unwrap(), "");
        assert!(b.try_get_cstring().is_err());
    }

    #[test]
    fn cstring_without_terminator_errors_and_does_not_panic() {
        let mut b = Bytes::from_static(b"nope");
        assert!(matches!(
            b.try_get_cstring(),
            Err(ProtoError::UnterminatedString)
        ));
    }

    #[test]
    fn try_get_u32_on_short_buffer_errors() {
        let mut b = Bytes::from_static(&[1, 2]);
        assert!(matches!(
            b.try_get_u32_le(),
            Err(ProtoError::Truncated { need: 4, have: 2 })
        ));
    }

    #[test]
    fn statstring_roundtrip() {
        let raw: Vec<u8> = vec![0x00, 0x01, 0x7F, 0x80, 0xFF, 0x10, 0x00, 0x2A, 0x03];
        let enc = encode_statstring(&raw);
        assert!(
            !enc.contains(&0u8),
            "encoded statstring must not contain NUL"
        );
        assert_eq!(decode_statstring(&enc), raw);
    }
}
