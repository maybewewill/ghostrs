use bytes::Bytes;

use crate::bytes_ext::BufExt;
use crate::error::ProtoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthInfo {
    pub logon_type: u32,
    pub server_token: u32,
    pub mpq_file_time: u64,
    pub ix86_ver_file_name: String,
    pub value_string_formula: String,
}

impl AuthInfo {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let logon_type = b.try_get_u32_le()?;
        let server_token = b.try_get_u32_le()?;
        let _unknown = b.try_get_bytes(4)?;
        let mpq_low = b.try_get_u32_le()? as u64;
        let mpq_high = b.try_get_u32_le()? as u64;
        let mpq_file_time = (mpq_high << 32) | mpq_low;
        let ix86_ver_file_name = b.try_get_cstring()?;
        let value_string_formula = b.try_get_cstring()?;
        Ok(Self {
            logon_type,
            server_token,
            mpq_file_time,
            ix86_ver_file_name,
            value_string_formula,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCheck {
    pub key_state: u32,
    pub key_state_description: String,
}

impl AuthCheck {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let key_state = b.try_get_u32_le()?;
        let key_state_description = b.try_get_cstring().unwrap_or_default();
        Ok(Self {
            key_state,
            key_state_description,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountLogon {
    pub status: u32,
    pub salt: [u8; 32],
    pub server_public_key: [u8; 32],
}

impl AccountLogon {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let status = b.try_get_u32_le()?;
        let mut salt = [0u8; 32];
        let mut server_public_key = [0u8; 32];
        if status == 0 && b.len() >= 64 {
            let s = b.try_get_bytes(32)?;
            let k = b.try_get_bytes(32)?;
            salt.copy_from_slice(&s);
            server_public_key.copy_from_slice(&k);
        }
        Ok(Self {
            status,
            salt,
            server_public_key,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogonProof {
    pub status: u32,
    pub server_password_proof: [u8; 20],
    pub message: String,
}

impl LogonProof {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let status = b.try_get_u32_le()?;
        let mut server_password_proof = [0u8; 20];
        if b.len() >= 20 {
            let p = b.try_get_bytes(20)?;
            server_password_proof.copy_from_slice(&p);
        }
        let message = if !b.is_empty() {
            b.try_get_cstring().unwrap_or_else(|_| {
                let rest = b.try_get_bytes(b.len()).unwrap_or_default();
                String::from_utf8_lossy(&rest)
                    .trim_end_matches('\0')
                    .to_string()
            })
        } else {
            String::new()
        };
        Ok(Self {
            status,
            server_password_proof,
            message,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatEvent {
    pub event_id: u32,
    pub ping: u32,
    pub user: String,
    pub message: String,
}

impl ChatEvent {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let event_id = b.try_get_u32_le()?;
        let _flags = b.try_get_u32_le()?;
        let ping = b.try_get_u32_le()?;
        let _ip = b.try_get_bytes(4)?;
        let _account_num = b.try_get_bytes(4)?;
        let _reg_auth = b.try_get_bytes(4)?;
        let user = b.try_get_cstring().unwrap_or_default();
        let message = if !b.is_empty() {
            b.try_get_cstring().unwrap_or_else(|_| {
                let rest = b.try_get_bytes(b.len()).unwrap_or_default();
                String::from_utf8_lossy(&rest)
                    .trim_end_matches('\0')
                    .to_string()
            })
        } else {
            String::new()
        };
        Ok(Self {
            event_id,
            ping,
            user,
            message,
        })
    }
}

pub fn decode_ping(payload: &Bytes) -> Result<[u8; 4], ProtoError> {
    let mut b = payload.clone();
    let bytes = b.try_get_bytes(4)?;
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}

/// One game entry from a `SID_GETADVLISTEX` reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvListEntry {
    /// Host address the server hands to a joining client, network order.
    pub ip: [u8; 4],
    /// Host port the server hands to a joining client. Big-endian on the wire.
    pub port: u16,
    pub game_name: String,
    /// Decoded from the 8 reversed ASCII hex chars, `None` if unparseable.
    pub host_counter: Option<u32>,
}

/// Decodes a `SID_GETADVLISTEX` reply payload (BNCS header already stripped).
///
/// Wire format transcribed from `bnetprotocol.cpp:52-93` (`RECEIVE_SID_GETADVLISTEX`).
///
/// Returns `Ok(None)` when the server reports zero games found — that is a valid
/// reply meaning "no such game", not a protocol error.
pub fn decode_getadvlistex(payload: &[u8]) -> Result<Option<AdvListEntry>, ProtoError> {
    let games_found_bytes = payload.get(0..4).ok_or(ProtoError::Truncated {
        need: 4,
        have: payload.len(),
    })?;
    let games_found =
        u32::from_le_bytes(
            games_found_bytes
                .try_into()
                .map_err(|_| ProtoError::Truncated {
                    need: 4,
                    have: payload.len(),
                })?,
        );
    if games_found == 0 {
        return Ok(None);
    }

    if payload.len() < 20 {
        return Err(ProtoError::Truncated {
            need: 20,
            have: payload.len(),
        });
    }

    let port_bytes = payload.get(14..16).ok_or(ProtoError::Truncated {
        need: 16,
        have: payload.len(),
    })?;
    let port = u16::from_be_bytes(port_bytes.try_into().map_err(|_| ProtoError::Truncated {
        need: 16,
        have: payload.len(),
    })?);

    let ip_bytes = payload.get(16..20).ok_or(ProtoError::Truncated {
        need: 20,
        have: payload.len(),
    })?;
    let ip: [u8; 4] = ip_bytes.try_into().map_err(|_| ProtoError::Truncated {
        need: 20,
        have: payload.len(),
    })?;

    let name_slice = payload.get(20..).ok_or(ProtoError::Truncated {
        need: 21,
        have: payload.len(),
    })?;
    let nul_pos = name_slice
        .iter()
        .position(|&b| b == 0)
        .ok_or(ProtoError::UnterminatedString)?;
    let name_bytes = name_slice
        .get(..nul_pos)
        .ok_or(ProtoError::UnterminatedString)?;
    let game_name = String::from_utf8_lossy(name_bytes).into_owned();

    let hc_start = nul_pos + 23;
    let hc_end = hc_start + 8;
    let hc_bytes = payload.get(hc_start..hc_end).ok_or(ProtoError::Truncated {
        need: hc_end,
        have: payload.len(),
    })?;

    let host_counter = std::str::from_utf8(hc_bytes).ok().and_then(|s| {
        let rev: String = s.chars().rev().collect();
        u32::from_str_radix(&rev, 16).ok()
    });

    Ok(Some(AdvListEntry {
        ip,
        port,
        game_name,
        host_counter,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendListEntry {
    pub account: String,
    pub status: u8,
    pub area: u8,
    pub location: String,
}

pub fn decode_friendslist(payload: &[u8]) -> Result<Vec<FriendListEntry>, ProtoError> {
    if payload.is_empty() {
        return Err(ProtoError::Truncated { need: 1, have: 0 });
    }
    let total = payload[0] as usize;
    let mut friends = Vec::with_capacity(total);
    let mut offset = 1;
    for _ in 0..total {
        if offset >= payload.len() {
            break;
        }
        let nul_pos = payload[offset..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(ProtoError::UnterminatedString)?;
        let account = String::from_utf8_lossy(&payload[offset..offset + nul_pos]).into_owned();
        offset += nul_pos + 1;
        if offset + 6 > payload.len() {
            break;
        }
        let status = payload[offset];
        let area = payload[offset + 1];
        offset += 6;
        if offset > payload.len() {
            break;
        }
        let loc_nul = payload[offset..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(ProtoError::UnterminatedString)?;
        let location = String::from_utf8_lossy(&payload[offset..offset + loc_nul]).into_owned();
        offset += loc_nul + 1;
        friends.push(FriendListEntry {
            account,
            status,
            area,
            location,
        });
    }
    Ok(friends)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClanMemberEntry {
    pub name: String,
    pub rank: u8,
    pub status: u8,
    pub location: String,
}

pub fn decode_clanmemberlist(payload: &[u8]) -> Result<Vec<ClanMemberEntry>, ProtoError> {
    if payload.len() < 5 {
        return Err(ProtoError::Truncated {
            need: 5,
            have: payload.len(),
        });
    }
    let total = payload[4] as usize;
    let mut members = Vec::with_capacity(total);
    let mut offset = 5;
    for _ in 0..total {
        if offset >= payload.len() {
            break;
        }
        let nul_pos = payload[offset..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(ProtoError::UnterminatedString)?;
        let name = String::from_utf8_lossy(&payload[offset..offset + nul_pos]).into_owned();
        offset += nul_pos + 1;
        if offset + 2 > payload.len() {
            break;
        }
        let rank = payload[offset];
        let status = payload[offset + 1];
        offset += 2;
        let loc_nul = payload[offset..]
            .iter()
            .position(|&b| b == 0)
            .ok_or(ProtoError::UnterminatedString)?;
        let location = String::from_utf8_lossy(&payload[offset..offset + loc_nul]).into_owned();
        offset += loc_nul + 1;
        members.push(ClanMemberEntry {
            name,
            rank,
            status,
            location,
        });
    }
    Ok(members)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClanInvite {
    pub tag: [u8; 4],
    pub clan_name: String,
    pub inviter_name: String,
}

pub fn decode_clancreationinvitation(payload: &[u8]) -> Result<ClanInvite, ProtoError> {
    if payload.len() < 8 {
        return Err(ProtoError::Truncated {
            need: 8,
            have: payload.len(),
        });
    }
    let mut tag = [0u8; 4];
    tag.copy_from_slice(&payload[4..8]);
    let mut offset = 8;
    let nul1 = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(ProtoError::UnterminatedString)?;
    let clan_name = String::from_utf8_lossy(&payload[offset..offset + nul1]).into_owned();
    offset += nul1 + 1;
    let nul2 = payload[offset..]
        .iter()
        .position(|&b| b == 0)
        .ok_or(ProtoError::UnterminatedString)?;
    let inviter_name = String::from_utf8_lossy(&payload[offset..offset + nul2]).into_owned();
    Ok(ClanInvite {
        tag,
        clan_name,
        inviter_name,
    })
}

pub fn decode_claninvitationresponse(payload: &[u8]) -> Result<ClanInvite, ProtoError> {
    decode_clancreationinvitation(payload)
}

pub fn decode_warden(payload: &[u8]) -> Result<Bytes, ProtoError> {
    Ok(Bytes::copy_from_slice(payload))
}

pub fn decode_checkad(payload: &[u8]) -> Result<(), ProtoError> {
    let _ = payload;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn getadvlistex_decodes_address_port_and_host_counter() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[1, 0, 0, 0]); // games_found = 1
        payload.extend_from_slice(&[0u8; 10]); // 10 unknown/reserved bytes
        payload.extend_from_slice(&[0x17, 0xE1]); // port 6113 BE
        payload.extend_from_slice(&[93, 184, 216, 34]); // IP
        payload.extend_from_slice(b"ghostrs probe4\0"); // GameName + NUL
        payload.extend_from_slice(&[0, 0]); // 2 unknown/reserved bytes
        payload.extend_from_slice(b"1fedcba0"); // HostCounter (0x0ABCDEF1 in reversed hex)

        let entry = decode_getadvlistex(&payload)
            .expect("decoding valid SID_GETADVLISTEX must succeed")
            .expect("must return Some(AdvListEntry)");

        assert_eq!(entry.port, 6113);
        assert_eq!(entry.ip, [93, 184, 216, 34]);
        assert_eq!(entry.game_name, "ghostrs probe4");
        assert_eq!(entry.host_counter, Some(0x0ABCDEF1));
    }

    #[test]
    fn getadvlistex_zero_games_found_is_not_an_error() {
        let payload = [0u8, 0, 0, 0];
        let result = decode_getadvlistex(&payload);
        assert_eq!(result, Ok(None));
    }

    #[test]
    fn getadvlistex_rejects_truncated_entry() {
        let payload = [1u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]; // 12 bytes
        let result = decode_getadvlistex(&payload);
        assert!(result.is_err());
    }

    #[test]
    fn friendslist_decodes_multiple_friends() {
        let mut payload = Vec::new();
        payload.push(2); // total = 2
        // Friend 1
        payload.extend_from_slice(b"Alice\0");
        payload.push(1); // status (Mutual)
        payload.push(3); // area (Public Game)
        payload.extend_from_slice(&[0, 0, 0, 0]); // 4 bytes unknown
        payload.extend_from_slice(b"PX3WDOTA\0");
        // Friend 2
        payload.extend_from_slice(b"Bob\0");
        payload.push(0); // status
        payload.push(0); // area (Offline)
        payload.extend_from_slice(&[0, 0, 0, 0]); // 4 bytes unknown
        payload.extend_from_slice(b".\0");

        let friends = decode_friendslist(&payload).expect("friendslist decoding succeeds");
        assert_eq!(friends.len(), 2);
        assert_eq!(friends[0].account, "Alice");
        assert_eq!(friends[0].status, 1);
        assert_eq!(friends[0].area, 3);
        assert_eq!(friends[0].location, "PX3WDOTA");

        assert_eq!(friends[1].account, "Bob");
        assert_eq!(friends[1].status, 0);
        assert_eq!(friends[1].area, 0);
        assert_eq!(friends[1].location, ".");
    }

    #[test]
    fn clanmemberlist_decodes_members() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0, 0, 0, 0]); // 4 unknown bytes
        payload.push(2); // total = 2
        // Member 1
        payload.extend_from_slice(b"ChieftainUser\0");
        payload.push(4); // rank (Leader)
        payload.push(1); // status (Online)
        payload.extend_from_slice(b"PX3WChannel\0");
        // Member 2
        payload.extend_from_slice(b"PeonUser\0");
        payload.push(1); // rank (Peon)
        payload.push(0); // status (Offline)
        payload.extend_from_slice(b"\0");

        let members = decode_clanmemberlist(&payload).expect("clanmemberlist decoding succeeds");
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "ChieftainUser");
        assert_eq!(members[0].rank, 4);
        assert_eq!(members[0].status, 1);
        assert_eq!(members[0].location, "PX3WChannel");

        assert_eq!(members[1].name, "PeonUser");
        assert_eq!(members[1].rank, 1);
        assert_eq!(members[1].status, 0);
    }

    #[test]
    fn clan_creation_and_invitation_decode() {
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0, 0, 0, 0]); // cookie
        payload.extend_from_slice(&[b'T', b'E', b'S', b'T']); // tag
        payload.extend_from_slice(b"MyClan\0");
        payload.extend_from_slice(b"InviterGuy\0");

        let invite = decode_clancreationinvitation(&payload).expect("clan invite decode succeeds");
        assert_eq!(&invite.tag, b"TEST");
        assert_eq!(invite.clan_name, "MyClan");
        assert_eq!(invite.inviter_name, "InviterGuy");

        let resp_invite = decode_claninvitationresponse(&payload).expect("response decode succeeds");
        assert_eq!(&resp_invite.tag, b"TEST");
        assert_eq!(resp_invite.clan_name, "MyClan");
        assert_eq!(resp_invite.inviter_name, "InviterGuy");
    }

    #[test]
    fn warden_and_checkad_decode() {
        let warden_data = b"warden challenge payload";
        let res = decode_warden(warden_data).expect("warden decode succeeds");
        assert_eq!(&res[..], warden_data);

        assert!(decode_checkad(&[]).is_ok());
    }
}
