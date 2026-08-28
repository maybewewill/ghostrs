use bytes::Bytes;

use crate::bytes_ext::BufExt;
use crate::error::ProtoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReqJoin {
    pub host_counter: u32,
    pub entry_key: u32,
    pub listen_port: u16,
    pub peer_key: u32,
    pub name: String,
    pub internal_ip: [u8; 4],
}

impl ReqJoin {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let host_counter = b.try_get_u32_le()?;
        let entry_key = b.try_get_u32_le()?;
        let _unknown = b.try_get_u8()?;
        let listen_port = b.try_get_u16_le()?;
        let peer_key = b.try_get_u32_le()?;
        let name = b.try_get_cstring()?;
        if name.is_empty() {
            return Err(ProtoError::BadValue("empty player name"));
        }
        let _unknown2 = b.try_get_bytes(6)?;
        let ip = b.try_get_bytes(4)?;
        Ok(Self {
            host_counter,
            entry_key,
            listen_port,
            peer_key,
            name,
            internal_ip: [ip[0], ip[1], ip[2], ip[3]],
        })
    }
}

/// A player action. `data` aliases the read buffer: relaying it costs a
/// refcount bump, and the engine never parses the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingAction {
    pub crc: u32,
    pub data: Bytes,
}

impl OutgoingAction {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        if payload.len() < 4 {
            return Err(ProtoError::Truncated {
                need: 4,
                have: payload.len(),
            });
        }
        let crc = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        Ok(Self {
            crc,
            data: payload.slice(4..),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatToHost {
    pub to_pids: Vec<u8>,
    pub from_pid: u8,
    pub flag: u8,
    /// Extra flags for flag 0x20 (chat scope: all/allies/observers/private).
    pub extra: Bytes,
    pub message: String,
    /// Set for flags 0x11..=0x14 (team/colour/race/handicap change requests).
    pub byte: u8,
}

impl ChatToHost {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let count = b.try_get_u8()? as usize;
        let to_pids = b.try_get_bytes(count)?.to_vec();
        let from_pid = b.try_get_u8()?;
        let flag = b.try_get_u8()?;

        let mut extra = Bytes::new();
        let mut message = String::new();
        let mut byte = 0u8;

        match flag {
            0x10 => message = b.try_get_cstring()?,
            0x11..=0x14 => byte = b.try_get_u8()?,
            0x20 => {
                extra = b.try_get_bytes(4)?;
                message = b.try_get_cstring()?;
            }
            _ => return Err(ProtoError::BadValue("unknown chat-to-host flag")),
        }

        Ok(Self {
            to_pids,
            from_pid,
            flag,
            extra,
            message,
            byte,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSizeReport {
    pub size_flag: u8,
    pub map_size: u32,
}

impl MapSizeReport {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let _unknown = b.try_get_bytes(4)?;
        Ok(Self {
            size_flag: b.try_get_u8()?,
            map_size: b.try_get_u32_le()?,
        })
    }
}

pub fn decode_leave_game(payload: &Bytes) -> Result<u32, ProtoError> {
    payload.clone().try_get_u32_le()
}

pub fn decode_keepalive(payload: &Bytes) -> Result<u32, ProtoError> {
    let mut b = payload.clone();
    let _unknown = b.try_get_u8()?;
    b.try_get_u32_le()
}

pub fn decode_pong_to_host(payload: &Bytes) -> Result<u32, ProtoError> {
    payload.clone().try_get_u32_le()
}

pub fn decode_map_part_ok(payload: &Bytes) -> Result<u32, ProtoError> {
    let mut b = payload.clone();
    let _to_pid = b.try_get_u8()?;
    let _from_pid = b.try_get_u8()?;
    let _unknown = b.try_get_bytes(4)?;
    b.try_get_u32_le()
}

pub fn decode_map_part_not_ok(payload: &Bytes) -> Result<u32, ProtoError> {
    if payload.len() < 6 {
        return Err(ProtoError::Truncated {
            need: 6,
            have: payload.len(),
        });
    }
    let offset_bytes = if payload.len() >= 10 {
        &payload[6..10]
    } else {
        &payload[2..6]
    };
    Ok(u32::from_le_bytes(offset_bytes.try_into().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{BufMut, BytesMut};

    fn reqjoin_payload(name: &str) -> Bytes {
        let mut b = BytesMut::new();
        b.put_u32_le(7); // host counter
        b.put_u32_le(0xDEAD_BEEF); // entry key
        b.put_u8(0); // unknown
        b.put_u16_le(6112); // listen port
        b.put_u32_le(0x1234_5678); // peer key
        b.put_slice(name.as_bytes());
        b.put_u8(0);
        b.put_slice(&[0, 0, 0, 0, 0, 0]); // 6 bytes unknown/sockaddr prefix (matching legacy offset name.len() + 26)
        b.put_slice(&[192, 168, 1, 50]);
        b.freeze()
    }

    #[test]
    fn decodes_req_join() {
        let p = ReqJoin::decode(&reqjoin_payload("Slash")).unwrap();
        assert_eq!(p.host_counter, 7);
        assert_eq!(p.entry_key, 0xDEAD_BEEF);
        assert_eq!(p.listen_port, 6112);
        assert_eq!(p.peer_key, 0x1234_5678);
        assert_eq!(p.name, "Slash");
        assert_eq!(p.internal_ip, [192, 168, 1, 50]);
    }

    #[test]
    fn req_join_truncated_errors_without_panicking() {
        let full = reqjoin_payload("Slash");
        for cut in 0..full.len() {
            let short = full.slice(0..cut);
            assert!(ReqJoin::decode(&short).is_err(), "cut at {cut} must error");
        }
    }

    #[test]
    fn outgoing_action_keeps_body_zero_copy() {
        let mut b = BytesMut::new();
        b.put_u32_le(0xAABB_CCDD);
        b.put_slice(&[0x10, 0x20, 0x30]);
        let payload = b.freeze();

        let a = OutgoingAction::decode(&payload).unwrap();
        assert_eq!(a.crc, 0xAABB_CCDD);
        assert_eq!(&a.data[..], &[0x10, 0x20, 0x30]);
        // The action body must be a slice of the original buffer, not a copy.
        assert_eq!(a.data.as_ptr(), payload[4..].as_ptr());
    }

    #[test]
    fn decodes_chat_message_flag_0x10() {
        let mut b = BytesMut::new();
        b.put_u8(2);
        b.put_slice(&[3, 4]); // to pids
        b.put_u8(1); // from pid
        b.put_u8(0x10); // flag: plain message
        b.put_slice(b"gl hf");
        b.put_u8(0);
        let c = ChatToHost::decode(&b.freeze()).unwrap();
        assert_eq!(c.to_pids, vec![3, 4]);
        assert_eq!(c.from_pid, 1);
        assert_eq!(c.message, "gl hf");
    }

    #[test]
    fn decodes_chat_extra_flag_0x20() {
        let mut b = BytesMut::new();
        b.put_u8(1);
        b.put_slice(&[2]);
        b.put_u8(1);
        b.put_u8(0x20);
        b.put_u32_le(0); // extra flags (chat scope)
        b.put_slice(b"ally");
        b.put_u8(0);
        let c = ChatToHost::decode(&b.freeze()).unwrap();
        assert_eq!(c.message, "ally");
        assert_eq!(c.extra.len(), 4);
    }

    #[test]
    fn decodes_keepalive_checksum() {
        let mut b = BytesMut::new();
        b.put_u8(0);
        b.put_u32_le(0x0BAD_F00D);
        assert_eq!(decode_keepalive(&b.freeze()).unwrap(), 0x0BAD_F00D);
    }

    #[test]
    fn decodes_map_size_report() {
        let mut b = BytesMut::new();
        b.put_slice(&[0, 0, 0, 0]);
        b.put_u8(1);
        b.put_u32_le(1_234_567);
        let m = MapSizeReport::decode(&b.freeze()).unwrap();
        assert_eq!(m.size_flag, 1);
        assert_eq!(m.map_size, 1_234_567);
    }

    #[test]
    fn test_decode_map_part_not_ok() {
        // GHost++ golden fixture from gameprotocol.h:99 (f7 45 0a 00 01 02 01 00 00 00)
        let payload_6b = Bytes::from_static(&[0x01, 0x02, 0x01, 0x00, 0x00, 0x00]);
        assert_eq!(decode_map_part_not_ok(&payload_6b).unwrap(), 1);

        // 10-byte payload variant (with 4-byte unknown field before offset)
        let payload_10b =
            Bytes::from_static(&[0x01, 0x02, 0x00, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00, 0x00]);
        assert_eq!(decode_map_part_not_ok(&payload_10b).unwrap(), 0x1234);

        // Short payload
        let short = Bytes::from_static(&[0x01, 0x02]);
        assert!(decode_map_part_not_ok(&short).is_err());
    }
}
