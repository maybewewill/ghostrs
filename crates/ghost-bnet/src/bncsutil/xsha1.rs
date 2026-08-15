/// Computes the Battle.net "Broken SHA-1" (XSHA-1) digest over an arbitrary byte buffer.
/// Chunks are padded with zeros to a multiple of 64 bytes (empty input still processes
/// one 64-byte block of zeroes) and parsed as little-endian 32-bit words.
pub fn xsha1(data: &[u8]) -> [u8; 20] {
    let mut hash = [
        0x6745_2301u32,
        0xEFCD_AB89u32,
        0x98BA_DCFEu32,
        0x1032_5476u32,
        0xC3D2_E1F0u32,
    ];

    let mut padded = data.to_vec();
    let rem = padded.len() % 64;
    if rem != 0 || padded.is_empty() {
        padded.resize(padded.len() + if padded.is_empty() { 64 } else { 64 - rem }, 0);
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
        for i in 0..64 {
            let val = (w[i] ^ w[i + 8] ^ w[i + 2] ^ w[i + 13]) % 32;
            w[i + 16] = 1u32.rotate_left(val);
        }

        let mut a = hash[0];
        let mut b = hash[1];
        let mut c = hash[2];
        let mut d = hash[3];
        let mut e = hash[4];

        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A82_7999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9_EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDCu32),
                _ => (b ^ c ^ d, 0xCA62_C1D6u32),
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

/// Hashes a Battle.net password using XSHA-1.
///
/// Note: XSHA-1 is case-sensitive; this does not lowercase the password.
pub fn hash_password(password: &str) -> [u8; 20] {
    xsha1(password.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ground truth: bncsutil.dll `hashPassword`, captured 2026-08-15.
    const V_PASSWORD: [u8; 20] = [
        0xec, 0xc8, 0x0d, 0x1d, 0x76, 0xe7, 0x58, 0xc0, 0xb9, 0xda,
        0x8c, 0x25, 0xff, 0x10, 0x6a, 0xff, 0x8e, 0x24, 0x29, 0x16,
    ];
    const V_PASSWORD_MIXED_CASE: [u8; 20] = [
        0x17, 0x5b, 0xce, 0x6b, 0xec, 0x30, 0xe9, 0x6b, 0x14, 0xec,
        0xf6, 0x98, 0x4f, 0x81, 0xf0, 0xc9, 0x4f, 0x1b, 0xab, 0xd1,
    ];
    const V_EMPTY: [u8; 20] = [
        0xee, 0xa0, 0x3a, 0x4d, 0x5a, 0x1d, 0x26, 0x94, 0x57, 0x6f,
        0x4a, 0x58, 0x60, 0x99, 0x8d, 0x6b, 0x80, 0xc6, 0x46, 0x15,
    ];
    const V_A: [u8; 20] = [
        0x93, 0x24, 0x44, 0xfe, 0x78, 0x00, 0xc2, 0x6d, 0x51, 0x95,
        0x33, 0xa0, 0x03, 0x23, 0xf8, 0x59, 0x13, 0x3f, 0x51, 0x6e,
    ];

    #[test]
    fn xsha1_matches_the_bncsutil_vectors() {
        assert_eq!(hash_password("password"), V_PASSWORD);
        assert_eq!(hash_password(""), V_EMPTY, "empty input must still run one padded block");
        assert_eq!(hash_password("a"), V_A);
    }

    #[test]
    fn xsha1_is_case_sensitive() {
        assert_eq!(hash_password("PassWord"), V_PASSWORD_MIXED_CASE);
        assert_ne!(
            hash_password("PassWord"),
            hash_password("password"),
            "bncsutil does not fold case; lowercasing the password changes the digest"
        );
    }
}
