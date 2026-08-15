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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogonProof {
    pub status: u32,
}

impl LogonProof {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let status = b.try_get_u32_le()?;
        Ok(Self { status })
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
        let user = b.try_get_cstring()?;
        let message = b.try_get_cstring().unwrap_or_default();
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
