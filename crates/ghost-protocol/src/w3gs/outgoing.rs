use bytes::{BufMut, Bytes, BytesMut};

use super::codec::Frame;
use super::ids;
use super::slot::SlotInfo;
use crate::bytes_ext::put_cstring;
use crate::error::ProtoError;

/// One player action as it appears inside INCOMING_ACTION:
/// pid (1) + length (2, LE) + body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionBlock {
    pub pid: u8,
    pub data: Bytes,
}

impl ActionBlock {
    pub fn wire_len(&self) -> usize {
        self.data.len() + 3
    }

    fn put(&self, buf: &mut BytesMut) {
        buf.put_u8(self.pid);
        buf.put_u16_le(self.data.len() as u16);
        buf.put_slice(&self.data);
    }
}

fn action_payload(actions: &[ActionBlock], send_interval: u16) -> Result<Bytes, ProtoError> {
    let body_len: usize = actions.iter().map(ActionBlock::wire_len).sum();
    for a in actions {
        if a.data.len() > u16::MAX as usize {
            return Err(ProtoError::TooLarge(a.data.len()));
        }
    }

    let mut sub = BytesMut::with_capacity(body_len);
    for a in actions {
        a.put(&mut sub);
    }

    let mut payload = BytesMut::with_capacity(4 + body_len);
    payload.put_u16_le(send_interval);
    if actions.is_empty() {
        // An empty tick carries no CRC field, matching src/gameprotocol.rs:358.
        return Ok(payload.freeze());
    }
    let crc = crc32fast::hash(&sub);
    payload.put_u8((crc & 0xFF) as u8);
    payload.put_u8(((crc >> 8) & 0xFF) as u8);
    payload.put_slice(&sub);
    Ok(payload.freeze())
}

/// W3GS_INCOMING_ACTION (0x0C): the per-tick action broadcast.
pub fn incoming_action(actions: &[ActionBlock], send_interval: u16) -> Result<Bytes, ProtoError> {
    Frame::new(ids::INCOMING_ACTION, action_payload(actions, send_interval)?).encode()
}

/// W3GS_INCOMING_ACTION2 (0x48): overflow packet, always send_interval 0.
pub fn incoming_action2(actions: &[ActionBlock]) -> Result<Bytes, ProtoError> {
    Frame::new(ids::INCOMING_ACTION2, action_payload(actions, 0)?).encode()
}

pub fn ping_from_host(ticks: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(ticks);
    Frame::new(ids::PING_FROM_HOST, p.freeze())
        .encode()
        .expect("4-byte ping always fits")
}

fn slot_block(slots: &[SlotInfo], random_seed: u32, layout_style: u8, player_slots: u8) -> BytesMut {
    let mut p = BytesMut::with_capacity(3 + slots.len() * SlotInfo::WIRE_LEN + 6);
    let block_len = 1 + slots.len() * SlotInfo::WIRE_LEN + 4 + 1 + 1;
    p.put_u16_le(block_len as u16);
    p.put_u8(slots.len() as u8);
    for s in slots {
        s.encode(&mut p);
    }
    p.put_u32_le(random_seed);
    p.put_u8(layout_style);
    p.put_u8(player_slots);
    p
}

/// W3GS_SLOTINFO (0x09).
pub fn slot_info(
    slots: &[SlotInfo],
    random_seed: u32,
    layout_style: u8,
    player_slots: u8,
) -> Result<Bytes, ProtoError> {
    let p = slot_block(slots, random_seed, layout_style, player_slots);
    Frame::new(ids::SLOT_INFO, p.freeze()).encode()
}

/// W3GS_SLOTINFOJOIN (0x04): slot table plus the joiner's own identity.
pub fn slot_info_join(
    pid: u8,
    port: u16,
    external_ip: [u8; 4],
    slots: &[SlotInfo],
    random_seed: u32,
    layout_style: u8,
    player_slots: u8,
) -> Result<Bytes, ProtoError> {
    let mut p = slot_block(slots, random_seed, layout_style, player_slots);
    p.put_u8(pid);
    p.put_u16_le(2); // AF_INET
    p.put_u16_le(port);
    p.put_slice(&external_ip);
    p.put_slice(&[0; 8]); // sockaddr padding
    Frame::new(ids::SLOT_INFO_JOIN, p.freeze()).encode()
}

pub fn reject_join(reason: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(reason);
    Frame::new(ids::REJECT_JOIN, p.freeze())
        .encode()
        .expect("4-byte reject always fits")
}

/// W3GS_PLAYERINFO (0x06).
pub fn player_info(
    pid: u8,
    name: &str,
    external_ip: [u8; 4],
    internal_ip: [u8; 4],
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(32 + name.len());
    p.put_u32_le(2); // player join counter
    p.put_u8(pid);
    put_cstring(&mut p, name);
    p.put_u8(1); // size of following unknown block
    p.put_u8(0);
    // external sockaddr
    p.put_u16_le(2);
    p.put_u16_le(0);
    p.put_slice(&external_ip);
    p.put_slice(&[0; 8]);
    // internal sockaddr
    p.put_u16_le(2);
    p.put_u16_le(0);
    p.put_slice(&internal_ip);
    p.put_slice(&[0; 8]);
    Frame::new(ids::PLAYER_INFO, p.freeze()).encode()
}

pub fn player_leave_others(pid: u8, left_code: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u8(pid);
    p.put_u32_le(left_code);
    Frame::new(ids::PLAYER_LEAVE_OTHERS, p.freeze())
        .encode()
        .expect("5-byte leave always fits")
}

pub fn game_loaded_others(pid: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(1);
    p.put_u8(pid);
    Frame::new(ids::GAME_LOADED_OTHERS, p.freeze())
        .encode()
        .expect("1-byte loaded always fits")
}

pub fn countdown_start() -> Bytes {
    Frame::new(ids::COUNTDOWN_START, Bytes::new())
        .encode()
        .expect("empty frame always fits")
}

pub fn countdown_end() -> Bytes {
    Frame::new(ids::COUNTDOWN_END, Bytes::new())
        .encode()
        .expect("empty frame always fits")
}

/// W3GS_CHAT_FROM_HOST (0x0F).
pub fn chat_from_host(
    from_pid: u8,
    to_pids: &[u8],
    flag: u8,
    extra: &[u8],
    message: &str,
) -> Result<Bytes, ProtoError> {
    if to_pids.is_empty() {
        return Err(ProtoError::BadValue("chat_from_host needs at least one recipient"));
    }
    let mut p = BytesMut::with_capacity(4 + to_pids.len() + extra.len() + message.len());
    p.put_u8(to_pids.len() as u8);
    p.put_slice(to_pids);
    p.put_u8(from_pid);
    p.put_u8(flag);
    p.put_slice(extra);
    put_cstring(&mut p, message);
    Frame::new(ids::CHAT_FROM_HOST, p.freeze()).encode()
}

/// W3GS_START_LAG (0x10): pid plus how long that player has been lagging.
pub fn start_lag(laggers: &[(u8, u32)]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(1 + laggers.len() * 5);
    p.put_u8(laggers.len() as u8);
    for &(pid, lag_ms) in laggers {
        p.put_u8(pid);
        p.put_u32_le(lag_ms);
    }
    Frame::new(ids::START_LAG, p.freeze()).encode()
}

pub fn stop_lag(pid: u8, lag_ms: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u8(pid);
    p.put_u32_le(lag_ms);
    Frame::new(ids::STOP_LAG, p.freeze())
        .encode()
        .expect("5-byte stoplag always fits")
}

/// W3GS_MAPCHECK (0x3D).
pub fn map_check(
    map_path: &str,
    map_size: u32,
    map_info: u32,
    map_crc: u32,
    map_sha1: [u8; 20],
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(40 + map_path.len());
    p.put_u32_le(1);
    put_cstring(&mut p, map_path);
    p.put_u32_le(map_size);
    p.put_u32_le(map_info);
    p.put_u32_le(map_crc);
    p.put_slice(&map_sha1);
    Frame::new(ids::MAP_CHECK, p.freeze()).encode()
}

pub fn start_download(from_pid: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u32_le(1);
    p.put_u8(from_pid);
    Frame::new(ids::START_DOWNLOAD, p.freeze())
        .encode()
        .expect("5-byte startdownload always fits")
}

/// W3GS_MAPPART (0x43). `chunk` must be at most 1442 bytes; the CRC covers it.
pub fn map_part(from_pid: u8, to_pid: u8, start: u32, chunk: &[u8]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(14 + chunk.len());
    p.put_u8(to_pid);
    p.put_u8(from_pid);
    p.put_u32_le(1);
    p.put_u32_le(start);
    p.put_u32_le(crc32fast::hash(chunk));
    p.put_slice(chunk);
    Frame::new(ids::MAP_PART, p.freeze()).encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_action_layout_and_crc() {
        let actions = vec![
            ActionBlock { pid: 1, data: Bytes::from_static(&[0x10, 0x20]) },
            ActionBlock { pid: 2, data: Bytes::from_static(&[0x30]) },
        ];
        let framed = incoming_action(&actions, 100).unwrap();

        // Frame header
        assert_eq!(framed[0], 0xF7);
        assert_eq!(framed[1], ids::INCOMING_ACTION);
        assert_eq!(u16::from_le_bytes([framed[2], framed[3]]) as usize, framed.len());

        // send interval, then 2-byte CRC, then the action blocks
        assert_eq!(u16::from_le_bytes([framed[4], framed[5]]), 100);

        let mut subpacket = BytesMut::new();
        subpacket.put_u8(1);
        subpacket.put_u16_le(2);
        subpacket.put_slice(&[0x10, 0x20]);
        subpacket.put_u8(2);
        subpacket.put_u16_le(1);
        subpacket.put_slice(&[0x30]);

        let full = crc32fast::hash(&subpacket);
        assert_eq!(framed[6], (full & 0xFF) as u8);
        assert_eq!(framed[7], ((full >> 8) & 0xFF) as u8);
        assert_eq!(&framed[8..], &subpacket[..]);
    }

    #[test]
    fn empty_action_tick_still_carries_send_interval() {
        let framed = incoming_action(&[], 100).unwrap();
        assert_eq!(framed.len(), 4 + 2);
        assert_eq!(u16::from_le_bytes([framed[4], framed[5]]), 100);
    }

    #[test]
    fn incoming_action2_uses_zero_send_interval() {
        let actions = vec![ActionBlock { pid: 1, data: Bytes::from_static(&[9]) }];
        let framed = incoming_action2(&actions).unwrap();
        assert_eq!(framed[1], ids::INCOMING_ACTION2);
        assert_eq!(u16::from_le_bytes([framed[4], framed[5]]), 0);
    }

    #[test]
    fn action_block_wire_len_matches_encoded_size() {
        let a = ActionBlock { pid: 1, data: Bytes::from_static(&[0; 17]) };
        assert_eq!(a.wire_len(), 20);
        let framed = incoming_action(std::slice::from_ref(&a), 100).unwrap();
        assert_eq!(framed.len(), 4 + 2 + 2 + a.wire_len());
    }

    #[test]
    fn slot_info_encodes_nine_bytes_per_slot() {
        let slots = vec![SlotInfo::default(); 12];
        let framed = slot_info(&slots, 42, 0, 12).unwrap();
        // header 4 + u16 blocklen + u8 numslots + 12*9 + u32 seed + u8 layout + u8 playerslots
        assert_eq!(framed.len(), 4 + 2 + 1 + 12 * 9 + 4 + 1 + 1);
        assert_eq!(framed[1], ids::SLOT_INFO);
    }

    #[test]
    fn map_part_over_u16_is_rejected() {
        let chunk = vec![0u8; 70_000];
        assert!(matches!(
            map_part(1, 2, 0, &chunk),
            Err(ProtoError::TooLarge(_))
        ));
    }

    #[test]
    fn player_info_contains_name_and_addresses() {
        let framed = player_info(3, "Slash", [1, 2, 3, 4], [192, 168, 0, 5]).unwrap();
        assert_eq!(framed[1], ids::PLAYER_INFO);
        assert!(framed.windows(5).any(|w| w == b"Slash"));
    }
}
