use crate::bncsutil::xsha1::{hash_password as xsha1_hash_password, xsha1};

#[inline]
pub fn hash_password(password: &str) -> [u8; 20] {
    xsha1_hash_password(password)
}

pub fn double_hash_password(password: &str, client_token: u32, server_token: u32) -> [u8; 20] {
    let h1 = hash_password(password);
    let mut intermediate = [0u8; 28];
    intermediate[0..4].copy_from_slice(&client_token.to_le_bytes());
    intermediate[4..8].copy_from_slice(&server_token.to_le_bytes());
    intermediate[8..28].copy_from_slice(&h1);
    xsha1(&intermediate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_hash_password_produces_consistent_xsha1() {
        let h = double_hash_password("password", 0x12345678, 0x9ABCDEF0);
        assert_eq!(h.len(), 20);

        let h1 = hash_password("password");
        let mut buf = [0u8; 28];
        buf[0..4].copy_from_slice(&0x12345678u32.to_le_bytes());
        buf[4..8].copy_from_slice(&0x9ABCDEF0u32.to_le_bytes());
        buf[8..28].copy_from_slice(&h1);
        let expected = xsha1(&buf);
        assert_eq!(h, expected);
    }
}
