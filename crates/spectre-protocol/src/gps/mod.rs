use bytes::{BufMut, Bytes, BytesMut};

use crate::bytes_ext::BufExt;
use crate::error::ProtoError;
use crate::frame::{Frame, HeaderCodec};

pub const GPS_HEADER: u8 = 0xF8;
pub type GpsCodec = HeaderCodec<GPS_HEADER>;

pub const GPS_ICCUP_HEADER: u8 = 0x6F; // 'o'
pub type IccupGpsCodec = HeaderCodec<GPS_ICCUP_HEADER>;

pub mod ids {
    pub const INIT: u8 = 0x01;
    pub const RECONNECT: u8 = 0x02;
    pub const ACK: u8 = 0x03;
    pub const REJECT: u8 = 0x04;
    pub const FULL: u8 = 0x05;
}

pub mod iccup_ids {
    pub const INIT: u8 = 0x08;
    pub const RECONNECT: u8 = 0x16; // 22
    pub const ACK: u8 = 0x32;       // 50
    pub const REJECT: u8 = 0x64;    // 100
}

pub mod reject_reason {
    pub const NOT_FOUND: u32 = 0x01;
    pub const INVALID_KEY: u32 = 0x02;
}

pub fn init(version: u32, pid: u8, reconnect_key: u32, num_empty_actions: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(10);
    p.put_u32_le(version);
    p.put_u8(pid);
    p.put_u32_le(reconnect_key);
    p.put_u8(num_empty_actions);
    Frame::new(ids::INIT, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("10-byte gps init always fits")
}

pub fn ack(last_packet: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(last_packet);
    Frame::new(ids::ACK, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("4-byte gps ack always fits")
}

pub fn reconnect_ok(last_packet: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(last_packet);
    Frame::new(ids::RECONNECT, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("4-byte gps reconnect always fits")
}

pub fn reject(reason: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(reason);
    Frame::new(ids::REJECT, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("4-byte gps reject always fits")
}

pub fn full(pid: u8, reconnect_key: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u8(pid);
    p.put_u32_le(reconnect_key);
    Frame::new(ids::FULL, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("5-byte gps full always fits")
}

pub fn iccup_init(reconn_port: u16, pid: u8, reconnect_key: u32, num_empty_actions: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(12);
    p.put_u16_le(reconn_port);
    p.put_u32_le(0);
    p.put_u8(pid);
    p.put_u32_le(reconnect_key);
    p.put_u8(num_empty_actions);
    Frame::new(iccup_ids::INIT, p.freeze())
        .encode_with(GPS_ICCUP_HEADER)
        .expect("12-byte iccup init always fits")
}

pub fn iccup_reconnect_ok(last_packet: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(8);
    p.put_u32_le(last_packet);
    p.put_u32_le(0);
    Frame::new(iccup_ids::RECONNECT, p.freeze())
        .encode_with(GPS_ICCUP_HEADER)
        .expect("8-byte iccup reconnect ok always fits")
}

pub fn iccup_ack(last_packet: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(last_packet);
    Frame::new(iccup_ids::ACK, p.freeze())
        .encode_with(GPS_ICCUP_HEADER)
        .expect("4-byte iccup ack always fits")
}

pub fn iccup_reject(reason: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(reason);
    Frame::new(iccup_ids::REJECT, p.freeze())
        .encode_with(GPS_ICCUP_HEADER)
        .expect("4-byte iccup reject always fits")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectReq {
    pub pid: u8,
    pub reconnect_key: u32,
    pub last_packet: u32,
}

pub fn decode_reconnect(payload: &Bytes) -> Result<ReconnectReq, ProtoError> {
    let mut b = payload.clone();
    Ok(ReconnectReq {
        pid: b.try_get_u8()?,
        reconnect_key: b.try_get_u32_le()?,
        last_packet: b.try_get_u32_le()?,
    })
}

pub fn decode_full(payload: &Bytes) -> Result<(u8, u32), ProtoError> {
    let mut b = payload.clone();
    let pid = b.try_get_u8()?;
    let key = b.try_get_u32_le()?;
    Ok((pid, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_roundtrip() {
        let mut p = BytesMut::new();
        p.put_u8(3);
        p.put_u32_le(0xCAFE_BABE);
        p.put_u32_le(1234);
        let r = decode_reconnect(&p.freeze()).unwrap();
        assert_eq!(r.pid, 3);
        assert_eq!(r.reconnect_key, 0xCAFE_BABE);
        assert_eq!(r.last_packet, 1234);
    }

    #[test]
    fn init_is_framed_with_the_gps_header() {
        let b = init(1, 3, 0xCAFE_BABE, 0);
        assert_eq!(b[0], GPS_HEADER);
        assert_eq!(b[1], ids::INIT);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
    }

    #[test]
    fn truncated_reconnect_errors() {
        assert!(decode_reconnect(&Bytes::from_static(&[3, 0, 0])).is_err());
    }

    #[test]
    fn full_token_roundtrip() {
        let b = full(7, 0xDEAD_BEEF);
        assert_eq!(b[0], GPS_HEADER);
        assert_eq!(b[1], ids::FULL);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
        let (pid, key) = decode_full(&b.slice(4..)).unwrap();
        assert_eq!(pid, 7);
        assert_eq!(key, 0xDEAD_BEEF);
    }

    #[test]
    fn decode_full_rejects_truncated() {
        assert!(decode_full(&Bytes::from_static(&[7, 0, 0])).is_err());
    }
}
