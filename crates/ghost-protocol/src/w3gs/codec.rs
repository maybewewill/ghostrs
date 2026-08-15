use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use super::ids;
use crate::error::ProtoError;

pub const W3GS_HEADER: u8 = 0xF7;
const HEADER_LEN: usize = 4;

/// A framed W3GS packet. `payload` excludes the 4-byte header and shares memory
/// with the read buffer, so cloning it is a refcount bump, not a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: u8,
    pub payload: Bytes,
}

impl Frame {
    pub fn new(id: u8, payload: Bytes) -> Self {
        Self { id, payload }
    }

    pub fn encode(&self) -> Result<Bytes, ProtoError> {
        let total = HEADER_LEN + self.payload.len();
        if total > u16::MAX as usize {
            return Err(ProtoError::TooLarge(total));
        }
        let mut buf = BytesMut::with_capacity(total);
        buf.put_u8(W3GS_HEADER);
        buf.put_u8(self.id);
        buf.put_u16_le(total as u16);
        buf.put_slice(&self.payload);
        Ok(buf.freeze())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct W3gsCodec;

impl Decoder for W3gsCodec {
    type Item = Frame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, ProtoError> {
        // Resync: drop anything before the next header byte.
        if !src.is_empty() && src[0] != W3GS_HEADER {
            match src.iter().position(|&b| b == W3GS_HEADER) {
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
            src.advance(1); // skip the bogus header byte so the next call resyncs
            return Err(ProtoError::BadValue("w3gs length below header size"));
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

impl Encoder<Bytes> for W3gsCodec {
    type Error = ProtoError;

    /// Frames are pre-encoded once by the engine and broadcast as shared `Bytes`,
    /// so the encoder only appends already-framed data.
    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), ProtoError> {
        dst.reserve(item.len());
        dst.put_slice(&item);
        Ok(())
    }
}

/// True for ids the engine acts on. Unknown ids are still framed and forwarded
/// so the stream never desyncs; the engine decides whether to ignore them.
pub fn is_known_id(id: u8) -> bool {
    matches!(
        id,
        ids::REQ_JOIN
            | ids::LEAVE_GAME
            | ids::GAME_LOADED_SELF
            | ids::OUTGOING_ACTION
            | ids::OUTGOING_KEEPALIVE
            | ids::CHAT_TO_HOST
            | ids::DROP_REQ
            | ids::SEARCH_GAME
            | ids::MAP_SIZE
            | ids::MAP_PART_OK
            | ids::PONG_TO_HOST
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::codec::Decoder;

    #[test]
    fn decodes_one_frame_and_leaves_the_rest() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0xF7, 0x1E, 0x06, 0x00, 0xAA, 0xBB]);
        buf.extend_from_slice(&[0xF7, 0x27, 0x04, 0x00]);

        let mut codec = W3gsCodec;
        let f = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, ids::REQ_JOIN);
        assert_eq!(&f.payload[..], &[0xAA, 0xBB]);
        assert_eq!(buf.len(), 4, "second frame must stay in the buffer");
    }

    #[test]
    fn returns_none_until_the_whole_frame_arrives() {
        let mut buf = BytesMut::from(&[0xF7, 0x1E, 0x08, 0x00, 0x01][..]);
        let mut codec = W3gsCodec;
        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert_eq!(buf.len(), 5, "partial frame must not be consumed");
    }

    #[test]
    fn unknown_packet_id_is_consumed_not_desynced() {
        // Regression: legacy src/protocol/w3gs.rs:160 errored before advancing,
        // leaving the byte stream permanently misaligned.
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0xF7, 0xEE, 0x05, 0x00, 0x99]);
        buf.extend_from_slice(&[0xF7, 0x27, 0x04, 0x00]);

        let mut codec = W3gsCodec;
        let unknown = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(unknown.id, 0xEE, "unknown ids are forwarded verbatim");
        let next = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(next.id, ids::OUTGOING_KEEPALIVE);
    }

    #[test]
    fn resyncs_after_garbage_prefix() {
        let mut buf = BytesMut::from(&[0x00, 0x11, 0xF7, 0x27, 0x04, 0x00][..]);
        let mut codec = W3gsCodec;
        let f = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, ids::OUTGOING_KEEPALIVE);
    }

    #[test]
    fn length_below_header_size_is_rejected_and_byte_skipped() {
        let mut buf = BytesMut::from(&[0xF7, 0x27, 0x02, 0x00, 0xF7, 0x27, 0x04, 0x00][..]);
        let mut codec = W3gsCodec;
        assert!(codec.decode(&mut buf).is_err());
        let f = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, ids::OUTGOING_KEEPALIVE);
    }

    #[test]
    fn oversized_payload_errors_instead_of_truncating() {
        // Regression: legacy encode cast total_len to u16 unchecked.
        let payload = Bytes::from(vec![0u8; 70_000]);
        let frame = Frame::new(ids::MAP_PART, payload);
        assert!(matches!(frame.encode(), Err(ProtoError::TooLarge(70_004))));
    }

    #[test]
    fn encode_decode_roundtrip() {
        let frame = Frame::new(ids::PING_FROM_HOST, Bytes::from_static(&[1, 2, 3, 4]));
        let mut buf = BytesMut::from(&frame.encode().unwrap()[..]);
        let back = W3gsCodec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(back, frame);
        assert!(buf.is_empty());
    }

    proptest::proptest! {
        #[test]
        fn decoder_never_panics_on_arbitrary_input(data: Vec<u8>) {
            let mut buf = BytesMut::from(&data[..]);
            let mut codec = W3gsCodec;
            for _ in 0..data.len() + 1 {
                let before = buf.len();
                match codec.decode(&mut buf) {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(_) => {}
                }
                if buf.len() == before {
                    break;
                }
            }
        }
    }
}
