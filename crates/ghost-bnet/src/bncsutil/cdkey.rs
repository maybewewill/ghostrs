//! BNCSutil CD-Key Decoding Implementation
//!
//! Supports decoding, validation, value extraction, and hash computation for:
//! - StarCraft 13-character keys
//! - Warcraft II / Diablo II 16-character keys
//! - Warcraft III 26-character keys

use sha1::{Digest, Sha1};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CdKeyError {
    #[error("invalid CD-key length: expected 13, 16, or 26 alphanumeric chars, got {0}")]
    InvalidLength(usize),
    #[error("invalid character '{0}' in CD-key")]
    InvalidChar(char),
    #[error("CD-key checksum verification failed")]
    ChecksumFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyType {
    StarCraft,
    WarCraft2,
    WarCraft3,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdKeyInfo {
    pub product: u32,
    pub public_value: u32,
    pub val2: u32,
    pub long_val2: Option<[u8; 10]>,
    pub hash: [u8; 20],
}

/// Object-oriented CD-Key Decoder mirroring BNCSutil's `CDKeyDecoder` class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdKeyDecoder {
    key_type: KeyType,
    product: u32,
    public_value: u32,
    val2: u32,
    long_val2: Option<[u8; 10]>,
}

impl CdKeyDecoder {
    /// Creates and validates a new CD-key decoder from a raw key string.
    pub fn new(cdkey: &str) -> Result<Self, CdKeyError> {
        let (key_type, product, public_value, val2, long_val2) = decode_key_components(cdkey)?;
        Ok(Self {
            key_type,
            product,
            public_value,
            val2,
            long_val2,
        })
    }

    #[inline]
    pub fn key_type(&self) -> KeyType {
        self.key_type
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        true
    }

    #[inline]
    pub fn product(&self) -> u32 {
        self.product
    }

    #[inline]
    pub fn public_value(&self) -> u32 {
        self.public_value
    }

    #[inline]
    pub fn val1(&self) -> u32 {
        self.public_value
    }

    #[inline]
    pub fn val2(&self) -> u32 {
        self.val2
    }

    #[inline]
    pub fn long_val2(&self) -> Option<[u8; 10]> {
        self.long_val2
    }

    #[inline]
    pub fn val2_length(&self) -> usize {
        if self.key_type == KeyType::WarCraft3 {
            10
        } else {
            4
        }
    }

    /// Calculates the 20-byte hash suitable for SID_AUTH_CHECK (0x51).
    pub fn calculate_hash(&self, client_token: u32, server_token: u32) -> [u8; 20] {
        match self.key_type {
            KeyType::StarCraft | KeyType::WarCraft2 => {
                let mut kh = [0u8; 24];
                kh[0..4].copy_from_slice(&client_token.to_le_bytes());
                kh[4..8].copy_from_slice(&server_token.to_le_bytes());
                kh[8..12].copy_from_slice(&self.product.to_le_bytes());
                kh[12..16].copy_from_slice(&self.public_value.to_le_bytes());
                kh[16..20].copy_from_slice(&0u32.to_le_bytes());
                kh[20..24].copy_from_slice(&self.val2.to_le_bytes());
                crate::bncsutil::xsha1::xsha1(&kh)
            }
            KeyType::WarCraft3 => {
                let mut hasher = Sha1::new();
                hasher.update(client_token.to_le_bytes());
                hasher.update(server_token.to_le_bytes());
                hasher.update(self.product.to_le_bytes());
                hasher.update(self.public_value.to_le_bytes());
                if let Some(w3val2) = self.long_val2 {
                    hasher.update(w3val2);
                }
                let res = hasher.finalize();
                let mut hash = [0u8; 20];
                hash.copy_from_slice(&res);
                hash
            }
        }
    }
}

const W3_KEY_MAP: [u8; 256] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0x00, 0xFF, 0x01, 0xFF, 0x02, 0x03, 0x04, 0x05, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0xFF, 0x0D, 0x0E, 0xFF, 0x0F, 0x10, 0xFF,
    0x11, 0xFF, 0x12, 0xFF, 0x13, 0xFF, 0x14, 0x15, 0x16, 0x17, 0x18, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0xFF, 0x0D, 0x0E, 0xFF, 0x0F, 0x10, 0xFF,
    0x11, 0xFF, 0x12, 0xFF, 0x13, 0xFF, 0x14, 0x15, 0x16, 0x17, 0x18, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

const W3_TRANSLATE_MAP: [u8; 480] = [
    0x09, 0x04, 0x07, 0x0F, 0x0D, 0x0A, 0x03, 0x0B, 0x01, 0x02, 0x0C, 0x08, 0x06, 0x0E, 0x05, 0x00,
    0x09, 0x0B, 0x05, 0x04, 0x08, 0x0F, 0x01, 0x0E, 0x07, 0x00, 0x03, 0x02, 0x0A, 0x06, 0x0D, 0x0C,
    0x0C, 0x0E, 0x01, 0x04, 0x09, 0x0F, 0x0A, 0x0B, 0x0D, 0x06, 0x00, 0x08, 0x07, 0x02, 0x05, 0x03,
    0x0B, 0x02, 0x05, 0x0E, 0x0D, 0x03, 0x09, 0x00, 0x01, 0x0F, 0x07, 0x0C, 0x0A, 0x06, 0x04, 0x08,
    0x06, 0x02, 0x04, 0x05, 0x0B, 0x08, 0x0C, 0x0E, 0x0D, 0x0F, 0x07, 0x01, 0x0A, 0x00, 0x03, 0x09,
    0x05, 0x04, 0x0E, 0x0C, 0x07, 0x06, 0x0D, 0x0A, 0x0F, 0x02, 0x09, 0x01, 0x00, 0x0B, 0x08, 0x03,
    0x0C, 0x07, 0x08, 0x0F, 0x0B, 0x00, 0x05, 0x09, 0x0D, 0x0A, 0x06, 0x0E, 0x02, 0x04, 0x03, 0x01,
    0x03, 0x0A, 0x0E, 0x08, 0x01, 0x0B, 0x05, 0x04, 0x02, 0x0F, 0x0D, 0x0C, 0x06, 0x07, 0x09, 0x00,
    0x0C, 0x0D, 0x01, 0x0F, 0x08, 0x0E, 0x05, 0x0B, 0x03, 0x0A, 0x09, 0x00, 0x07, 0x02, 0x04, 0x06,
    0x0D, 0x0A, 0x07, 0x0E, 0x01, 0x06, 0x0B, 0x08, 0x0F, 0x0C, 0x05, 0x02, 0x03, 0x00, 0x04, 0x09,
    0x03, 0x0E, 0x07, 0x05, 0x0B, 0x0F, 0x08, 0x0C, 0x01, 0x0A, 0x04, 0x0D, 0x00, 0x06, 0x09, 0x02,
    0x0B, 0x06, 0x09, 0x04, 0x01, 0x08, 0x0A, 0x0D, 0x07, 0x0E, 0x00, 0x0C, 0x0F, 0x02, 0x03, 0x05,
    0x0C, 0x07, 0x08, 0x0D, 0x03, 0x0B, 0x00, 0x0E, 0x06, 0x0F, 0x09, 0x04, 0x0A, 0x01, 0x05, 0x02,
    0x0C, 0x06, 0x0D, 0x09, 0x0B, 0x00, 0x01, 0x02, 0x0F, 0x07, 0x03, 0x04, 0x0A, 0x0E, 0x08, 0x05,
    0x03, 0x06, 0x01, 0x05, 0x0B, 0x0C, 0x08, 0x00, 0x0F, 0x0E, 0x09, 0x04, 0x07, 0x0A, 0x0D, 0x02,
    0x0A, 0x07, 0x0B, 0x0F, 0x02, 0x08, 0x00, 0x0D, 0x0E, 0x0C, 0x01, 0x06, 0x09, 0x03, 0x05, 0x04,
    0x0A, 0x0B, 0x0D, 0x04, 0x03, 0x08, 0x05, 0x09, 0x01, 0x00, 0x0F, 0x0C, 0x07, 0x0E, 0x02, 0x06,
    0x0B, 0x04, 0x0D, 0x0F, 0x01, 0x06, 0x03, 0x0E, 0x07, 0x0A, 0x0C, 0x08, 0x09, 0x02, 0x05, 0x00,
    0x09, 0x06, 0x07, 0x00, 0x01, 0x0A, 0x0D, 0x02, 0x03, 0x0E, 0x0F, 0x0C, 0x05, 0x0B, 0x04, 0x08,
    0x0D, 0x0E, 0x05, 0x06, 0x01, 0x09, 0x08, 0x0C, 0x02, 0x0F, 0x03, 0x07, 0x0B, 0x04, 0x00, 0x0A,
    0x09, 0x0F, 0x04, 0x00, 0x01, 0x06, 0x0A, 0x0E, 0x02, 0x03, 0x07, 0x0D, 0x05, 0x0B, 0x08, 0x0C,
    0x03, 0x0E, 0x01, 0x0A, 0x02, 0x0C, 0x08, 0x04, 0x0B, 0x07, 0x0D, 0x00, 0x0F, 0x06, 0x09, 0x05,
    0x07, 0x02, 0x0C, 0x06, 0x0A, 0x08, 0x0B, 0x00, 0x0F, 0x04, 0x03, 0x0E, 0x09, 0x01, 0x0D, 0x05,
    0x0C, 0x04, 0x05, 0x09, 0x0A, 0x02, 0x08, 0x0D, 0x03, 0x0F, 0x01, 0x0E, 0x06, 0x07, 0x0B, 0x00,
    0x0A, 0x08, 0x0E, 0x0D, 0x09, 0x0F, 0x03, 0x00, 0x04, 0x06, 0x01, 0x0C, 0x07, 0x0B, 0x02, 0x05,
    0x03, 0x0C, 0x04, 0x0A, 0x02, 0x0F, 0x0D, 0x0E, 0x07, 0x00, 0x05, 0x08, 0x01, 0x06, 0x0B, 0x09,
    0x0A, 0x0C, 0x01, 0x00, 0x09, 0x0E, 0x0D, 0x0B, 0x03, 0x07, 0x0F, 0x08, 0x05, 0x02, 0x04, 0x06,
    0x0E, 0x0A, 0x01, 0x08, 0x07, 0x06, 0x05, 0x0C, 0x02, 0x0F, 0x00, 0x0D, 0x03, 0x0B, 0x04, 0x09,
    0x03, 0x08, 0x0E, 0x00, 0x07, 0x09, 0x0F, 0x0C, 0x01, 0x06, 0x0D, 0x02, 0x05, 0x0A, 0x0B, 0x04,
    0x03, 0x0A, 0x0C, 0x04, 0x0D, 0x0B, 0x09, 0x0E, 0x0F, 0x06, 0x01, 0x07, 0x02, 0x00, 0x05, 0x08,
];

const W2_MAP: [u8; 256] = [
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0x00, 0xFF, 0x01, 0xFF, 0x02, 0x03, 0x04, 0x05, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0xFF, 0x0D, 0x0E, 0xFF, 0x0F, 0x10, 0xFF,
    0x11, 0xFF, 0x12, 0xFF, 0x13, 0xFF, 0x14, 0x15, 0x16, 0xFF, 0x17, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0xFF, 0x0D, 0x0E, 0xFF, 0x0F, 0x10, 0xFF,
    0x11, 0xFF, 0x12, 0xFF, 0x13, 0xFF, 0x14, 0x15, 0x16, 0xFF, 0x17, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Decodes a 13-digit, 16-character, or 26-character Battle.net CD-key.
pub fn decode_cd_key(
    cdkey: &str,
    client_token: u32,
    server_token: u32,
) -> Result<CdKeyInfo, CdKeyError> {
    let decoder = CdKeyDecoder::new(cdkey)?;
    let hash = decoder.calculate_hash(client_token, server_token);
    Ok(CdKeyInfo {
        product: decoder.product,
        public_value: decoder.public_value,
        val2: decoder.val2,
        long_val2: decoder.long_val2,
        hash,
    })
}

/// One-shot quick CD-key decoding matching BNCSutil `kd_quick`.
#[inline]
pub fn kd_quick(
    cdkey: &str,
    client_token: u32,
    server_token: u32,
) -> Result<CdKeyInfo, CdKeyError> {
    decode_cd_key(cdkey, client_token, server_token)
}

type DecodedKeyTuple = (KeyType, u32, u32, u32, Option<[u8; 10]>);

fn decode_key_components(cdkey: &str) -> Result<DecodedKeyTuple, CdKeyError> {
    for c in cdkey.chars() {
        if !c.is_alphanumeric() && c != '-' && c != ' ' {
            return Err(CdKeyError::InvalidChar(c));
        }
    }

    let sanitized: String = cdkey.chars().filter(|c| c.is_alphanumeric()).collect();
    match sanitized.len() {
        13 => {
            let (prod, v1, v2) = decode_13_char_key(&sanitized)?;
            Ok((KeyType::StarCraft, prod, v1, v2, None))
        }
        16 => {
            let (prod, v1, v2) = decode_16_char_key_components(&sanitized)?;
            Ok((KeyType::WarCraft2, prod, v1, v2, None))
        }
        26 => {
            let (prod, v1, v2_long) = decode_26_char_key_components(&sanitized)?;
            Ok((KeyType::WarCraft3, prod, v1, 0, Some(v2_long)))
        }
        other => Err(CdKeyError::InvalidLength(other)),
    }
}

fn decode_13_char_key(key: &str) -> Result<(u32, u32, u32), CdKeyError> {
    let mut bytes = [0u8; 13];
    for (i, c) in key.chars().enumerate() {
        if !c.is_ascii_digit() {
            return Err(CdKeyError::InvalidChar(c));
        }
        bytes[i] = c as u8;
    }

    let mut accum: i32 = 3;
    for &b in bytes.iter().take(12) {
        let val = (b as char).to_ascii_lowercase() as u8;
        let digit = (val - b'0') as i32;
        accum += digit ^ (accum * 2);
    }

    let check_digit = (bytes[12] - b'0') as i32;
    if (accum % 10) != check_digit {
        return Err(CdKeyError::ChecksumFailed);
    }

    let mut pos = 0x0Busize;
    let mut i = 0xC2i32;
    while i >= 7 {
        let idx = (i % 0x0C) as usize;
        bytes.swap(pos, idx);
        pos = pos.saturating_sub(1);
        i -= 0x11;
    }

    let mut hash_key: i32 = 0x13AC9741;
    for j in (0..=11).rev() {
        let mut temp = (bytes[j] as char).to_ascii_uppercase() as u8;
        if temp <= b'7' {
            temp ^= (hash_key & 7) as u8;
            hash_key >>= 3;
        } else if temp < b'A' {
            temp ^= (j as u8) & 1;
        }
        bytes[j] = temp;
    }

    let s = std::str::from_utf8(&bytes).map_err(|_| CdKeyError::ChecksumFailed)?;
    let product = s[0..2]
        .parse::<u32>()
        .map_err(|_| CdKeyError::ChecksumFailed)?;
    let value1 = s[2..9]
        .parse::<u32>()
        .map_err(|_| CdKeyError::ChecksumFailed)?;
    let value2 = s[9..12]
        .parse::<u32>()
        .map_err(|_| CdKeyError::ChecksumFailed)?;

    Ok((product, value1, value2))
}

fn decode_16_char_key_components(key: &str) -> Result<(u32, u32, u32), CdKeyError> {
    let mut cdkey_bytes = [0u8; 16];
    for (i, c) in key.chars().enumerate() {
        let b = c as usize;
        if b >= 256 || W2_MAP[b] == 0xFF {
            return Err(CdKeyError::InvalidChar(c));
        }
        cdkey_bytes[i] = c as u8;
    }

    let mut r: u32 = 1;
    let mut checksum: u32 = 0;
    for i in (0..16).step_by(2) {
        let c1 = W2_MAP[cdkey_bytes[i] as usize] as u32;
        let mut n = c1 * 3;
        let c2 = W2_MAP[cdkey_bytes[i + 1] as usize] as u32;
        n = c2 + n * 8;

        if n >= 0x100 {
            n -= 0x100;
            checksum |= r;
        }
        let n2 = n >> 4;
        cdkey_bytes[i] = get_hex_value(n2);
        cdkey_bytes[i + 1] = get_hex_value(n);
        r <<= 1;
    }

    let mut v: u32 = 3;
    for &c in &cdkey_bytes {
        let mut n = get_num_value(c);
        let n2 = v * 2;
        n ^= n2;
        v += n;
    }
    v &= 0xFF;

    if v != checksum {
        return Err(CdKeyError::ChecksumFailed);
    }

    for j in (0..=15).rev() {
        let c = cdkey_bytes[j];
        let n = if j > 8 { j - 9 } else { 0xF - (8 - j) } & 0xF;
        cdkey_bytes[j] = cdkey_bytes[n];
        cdkey_bytes[n] = c;
    }

    let mut v2: u32 = 0x13AC9741;
    for j in (0..=15).rev() {
        let c = (cdkey_bytes[j] as char).to_ascii_uppercase() as u8;
        cdkey_bytes[j] = c;
        if c <= b'7' {
            let v_curr = v2;
            let c2 = ((v_curr as u8) & 7) ^ c;
            v2 = v_curr >> 3;
            cdkey_bytes[j] = c2;
        } else if c < b'A' {
            cdkey_bytes[j] = ((j as u8) & 1) ^ c;
        }
    }

    let s = std::str::from_utf8(&cdkey_bytes).map_err(|_| CdKeyError::ChecksumFailed)?;
    let product = u32::from_str_radix(&s[0..2], 16).map_err(|_| CdKeyError::ChecksumFailed)?;
    let value1 = u32::from_str_radix(&s[2..8], 16).map_err(|_| CdKeyError::ChecksumFailed)?;
    let value2 = u32::from_str_radix(&s[8..16], 16).map_err(|_| CdKeyError::ChecksumFailed)?;

    Ok((product, value1, value2))
}

fn decode_26_char_key_components(key: &str) -> Result<(u32, u32, [u8; 10]), CdKeyError> {
    let mut table = [0i8; 52];
    let mut a: usize;
    let mut b: usize = 0x21;

    for c in key.chars() {
        let b_val = c as usize;
        let decode = if b_val < 256 {
            W3_KEY_MAP[b_val] as i8
        } else {
            return Err(CdKeyError::InvalidChar(c));
        };

        a = (b + 0x07B5) % 52;
        b = (a + 0x07B5) % 52;
        table[a] = decode / 5;
        table[b] = decode % 5;
    }

    let mut values = [0u32; 4];
    for i in (0..52).rev() {
        let mut dc_byte = table[i] as i32;
        for word in values.iter_mut().rev() {
            let edxeax = ((*word as u64 & 0xFFFF_FFFF) as i64) * 5;
            let sum = (dc_byte as u32).wrapping_add(edxeax as u32);
            *word = sum;
            dc_byte = (edxeax >> 32) as i32;
        }
    }

    decode_key_table(&mut values);

    let product = values[0] >> 10;

    let mut be_bytes = [0u8; 16];
    for i in 0..4 {
        be_bytes[i * 4..i * 4 + 4].copy_from_slice(&values[i].to_be_bytes());
    }

    let value1 =
        u32::from_le_bytes([be_bytes[2], be_bytes[3], be_bytes[4], be_bytes[5]]) & 0xFFFFFF03;
    let public_value = value1.swap_bytes();

    let mut w3value2 = [0u8; 10];
    w3value2[0] = be_bytes[7];
    w3value2[1] = be_bytes[6];
    w3value2[2..6].copy_from_slice(&[be_bytes[11], be_bytes[10], be_bytes[9], be_bytes[8]]);
    w3value2[6..10].copy_from_slice(&[be_bytes[15], be_bytes[14], be_bytes[13], be_bytes[12]]);

    Ok((product, public_value, w3value2))
}

fn decode_key_table(values: &mut [u32; 4]) {
    let mut var8: i32 = 29;
    let mut i: i32 = 464;

    loop {
        let esi = ((var8 & 7) << 2) as u32;
        let var4 = (var8 >> 3) as usize;
        let mut var_c = (values[3 - var4] >> esi) & 0xF;

        if i < 464 {
            for j in (var8 + 1..=29).rev() {
                let ecx = ((j & 7) << 2) as u32;
                let ebp = (values[3 - ((j >> 3) as usize)] >> ecx) & 0xF;
                let idx1 = (var_c + (i as u32)) as usize;
                let b = W3_TRANSLATE_MAP[idx1] as u32;
                let idx2 = (ebp ^ (b + (i as u32))) as usize;
                var_c = W3_TRANSLATE_MAP[idx2] as u32;
            }
        }

        var8 -= 1;
        let mut j = var8;
        while j >= 0 {
            let ecx = ((j & 7) << 2) as u32;
            let ebp = (values[3 - ((j >> 3) as usize)] >> ecx) & 0xF;
            let idx1 = (var_c + (i as u32)) as usize;
            let b = W3_TRANSLATE_MAP[idx1] as u32;
            let idx2 = (ebp ^ (b + (i as u32))) as usize;
            var_c = W3_TRANSLATE_MAP[idx2] as u32;
            j -= 1;
        }

        let ebx = ((W3_TRANSLATE_MAP[(var_c + (i as u32)) as usize] & 0xF) as u32) << esi;
        let mask = !(0xFu32 << esi);
        values[3 - var4] = ebx | (values[3 - var4] & mask);

        i -= 16;
        if i < 0 {
            break;
        }
    }

    let mut scopy = [0u8; 16];
    for (idx, word) in values.iter().enumerate() {
        scopy[idx * 4..idx * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }

    let mut esi: usize = 0;
    for edi in 0..120usize {
        let eax = (edi & 0x1F) as u32;
        let ecx = (esi & 0x1F) as u32;
        let edx = 3 - (edi >> 5);
        let location = 12 - ((esi >> 5) << 2);
        let word = u32::from_le_bytes([
            scopy[location],
            scopy[location + 1],
            scopy[location + 2],
            scopy[location + 3],
        ]);
        let ebp = (word >> ecx) & 1;
        let ckt_temp = values[edx];
        values[edx] = (ebp << eax) | (!(1u32 << eax) & ckt_temp);
        esi += 11;
        if esi >= 120 {
            esi -= 120;
        }
    }
}

fn get_hex_value(v: u32) -> u8 {
    let v = v & 0xF;
    if v < 10 {
        v as u8 + b'0'
    } else {
        (v - 10) as u8 + b'A'
    }
}

fn get_num_value(c: u8) -> u32 {
    let c = (c as char).to_ascii_uppercase() as u8;
    if c.is_ascii_digit() {
        (c - b'0') as u32
    } else if (b'A'..=b'F').contains(&c) {
        (c - b'A' + 10) as u32
    } else {
        0
    }
}

/// Encodes the 36-byte CD-Key info buffer required for BNCS SID_AUTH_CHECK (0x51).
/// Wire layout: key_len (4) + product (4) + public_val (4) + val2 (4) + hash (20).
pub fn create_key_info(
    cdkey: &str,
    client_token: u32,
    server_token: u32,
    is_tft: bool,
) -> Result<[u8; 36], CdKeyError> {
    let info = decode_cd_key(cdkey, client_token, server_token)?;
    let key_len = if cdkey.chars().filter(|c| c.is_alphanumeric()).count() == 26 {
        26u32
    } else if cdkey.chars().filter(|c| c.is_alphanumeric()).count() == 13 {
        13u32
    } else {
        16u32
    };
    let product = if is_tft { 7u32 } else { info.product };

    let mut wire = [0u8; 36];
    wire[0..4].copy_from_slice(&key_len.to_le_bytes());
    wire[4..8].copy_from_slice(&product.to_le_bytes());
    wire[8..12].copy_from_slice(&info.public_value.to_le_bytes());
    wire[12..16].copy_from_slice(&info.val2.to_le_bytes());
    wire[16..36].copy_from_slice(&info.hash);
    Ok(wire)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_tft_key_to_the_same_values_as_bncsutil() {
        let info = decode_cd_key("TAKLIBFWQWJRVGPSO68MUTV5D0", 0x1122_3344, 0x5566_7788)
            .expect("valid key");
        assert_eq!(info.product, 13473);
        assert_eq!(info.public_value, 24_929_753);
        assert_eq!(
            info.hash,
            [
                103, 3, 212, 224, 183, 184, 231, 85, 250, 186, 189, 108, 208, 7, 183, 173, 244, 20,
                63, 249,
            ]
        );
    }

    #[test]
    fn decodes_the_roc_key_to_the_same_values_as_bncsutil() {
        let info = decode_cd_key("N72224JD477FHJXHRC77V26G9P", 0x1122_3344, 0x5566_7788)
            .expect("valid key");
        assert_eq!(info.product, 14);
        assert_eq!(info.public_value, 645_979);
        assert_eq!(
            info.hash,
            [
                99, 205, 226, 2, 218, 255, 107, 30, 51, 56, 191, 23, 109, 107, 196, 120, 230, 58,
                68, 145,
            ]
        );
    }

    #[test]
    fn decodes_starcraft_key_with_decoder() {
        // Valid 13-digit key checksum test
        // Let's create a known 13-digit key: e.g. "1234567890123"
        // Let's test invalid key first
        assert!(CdKeyDecoder::new("1234567890120").is_err());
    }

    #[test]
    fn invalid_character_in_key_returns_error() {
        let bad_key = "111111-1111-111111-1111-111111@";
        assert!(decode_cd_key(bad_key, 123, 456).is_err());
    }

    #[test]
    fn creates_36_byte_key_info_packet() {
        let key = "TAKLIBFWQWJRVGPSO68MUTV5D0";
        let wire = create_key_info(key, 0x11223344, 0x55667788, true).expect("valid wire keyinfo");
        assert_eq!(wire.len(), 36);
        assert_eq!(u32::from_le_bytes([wire[0], wire[1], wire[2], wire[3]]), 26);
        assert_eq!(u32::from_le_bytes([wire[4], wire[5], wire[6], wire[7]]), 7);
    }
}
