use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::ProtoError;

pub const HEADER_LEN: usize = 4;

/// A framed packet. `payload` excludes the 4-byte header and shares memory with
/// the read buffer, so cloning it is a refcount bump, not a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: u8,
    pub payload: Bytes,
}

impl Frame {
    pub fn new(id: u8, payload: Bytes) -> Self {
        Self { id, payload }
    }

    pub fn encode_with(&self, header: u8) -> Result<Bytes, ProtoError> {
        let total = HEADER_LEN + self.payload.len();
        if total > u16::MAX as usize {
            return Err(ProtoError::TooLarge(total));
        }
        let mut buf = BytesMut::with_capacity(total);
        buf.put_u8(header);
        buf.put_u8(self.id);
        buf.put_u16_le(total as u16);
        buf.put_slice(&self.payload);
        Ok(buf.freeze())
    }
}

/// Length-prefixed framing shared by W3GS (0xF7), GPS (0xF8) and BNCS (0xFF).
#[derive(Debug, Default, Clone, Copy)]
pub struct HeaderCodec<const H: u8>;

impl<const H: u8> Decoder for HeaderCodec<H> {
    type Item = Frame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, ProtoError> {
        if !src.is_empty() && src[0] != H {
            match src.iter().position(|&b| b == H) {
                Some(pos) => src.advance(pos),
                None => {
                    src.clear();
                    return Ok(None);
                }
            }
        }
        if src.len() < HEADER_LEN {
            return Ok(None);
        }

        let id = src[1];
        let total = u16::from_le_bytes([src[2], src[3]]) as usize;

        if total < HEADER_LEN {
            src.advance(1);
            return Err(ProtoError::BadValue("frame length below header size"));
        }
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }

        src.advance(HEADER_LEN);
        let payload = src.split_to(total - HEADER_LEN).freeze();
        Ok(Some(Frame { id, payload }))
    }
}

impl<const H: u8> Encoder<Bytes> for HeaderCodec<H> {
    type Error = ProtoError;

    /// Packets are pre-encoded once and broadcast as shared `Bytes`, so the
    /// encoder only appends already-framed data.
    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), ProtoError> {
        dst.reserve(item.len());
        dst.put_slice(&item);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::codec::Decoder;

    type Gps = HeaderCodec<0xF8>;
    type Bncs = HeaderCodec<0xFF>;

    #[test]
    fn gps_and_bncs_frame_independently() {
        let mut buf = BytesMut::from(&[0xF8, 0x02, 0x05, 0x00, 0x7B][..]);
        let f = Gps::default().decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, 0x02);
        assert_eq!(&f.payload[..], &[0x7B]);

        let mut buf = BytesMut::from(&[0xFF, 0x50, 0x04, 0x00][..]);
        let f = Bncs::default().decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, 0x50);
        assert!(f.payload.is_empty());
    }

    #[test]
    fn a_bncs_frame_is_not_mistaken_for_a_gps_frame() {
        // 0xFF is not the GPS header, so the GPS codec must resync past it and
        // then find nothing rather than decoding a bogus frame.
        let mut buf = BytesMut::from(&[0xFF, 0x50, 0x04, 0x00][..]);
        assert!(Gps::default().decode(&mut buf).unwrap().is_none());
        assert!(buf.is_empty(), "unusable bytes must be discarded");
    }

    #[test]
    fn encode_with_uses_the_requested_header() {
        let f = Frame::new(0x02, Bytes::from_static(&[1]));
        assert_eq!(&f.encode_with(0xF8).unwrap()[..], &[0xF8, 0x02, 0x05, 0x00, 0x01]);
        assert_eq!(&f.encode_with(0xFF).unwrap()[..], &[0xFF, 0x02, 0x05, 0x00, 0x01]);
    }
}
