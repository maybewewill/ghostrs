use sha1::{Digest, Sha1};

#[allow(clippy::needless_range_loop)]
pub fn hash_password_pvpgn(password: &str) -> [u8; 20] {
    if let Some(bu) = crate::bncsutil::BncsUtil::global() {
        if let Some(h) = bu.hash_password(password) {
            return h;
        }
    }

    let lower = password.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut hash = [
        0x67452301u32,
        0xefcdab89u32,
        0x98badcfeu32,
        0x10325476u32,
        0xc3d2e1f0u32,
    ];

    let mut padded = bytes.to_vec();
    let rem = padded.len() % 64;
    if rem != 0 {
        padded.resize(padded.len() + (64 - rem), 0);
    }

    for chunk in padded.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let mut a = hash[0];
        let mut b = hash[1];
        let mut c = hash[2];
        let mut d = hash[3];
        let mut e = hash[4];
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5a827999),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        hash[0] = hash[0].wrapping_add(a);
        hash[1] = hash[1].wrapping_add(b);
        hash[2] = hash[2].wrapping_add(c);
        hash[3] = hash[3].wrapping_add(d);
        hash[4] = hash[4].wrapping_add(e);
    }
    let mut out = [0u8; 20];
    for (i, val) in hash.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&val.to_le_bytes());
    }
    out
}

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

/// Builds 36-byte CD-Key info for PvPGN SID_AUTH_CHECK:
/// 4 bytes len (26), 4 bytes product (ROC=4, TFT=7), 4 bytes public_val, 4 bytes val2, 20 bytes hash.
pub fn create_key_info(cdkey: &str, client_token: u32, server_token: u32, is_tft: bool) -> [u8; 36] {
    let sanitized: String = cdkey.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_uppercase();
    let mut out = [0u8; 36];
    let key_len = if sanitized.len() == 26 { 26u32 } else { 16u32 };
    let fallback_product = if is_tft { 7u32 } else { 4u32 };

    if let Some(bu) = crate::bncsutil::BncsUtil::global() {
        if let Some((public_val, product, hash)) = bu.kd_quick(&sanitized, client_token, server_token) {
            out[0..4].copy_from_slice(&key_len.to_le_bytes());
            out[4..8].copy_from_slice(&product.to_le_bytes());
            out[8..12].copy_from_slice(&public_val.to_le_bytes());
            out[12..16].copy_from_slice(&0u32.to_le_bytes());
            out[16..36].copy_from_slice(&hash);
            return out;
        }
    }

    // Fallback if bncsutil not available or fails
    out[0..4].copy_from_slice(&key_len.to_le_bytes());
    out[4..8].copy_from_slice(&fallback_product.to_le_bytes());
    out[8..12].copy_from_slice(&1u32.to_le_bytes());
    out[12..16].copy_from_slice(&0u32.to_le_bytes());

    let mut hasher = Sha1::new();
    hasher.update(client_token.to_le_bytes());
    hasher.update(server_token.to_le_bytes());
    hasher.update(sanitized.as_bytes());
    let hash = hasher.finalize();
    out[16..36].copy_from_slice(&hash);
    out
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
    fn pvpgn_password_hash_returns_20_bytes() {
        let h = hash_password_pvpgn("password123");
        println!("hash hex: {:02x?}", h);
        let s = h.iter().map(|b| format!("{b:02x}")).collect::<String>();
        println!("hash string: {}", s);
        assert_eq!(h.len(), 20);
        assert_ne!(h, [0u8; 20]);
    }

    #[test]
    fn key_info_returns_36_bytes() {
        let k = create_key_info("FFFFFFFFFFFFFFFFFFFFFFFFFF", 123, 456, true);
        assert_eq!(k.len(), 36);
        assert_eq!(u32::from_le_bytes([k[0], k[1], k[2], k[3]]), 26);
    }
}
