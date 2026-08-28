

use num_bigint::BigUint;
use num_traits::Zero;
use sha1::{Digest, Sha1};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum NlsError {
    #[error("invalid server public key (zero or out of field)")]
    InvalidServerPublicKey,
}

pub const NLS_PRIME_BYTES: [u8; 32] = [
    0x87, 0xC7, 0x23, 0x85, 0x65, 0xF6, 0x16, 0x12, 0xD9, 0x12, 0x32, 0xC7, 0x78, 0x6C, 0x97, 0x7E,
    0x55, 0xB5, 0x92, 0xA0, 0x8C, 0xB6, 0x86, 0x21, 0x03, 0x18, 0x99, 0x61, 0x8B, 0x1A, 0xFF, 0xF8,
];

pub const NLS_G: u32 = 47;

pub const NLS_I: [u8; 20] = [
    0x6C, 0x0E, 0x97, 0xED, 0x0A, 0xF9, 0x6B, 0xAB, 0xB1, 0x58, 0x89, 0xEB, 0x8B, 0xBA, 0x25, 0xA4,
    0xF0, 0x8C, 0x01, 0xF8,
];

pub const NLS_SIGNATURE_KEY: u32 = 0x10001;

pub const NLS_SIG_N: [u8; 128] = [
    0xD5, 0xA3, 0xD6, 0xAB, 0x0F, 0x0D, 0xC5, 0x0F, 0xC3, 0xFA, 0x6E, 0x78, 0x9D, 0x0B, 0xE3, 0x32,
    0xB0, 0xFA, 0x20, 0xE8, 0x42, 0x19, 0xB4, 0xA1, 0x3A, 0x3B, 0xCD, 0x0E, 0x8F, 0xB5, 0x56, 0xB5,
    0xDC, 0xE5, 0xC1, 0xFC, 0x2D, 0xBA, 0x56, 0x35, 0x29, 0x0F, 0x48, 0x0B, 0x15, 0x5A, 0x39, 0xFC,
    0x88, 0x07, 0x43, 0x9E, 0xCB, 0xF3, 0xB8, 0x73, 0xC9, 0xE1, 0x77, 0xD5, 0xA1, 0x06, 0xA6, 0x20,
    0xD0, 0x82, 0xC5, 0x2D, 0x4D, 0xD3, 0x25, 0xF4, 0xFD, 0x26, 0xFC, 0xE4, 0xC2, 0x00, 0xDD, 0x98,
    0x2A, 0xF4, 0x3D, 0x5E, 0x08, 0x8A, 0xD3, 0x20, 0x41, 0x84, 0x32, 0x69, 0x8E, 0x8A, 0x34, 0x76,
    0xEA, 0x16, 0x8E, 0x66, 0x40, 0xD9, 0x32, 0xB0, 0x2D, 0xF5, 0xBD, 0xE7, 0x57, 0x51, 0x78, 0x96,
    0xC2, 0xED, 0x40, 0x41, 0xCC, 0x54, 0x9D, 0xFD, 0xB6, 0x8D, 0xC2, 0xBA, 0x7F, 0x69, 0x8D, 0xCF,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NlsAccountCreatePacket {
    pub salt: [u8; 32],
    pub v: [u8; 32],
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NlsAccountLogonPacket {
    pub a_pub: [u8; 32],
    pub username: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NlsChangeProofPacket {
    pub m1: [u8; 20],
    pub new_salt: [u8; 32],
    pub new_v: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct NlsSession {
    username: String,
    password: String,
    a: BigUint,
    a_pub: [u8; 32],
}

impl NlsSession {

    pub fn new(username: &str, password: &str) -> Self {
        let n = BigUint::from_bytes_le(&NLS_PRIME_BYTES);
        let g = BigUint::from(NLS_G);

        let mut a_bytes = [0u8; 32];
        for b in &mut a_bytes {
            *b = rand::random();
        }
        let a = BigUint::from_bytes_le(&a_bytes) % &n;

        let a_biguint = g.modpow(&a, &n);
        let a_le = a_biguint.to_bytes_le();
        let mut a_pub = [0u8; 32];
        let len = a_le.len().min(32);
        a_pub[..len].copy_from_slice(&a_le[..len]);

        Self {
            username: username.to_string(),
            password: password.to_string(),
            a,
            a_pub,
        }
    }

    pub fn with_private_key_for_test(username: &str, password: &str, private_key: u32) -> Self {
        let n = BigUint::from_bytes_le(&NLS_PRIME_BYTES);
        let g = BigUint::from(NLS_G);
        let a = BigUint::from(private_key);

        let a_biguint = g.modpow(&a, &n);
        let a_le = a_biguint.to_bytes_le();
        let mut a_pub = [0u8; 32];
        let len = a_le.len().min(32);
        a_pub[..len].copy_from_slice(&a_le[..len]);

        Self {
            username: username.to_string(),
            password: password.to_string(),
            a,
            a_pub,
        }
    }

    #[inline]
    pub fn username(&self) -> &str {
        &self.username
    }

    #[inline]
    pub fn client_public_key(&self) -> [u8; 32] {
        self.a_pub
    }

    pub fn compute_v(&self, salt: &[u8; 32]) -> [u8; 32] {
        let n = BigUint::from_bytes_le(&NLS_PRIME_BYTES);
        let g = BigUint::from(NLS_G);
        let x = self.compute_x(salt);
        let v = g.modpow(&x, &n);

        let mut v_bytes = [0u8; 32];
        let v_le = v.to_bytes_le();
        let len = v_le.len().min(32);
        v_bytes[..len].copy_from_slice(&v_le[..len]);
        v_bytes
    }

    fn compute_x(&self, salt: &[u8; 32]) -> BigUint {
        let upper_user = self.username.to_ascii_uppercase();
        let upper_pass = self.password.to_ascii_uppercase();
        let userpass = format!("{upper_user}:{upper_pass}");
        let userpass_hash = sha1_digest(userpass.as_bytes());

        let mut x_hasher = Sha1::new();
        x_hasher.update(salt);
        x_hasher.update(userpass_hash);
        let x_bytes = x_hasher.finalize();
        BigUint::from_bytes_le(&x_bytes)
    }

    pub fn compute_s(
        &self,
        server_public_key: &[u8; 32],
        salt: &[u8; 32],
    ) -> Result<[u8; 32], NlsError> {
        let n = BigUint::from_bytes_le(&NLS_PRIME_BYTES);
        let g = BigUint::from(NLS_G);
        let b = BigUint::from_bytes_le(server_public_key);

        if b.is_zero() || b >= n {
            return Err(NlsError::InvalidServerPublicKey);
        }

        let x = self.compute_x(salt);
        let v = g.modpow(&x, &n);

        let u_hash = sha1_digest(server_public_key);
        let u_val = u32::from_be_bytes([u_hash[0], u_hash[1], u_hash[2], u_hash[3]]);
        let u = BigUint::from(u_val);

        let v_mod_n = v % &n;
        let base = (&b + &n - v_mod_n) % &n;
        let exp = &self.a + (u * x);
        let s = base.modpow(&exp, &n);

        let mut s_bytes = [0u8; 32];
        let s_le = s.to_bytes_le();
        let s_len = s_le.len().min(32);
        s_bytes[..s_len].copy_from_slice(&s_le[..s_len]);
        Ok(s_bytes)
    }

    pub fn compute_k(&self, s_bytes: &[u8; 32]) -> [u8; 40] {
        let mut odd = [0u8; 16];
        let mut even = [0u8; 16];
        for i in 0..16 {
            odd[i] = s_bytes[i * 2];
            even[i] = s_bytes[i * 2 + 1];
        }
        let odd_hash = sha1_digest(&odd);
        let even_hash = sha1_digest(&even);

        let mut k = [0u8; 40];
        for i in 0..20 {
            k[i * 2] = odd_hash[i];
            k[i * 2 + 1] = even_hash[i];
        }
        k
    }

    pub fn compute_m1(
        &self,
        server_public_key: &[u8; 32],
        salt: &[u8; 32],
    ) -> Result<[u8; 20], NlsError> {
        let s = self.compute_s(server_public_key, salt)?;
        let k = self.compute_k(&s);

        let upper_user = self.username.to_ascii_uppercase();
        let username_hash = sha1_digest(upper_user.as_bytes());

        let mut m1_hasher = Sha1::new();
        m1_hasher.update(NLS_I);
        m1_hasher.update(username_hash);
        m1_hasher.update(salt);
        m1_hasher.update(self.a_pub);
        m1_hasher.update(server_public_key);
        m1_hasher.update(k);
        let res = m1_hasher.finalize();

        let mut m1 = [0u8; 20];
        m1.copy_from_slice(&res);
        Ok(m1)
    }

    pub fn compute_m2(
        &self,
        server_public_key: &[u8; 32],
        salt: &[u8; 32],
        m1: &[u8; 20],
    ) -> Result<[u8; 20], NlsError> {
        let s = self.compute_s(server_public_key, salt)?;
        let k = self.compute_k(&s);

        let mut m2_hasher = Sha1::new();
        m2_hasher.update(self.a_pub);
        m2_hasher.update(m1);
        m2_hasher.update(k);
        let res = m2_hasher.finalize();

        let mut m2 = [0u8; 20];
        m2.copy_from_slice(&res);
        Ok(m2)
    }

    pub fn check_m2(
        &self,
        var_m2: &[u8; 20],
        server_public_key: &[u8; 32],
        salt: &[u8; 32],
    ) -> bool {
        if let Ok(m1) = self.compute_m1(server_public_key, salt)
            && let Ok(expected_m2) = self.compute_m2(server_public_key, salt, &m1)
        {
            &expected_m2 == var_m2
        } else {
            false
        }
    }

    pub fn account_create(&self) -> NlsAccountCreatePacket {
        let mut salt = [0u8; 32];
        for b in &mut salt {
            *b = rand::random();
        }
        let v = self.compute_v(&salt);
        NlsAccountCreatePacket {
            salt,
            v,
            username: self.username.clone(),
        }
    }

    pub fn account_logon(&self) -> NlsAccountLogonPacket {
        NlsAccountLogonPacket {
            a_pub: self.a_pub,
            username: self.username.clone(),
        }
    }

    pub fn account_change_proof(
        &self,
        new_password: &str,
        server_public_key: &[u8; 32],
        salt: &[u8; 32],
    ) -> Result<(NlsSession, NlsChangeProofPacket), NlsError> {
        let m1 = self.compute_m1(server_public_key, salt)?;
        let new_session = NlsSession::new(&self.username, new_password);

        let mut new_salt = [0u8; 32];
        for b in &mut new_salt {
            *b = rand::random();
        }
        let new_v = new_session.compute_v(&new_salt);

        Ok((
            new_session,
            NlsChangeProofPacket {
                m1,
                new_salt,
                new_v,
            },
        ))
    }
}

pub fn check_signature(server_ip_be: u32, signature_raw: &[u8; 128]) -> bool {
    let mut check = [0xBBu8; 32];
    check[0..4].copy_from_slice(&server_ip_be.to_ne_bytes());

    let modulus = BigUint::from_bytes_le(&NLS_SIG_N);
    let signature = BigUint::from_bytes_le(signature_raw);
    let exponent = BigUint::from(NLS_SIGNATURE_KEY);

    let result = signature.modpow(&exponent, &modulus);
    let res_le = result.to_bytes_le();

    if res_le.len() < 32 {
        return false;
    }

    res_le[..32] == check[..32]
}

fn sha1_digest(data: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(data);
    let mut out = [0u8; 20];
    out.copy_from_slice(&hasher.finalize());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_public_key_is_g_pow_a_mod_n_for_a_known_private_key() {
        let nls = NlsSession::with_private_key_for_test("testuser", "secretpass", 2u32);
        let mut expected = [0u8; 32];
        expected[0..2].copy_from_slice(&2209u16.to_le_bytes());
        assert_eq!(nls.client_public_key(), expected);
    }

    #[test]
    fn m1_is_deterministic_for_a_fixed_private_key() {
        let nls = NlsSession::with_private_key_for_test("testuser", "secretpass", 2u32);
        let a = nls.compute_m1(&[0x42u8; 32], &[0x19u8; 32]).expect("m1");
        let b = nls.compute_m1(&[0x42u8; 32], &[0x19u8; 32]).expect("m1");
        assert_eq!(a, b, "M1 must be a pure function of its inputs");
    }

    #[test]
    fn rejects_zero_or_out_of_bounds_server_key() {
        let nls = NlsSession::with_private_key_for_test("testuser", "secretpass", 2u32);
        assert_eq!(
            nls.compute_m1(&[0u8; 32], &[0x19u8; 32]),
            Err(NlsError::InvalidServerPublicKey)
        );
    }

    #[test]
    fn computes_v_s_k_m2_consistently() {
        let nls = NlsSession::with_private_key_for_test("testuser", "secretpass", 2u32);
        let salt = [0x19u8; 32];
        let server_pub = [0x42u8; 32];

        let v = nls.compute_v(&salt);
        assert_ne!(v, [0u8; 32]);

        let s = nls.compute_s(&server_pub, &salt).expect("S");
        assert_ne!(s, [0u8; 32]);

        let k = nls.compute_k(&s);
        assert_eq!(k.len(), 40);

        let m1 = nls.compute_m1(&server_pub, &salt).expect("M1");
        let m2 = nls.compute_m2(&server_pub, &salt, &m1).expect("M2");
        assert!(nls.check_m2(&m2, &server_pub, &salt));
    }
}
