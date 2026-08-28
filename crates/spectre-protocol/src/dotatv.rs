use bytes::{BufMut, Bytes, BytesMut};

use crate::bytes_ext::{BufExt, put_cstring};
use crate::error::ProtoError;
use crate::frame::{Frame as RawFrame, HeaderCodec};
use crate::w3gs::slot::SlotInfo;

pub const DOTATV_HEADER: u8 = 0xFD;
pub type DotaTvCodec = HeaderCodec<DOTATV_HEADER>;
pub type Frame = RawFrame;

pub mod ids {
    pub const HELLO: u8 = 0x01;
    pub const GAME_START_SNAPSHOT: u8 = 0x02;
    pub const PLAYER: u8 = 0x03;
    pub const ACTION: u8 = 0x04;
    pub const CHAT: u8 = 0x05;
    pub const GAME_OVER: u8 = 0x06;
    pub const HISTORY_END: u8 = 0x07;
    pub const SUBSCRIBE: u8 = 0x80;
    pub const CLIENT_CHAT: u8 = 0x81;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameStartSnapshot {
    pub game_name: String,
    pub map_path: String,
    pub map_size: u32,
    pub map_info_crc: u32,
    pub map_crc: u32,
    pub map_sha1: [u8; 20],
    pub stat_string: Vec<u8>,
    pub random_seed: u32,
    pub layout_style: u8,
    pub player_slots: u8,
    pub war3_version: u8,
    pub is_tft: bool,
    pub slots: Vec<SlotInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerInfo {
    pub pid: u8,
    pub name: String,
    pub colour: u8,
    pub team: u8,
    pub race: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpectatorChat {
    pub sender: String,
    pub text: String,
}

pub fn encode_hello(version: u16, server_name: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(2 + server_name.len() + 1);
    p.put_u16_le(version);
    put_cstring(&mut p, server_name);
    Frame::new(ids::HELLO, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_hello(payload: &[u8]) -> Result<(u16, String), ProtoError> {
    let mut b = payload;
    let version = b.try_get_u16_le()?;
    let server_name = b.try_get_cstring()?;
    Ok((version, server_name))
}

pub fn encode_snapshot(snap: &GameStartSnapshot) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(
        snap.game_name.len()
            + 1
            + snap.map_path.len()
            + 1
            + 4
            + 4
            + 4
            + 20
            + snap.stat_string.len()
            + 1
            + 4
            + 1
            + 1
            + 1
            + 1
            + 1
            + snap.slots.len() * SlotInfo::WIRE_LEN,
    );
    put_cstring(&mut p, &snap.game_name);
    put_cstring(&mut p, &snap.map_path);
    p.put_u32_le(snap.map_size);
    p.put_u32_le(snap.map_info_crc);
    p.put_u32_le(snap.map_crc);
    p.put_slice(&snap.map_sha1);
    p.put_slice(&snap.stat_string);
    p.put_u8(0);
    p.put_u32_le(snap.random_seed);
    p.put_u8(snap.layout_style);
    p.put_u8(snap.player_slots);
    p.put_u8(snap.war3_version);
    p.put_u8(if snap.is_tft { 1 } else { 0 });
    if snap.slots.len() > u8::MAX as usize {
        return Err(ProtoError::BadValue("too many slots in snapshot"));
    }
    p.put_u8(snap.slots.len() as u8);
    for slot in &snap.slots {
        slot.encode(&mut p);
    }
    Frame::new(ids::GAME_START_SNAPSHOT, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_snapshot(payload: &[u8]) -> Result<GameStartSnapshot, ProtoError> {
    let mut b = payload;
    let game_name = b.try_get_cstring()?;
    let map_path = b.try_get_cstring()?;
    let map_size = b.try_get_u32_le()?;
    let map_info_crc = b.try_get_u32_le()?;
    let map_crc = b.try_get_u32_le()?;
    let sha1_bytes = b.try_get_bytes(20)?;
    let mut map_sha1 = [0u8; 20];
    map_sha1.copy_from_slice(&sha1_bytes);

    let mut stat_string = Vec::new();
    loop {
        let byte = match b.try_get_u8() {
            Ok(byte) => byte,
            Err(ProtoError::Truncated { .. }) => return Err(ProtoError::UnterminatedString),
            Err(e) => return Err(e),
        };
        if byte == 0 {
            break;
        }
        stat_string.push(byte);
    }

    let random_seed = b.try_get_u32_le()?;
    let layout_style = b.try_get_u8()?;
    let player_slots = b.try_get_u8()?;
    let war3_version = b.try_get_u8()?;
    let is_tft = b.try_get_u8()? != 0;
    let num_slots = b.try_get_u8()? as usize;
    let mut slots = Vec::with_capacity(num_slots);
    for _ in 0..num_slots {
        let pid = b.try_get_u8()?;
        let download_status = b.try_get_u8()?;
        let slot_status = b.try_get_u8()?;
        let computer = b.try_get_u8()?;
        let team = b.try_get_u8()?;
        let colour = b.try_get_u8()?;
        let race = b.try_get_u8()?;
        let computer_type = b.try_get_u8()?;
        let handicap = b.try_get_u8()?;
        slots.push(SlotInfo {
            pid,
            download_status,
            slot_status,
            computer,
            team,
            colour,
            race,
            computer_type,
            handicap,
        });
    }

    Ok(GameStartSnapshot {
        game_name,
        map_path,
        map_size,
        map_info_crc,
        map_crc,
        map_sha1,
        stat_string,
        random_seed,
        layout_style,
        player_slots,
        war3_version,
        is_tft,
        slots,
    })
}

pub fn encode_player(
    pid: u8,
    name: &str,
    colour: u8,
    team: u8,
    race: u8,
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(1 + name.len() + 1 + 3);
    p.put_u8(pid);
    put_cstring(&mut p, name);
    p.put_u8(colour);
    p.put_u8(team);
    p.put_u8(race);
    Frame::new(ids::PLAYER, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_player(payload: &[u8]) -> Result<PlayerInfo, ProtoError> {
    let mut b = payload;
    let pid = b.try_get_u8()?;
    let name = b.try_get_cstring()?;
    let colour = b.try_get_u8()?;
    let team = b.try_get_u8()?;
    let race = b.try_get_u8()?;
    Ok(PlayerInfo {
        pid,
        name,
        colour,
        team,
        race,
    })
}

pub fn encode_action(w3gs_raw_frame: &[u8]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(w3gs_raw_frame.len());
    p.put_slice(w3gs_raw_frame);
    Frame::new(ids::ACTION, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_action(payload: &[u8]) -> Result<Bytes, ProtoError> {
    if payload.len() < 4 {
        return Err(ProtoError::Truncated {
            need: 4,
            have: payload.len(),
        });
    }
    if payload[0] != crate::w3gs::codec::W3GS_HEADER {
        return Err(ProtoError::BadValue("action frame missing 0xF7 header"));
    }
    let len = u16::from_le_bytes([payload[2], payload[3]]) as usize;
    if len < 4 {
        return Err(ProtoError::BadValue(
            "action frame length below header size",
        ));
    }
    if payload.len() < len {
        return Err(ProtoError::Truncated {
            need: len,
            have: payload.len(),
        });
    }
    Ok(Bytes::copy_from_slice(&payload[..len]))
}

pub fn encode_chat(sender: &str, text: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(sender.len() + 1 + text.len() + 1);
    put_cstring(&mut p, sender);
    put_cstring(&mut p, text);
    Frame::new(ids::CHAT, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_chat(payload: &[u8]) -> Result<SpectatorChat, ProtoError> {
    let mut b = payload;
    let sender = b.try_get_cstring()?;
    let text = b.try_get_cstring()?;
    Ok(SpectatorChat { sender, text })
}

pub fn encode_game_over(winner: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(winner.len() + 1);
    put_cstring(&mut p, winner);
    Frame::new(ids::GAME_OVER, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_game_over(payload: &[u8]) -> Result<String, ProtoError> {
    let mut b = payload;
    b.try_get_cstring()
}

pub fn encode_history_end(count: u32) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(count);
    Frame::new(ids::HISTORY_END, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_history_end(payload: &[u8]) -> Result<u32, ProtoError> {
    let mut b = payload;
    b.try_get_u32_le()
}

pub fn encode_subscribe(client_version: u16) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(2);
    p.put_u16_le(client_version);
    Frame::new(ids::SUBSCRIBE, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_subscribe(payload: &[u8]) -> Result<u16, ProtoError> {
    let mut b = payload;
    b.try_get_u16_le()
}

pub fn encode_client_chat(text: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(text.len() + 1);
    put_cstring(&mut p, text);
    Frame::new(ids::CLIENT_CHAT, p.freeze()).encode_with(DOTATV_HEADER)
}

pub fn decode_client_chat(payload: &[u8]) -> Result<String, ProtoError> {
    let mut b = payload;
    b.try_get_cstring()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::codec::Decoder;

    #[test]
    fn test_encode_hello_exact_bytes() {
        let bytes = encode_hello(1, "spectre").unwrap();
        let expected: &[u8] = &[
            0xFD, 0x01, 0x0E, 0x00, 0x01, 0x00, 0x73, 0x70, 0x65, 0x63, 0x74, 0x72, 0x65, 0x00,
        ];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_snapshot_exact_bytes() {
        let snap = GameStartSnapshot {
            game_name: "DotA Live".to_string(),
            map_path: "Maps\\dota.w3x".to_string(),
            map_size: 0x00BC_614E,
            map_info_crc: 0x1122_3344,
            map_crc: 0x5566_7788,
            map_sha1: [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
            ],
            stat_string: vec![0xAA, 0xBB, 0xCC],
            random_seed: 42,
            layout_style: 0,
            player_slots: 10,
            war3_version: 26,
            is_tft: true,
            slots: vec![SlotInfo {
                pid: 0,
                download_status: 255,
                slot_status: 2,
                computer: 0,
                team: 0,
                colour: 1,
                race: 1,
                computer_type: 0,
                handicap: 100,
            }],
        };
        let bytes = encode_snapshot(&snap).unwrap();
        let expected: &[u8] = &[
            0xFD, 0x02, 82, 0x00, 0x44, 0x6F, 0x74, 0x41, 0x20, 0x4C, 0x69, 0x76, 0x65, 0x00, 0x4D,
            0x61, 0x70, 0x73, 0x5C, 0x64, 0x6F, 0x74, 0x61, 0x2E, 0x77, 0x33, 0x78, 0x00, 0x4E,
            0x61, 0xBC, 0x00, 0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55, 1, 2, 3, 4, 5, 6, 7,
            8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 0xAA, 0xBB, 0xCC, 0x00, 0x2A, 0x00,
            0x00, 0x00, 0x00, 0x0A, 0x1A, 0x01, 0x01, 0x00, 0xFF, 0x02, 0x00, 0x00, 0x01, 0x01,
            0x00, 0x64,
        ];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_player_exact_bytes() {
        let bytes = encode_player(1, "Player1", 1, 0, 1).unwrap();
        let expected: &[u8] = &[
            0xFD, 0x03, 0x10, 0x00, 0x01, 0x50, 0x6C, 0x61, 0x79, 0x65, 0x72, 0x31, 0x00, 0x01,
            0x00, 0x01,
        ];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_action_exact_bytes() {
        let raw_action = [0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00];
        let bytes = encode_action(&raw_action).unwrap();
        let expected: &[u8] = &[0xFD, 0x04, 0x0A, 0x00, 0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_chat_exact_bytes() {
        let bytes = encode_chat("Host", "Welcome!").unwrap();
        let expected: &[u8] = &[
            0xFD, 0x05, 0x12, 0x00, 0x48, 0x6F, 0x73, 0x74, 0x00, 0x57, 0x65, 0x6C, 0x63, 0x6F,
            0x6D, 0x65, 0x21, 0x00,
        ];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_game_over_exact_bytes() {
        let bytes = encode_game_over("Sentinel").unwrap();
        let expected: &[u8] = &[
            0xFD, 0x06, 0x0D, 0x00, 0x53, 0x65, 0x6E, 0x74, 0x69, 0x6E, 0x65, 0x6C, 0x00,
        ];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_history_end_exact_bytes() {
        let bytes = encode_history_end(1000).unwrap();
        let expected: &[u8] = &[0xFD, 0x07, 0x08, 0x00, 0xE8, 0x03, 0x00, 0x00];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_subscribe_exact_bytes() {
        let bytes = encode_subscribe(1).unwrap();
        let expected: &[u8] = &[0xFD, 0x80, 0x06, 0x00, 0x01, 0x00];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_encode_client_chat_exact_bytes() {
        let bytes = encode_client_chat("GG WP").unwrap();
        let expected: &[u8] = &[0xFD, 0x81, 0x0A, 0x00, 0x47, 0x47, 0x20, 0x57, 0x50, 0x00];
        assert_eq!(&bytes[..], expected);
    }

    #[test]
    fn test_hello_roundtrip() {
        let raw = encode_hello(42, "MyServer").unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::HELLO);
        let (ver, name) = decode_hello(&frame.payload).unwrap();
        assert_eq!(ver, 42);
        assert_eq!(name, "MyServer");
    }

    #[test]
    fn test_snapshot_roundtrip() {
        let snap = GameStartSnapshot {
            game_name: "Test Game".to_string(),
            map_path: "Maps\\Test\\Map.w3x".to_string(),
            map_size: 999_999,
            map_info_crc: 0x1234_5678,
            map_crc: 0x9ABC_DEF0,
            map_sha1: [7u8; 20],
            stat_string: vec![1, 3, 5, 7, 9],
            random_seed: 987_654,
            layout_style: 1,
            player_slots: 12,
            war3_version: 26,
            is_tft: false,
            slots: vec![
                SlotInfo {
                    pid: 1,
                    download_status: 100,
                    slot_status: 2,
                    computer: 0,
                    team: 0,
                    colour: 1,
                    race: 1,
                    computer_type: 0,
                    handicap: 100,
                },
                SlotInfo {
                    pid: 2,
                    download_status: 50,
                    slot_status: 1,
                    computer: 1,
                    team: 1,
                    colour: 2,
                    race: 2,
                    computer_type: 1,
                    handicap: 80,
                },
            ],
        };
        let raw = encode_snapshot(&snap).unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::GAME_START_SNAPSHOT);
        let decoded = decode_snapshot(&frame.payload).unwrap();
        assert_eq!(decoded, snap);
    }

    #[test]
    fn test_player_roundtrip() {
        let raw = encode_player(5, "Spectator", 3, 1, 2).unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::PLAYER);
        let p = decode_player(&frame.payload).unwrap();
        assert_eq!(p.pid, 5);
        assert_eq!(p.name, "Spectator");
        assert_eq!(p.colour, 3);
        assert_eq!(p.team, 1);
        assert_eq!(p.race, 2);
    }

    #[test]
    fn test_action_roundtrip() {
        let action_payload = &[0xF7, 0x0C, 0x08, 0x00, 0x64, 0x00, 0xAA, 0xBB];
        let raw = encode_action(action_payload).unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::ACTION);
        let act = decode_action(&frame.payload).unwrap();
        assert_eq!(&act[..], action_payload);
    }

    #[test]
    fn test_chat_roundtrip() {
        let raw = encode_chat("Alice", "Hello World").unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::CHAT);
        let chat = decode_chat(&frame.payload).unwrap();
        assert_eq!(chat.sender, "Alice");
        assert_eq!(chat.text, "Hello World");
    }

    #[test]
    fn test_game_over_roundtrip() {
        let raw = encode_game_over("Scourge").unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::GAME_OVER);
        let winner = decode_game_over(&frame.payload).unwrap();
        assert_eq!(winner, "Scourge");
    }

    #[test]
    fn test_history_end_roundtrip() {
        let raw = encode_history_end(42_000).unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::HISTORY_END);
        let count = decode_history_end(&frame.payload).unwrap();
        assert_eq!(count, 42_000);
    }

    #[test]
    fn test_subscribe_roundtrip() {
        let raw = encode_subscribe(1).unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::SUBSCRIBE);
        let ver = decode_subscribe(&frame.payload).unwrap();
        assert_eq!(ver, 1);
    }

    #[test]
    fn test_client_chat_roundtrip() {
        let raw = encode_client_chat("Spectator Chat Text").unwrap();
        let mut codec = DotaTvCodec::default();
        let mut buf = BytesMut::from(&raw[..]);
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::CLIENT_CHAT);
        let text = decode_client_chat(&frame.payload).unwrap();
        assert_eq!(text, "Spectator Chat Text");
    }

    #[test]
    fn test_decode_hello_truncation() {
        assert!(matches!(
            decode_hello(&[]),
            Err(ProtoError::Truncated { need: 2, have: 0 })
        ));
        assert!(matches!(
            decode_hello(&[0x01]),
            Err(ProtoError::Truncated { need: 2, have: 1 })
        ));
        assert!(matches!(
            decode_hello(&[0x01, 0x00, b'g', b'h']),
            Err(ProtoError::UnterminatedString)
        ));
    }

    #[test]
    fn test_decode_snapshot_truncation() {
        assert!(decode_snapshot(&[]).is_err());
        assert!(decode_snapshot(b"game\0map\0").is_err());
        let mut truncated = vec![b'g', b'\0', b'm', b'\0', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        assert!(decode_snapshot(&truncated).is_err());
        truncated.extend_from_slice(&[0u8; 20]);
        assert!(decode_snapshot(&truncated).is_err());
        truncated.push(b'\0');
        truncated.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 5]);
        assert!(decode_snapshot(&truncated).is_err());
    }

    #[test]
    fn test_decode_player_truncation() {
        assert!(decode_player(&[]).is_err());
        assert!(decode_player(&[1, b'P', b'1']).is_err());
        assert!(decode_player(&[1, b'P', b'1', 0x00, 1]).is_err());
    }

    #[test]
    fn test_decode_action_truncation_and_malformed() {
        assert!(matches!(
            decode_action(&[]),
            Err(ProtoError::Truncated { need: 4, have: 0 })
        ));
        assert!(matches!(
            decode_action(&[0xF7, 0x0C, 0x06]),
            Err(ProtoError::Truncated { need: 4, have: 3 })
        ));
        assert!(matches!(
            decode_action(&[0xF8, 0x0C, 0x06, 0x00, 0x64, 0x00]),
            Err(ProtoError::BadValue("action frame missing 0xF7 header"))
        ));
        assert!(matches!(
            decode_action(&[0xF7, 0x0C, 0x02, 0x00]),
            Err(ProtoError::BadValue(
                "action frame length below header size"
            ))
        ));
        assert!(matches!(
            decode_action(&[0xF7, 0x0C, 0x06, 0x00, 0x64]),
            Err(ProtoError::Truncated { need: 6, have: 5 })
        ));
    }

    #[test]
    fn test_decode_chat_truncation() {
        assert!(decode_chat(&[]).is_err());
        assert!(decode_chat(b"Host\0").is_err());
        assert!(decode_chat(b"Host").is_err());
    }

    #[test]
    fn test_decode_game_over_truncation() {
        assert!(decode_game_over(&[]).is_err());
        assert!(decode_game_over(b"Sentinel").is_err());
    }

    #[test]
    fn test_decode_history_end_truncation() {
        assert!(matches!(
            decode_history_end(&[]),
            Err(ProtoError::Truncated { need: 4, have: 0 })
        ));
        assert!(matches!(
            decode_history_end(&[0xE8, 0x03]),
            Err(ProtoError::Truncated { need: 4, have: 2 })
        ));
    }

    #[test]
    fn test_decode_subscribe_truncation() {
        assert!(matches!(
            decode_subscribe(&[]),
            Err(ProtoError::Truncated { need: 2, have: 0 })
        ));
        assert!(matches!(
            decode_subscribe(&[0x01]),
            Err(ProtoError::Truncated { need: 2, have: 1 })
        ));
    }

    #[test]
    fn test_decode_client_chat_truncation() {
        assert!(decode_client_chat(&[]).is_err());
        assert!(decode_client_chat(b"GG WP").is_err());
    }

    #[test]
    fn test_codec_resyncs_on_garbage() {
        let mut buf =
            BytesMut::from(&[0x00, 0xAA, 0xFD, 0x07, 0x08, 0x00, 0xE8, 0x03, 0x00, 0x00][..]);
        let mut codec = DotaTvCodec::default();
        let frame = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(frame.id, ids::HISTORY_END);
        assert_eq!(decode_history_end(&frame.payload).unwrap(), 1000);
    }

    proptest::proptest! {
        #[test]
        fn decoder_never_panics_on_arbitrary_input(data: Vec<u8>) {
            let mut buf = BytesMut::from(&data[..]);
            let mut codec = DotaTvCodec::default();
            for _ in 0..data.len() + 1 {
                let before = buf.len();
                match codec.decode(&mut buf) {
                    Ok(Some(frame)) => {
                        let _ = decode_hello(&frame.payload);
                        let _ = decode_snapshot(&frame.payload);
                        let _ = decode_player(&frame.payload);
                        let _ = decode_action(&frame.payload);
                        let _ = decode_chat(&frame.payload);
                        let _ = decode_game_over(&frame.payload);
                        let _ = decode_history_end(&frame.payload);
                        let _ = decode_subscribe(&frame.payload);
                        let _ = decode_client_chat(&frame.payload);
                    }
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
