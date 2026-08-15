use sha1::{Digest, Sha1};

pub use crate::bncsutil::cdkey::CdKeyError;

/// Hashes a password using Battle.net XSHA-1.
/// Authoritative implementation from `crates/ghost-bnet/src/bncsutil/xsha1.rs`.
pub fn hash_password_pvpgn(password: &str) -> [u8; 20] {
    crate::bncsutil::xsha1::hash_password(password)
}

/// Standard SHA-1 hash over lowercased ASCII password.
pub fn hash_password_standard_sha1(password: &str) -> [u8; 20] {
    let lower = password.to_ascii_lowercase();
    let mut hasher = Sha1::new();
    hasher.update(lower.as_bytes());
    let res = hasher.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&res);
    out
}

/// Computes double-hashed password for BNCS SID_LOGONRESPONSE:
/// SHA1(client_token + server_token + hash_password_pvpgn(password))
pub fn hash_password_double(password: &str, client_token: u32, server_token: u32) -> [u8; 20] {
    let h1 = hash_password_pvpgn(password);
    let mut hasher = Sha1::new();
    hasher.update(client_token.to_le_bytes());
    hasher.update(server_token.to_le_bytes());
    hasher.update(h1);
    let result = hasher.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&result);
    out
}

/// Builds 36-byte CD-Key info for PvPGN SID_AUTH_CHECK using the pure-Rust CD-key decoder:
/// 4 bytes len (26), 4 bytes product (ROC=4, TFT=7), 4 bytes public_val, 4 bytes val2, 20 bytes hash.
pub fn create_key_info(
    cdkey: &str,
    client_token: u32,
    server_token: u32,
    is_tft: bool,
) -> Result<[u8; 36], CdKeyError> {
    crate::bncsutil::cdkey::create_key_info(cdkey, client_token, server_token, is_tft)
}

/// Generates a random 32-byte client public key for NLS login.
pub fn generate_client_key() -> [u8; 32] {
    let mut k = [0u8; 32];
    for b in &mut k {
        *b = rand::random();
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pvpgn_password_hash_matches_authoritative_xsha1() {
        let h = hash_password_pvpgn("password");
        assert_eq!(
            h,
            [
                0xec, 0xc8, 0x0d, 0x1d, 0x76, 0xe7, 0x58, 0xc0, 0xb9, 0xda, 0x8c, 0x25, 0xff,
                0x10, 0x6a, 0xff, 0x8e, 0x24, 0x29, 0x16,
            ]
        );
    }

    #[test]
    fn pvpgn_password_hash_is_case_sensitive() {
        assert_ne!(
            hash_password_pvpgn("PassWord"),
            hash_password_pvpgn("password")
        );
    }

    #[test]
    fn key_info_returns_36_bytes_on_valid_key() {
        let k = create_key_info("TAKLIBFWQWJRVGPSO68MUTV5D0", 0x1122_3344, 0x5566_7788, true)
            .expect("valid key info");
        assert_eq!(k.len(), 36);
        assert_eq!(u32::from_le_bytes([k[0], k[1], k[2], k[3]]), 26);
        assert_eq!(u32::from_le_bytes([k[4], k[5], k[6], k[7]]), 7);
    }

    #[test]
    fn key_info_returns_error_on_invalid_key() {
        let err = create_key_info("INVALID!KEY", 123, 456, true);
        assert!(err.is_err());
    }
}
