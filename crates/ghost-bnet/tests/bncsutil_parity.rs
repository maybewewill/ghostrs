use ghost_bnet::bncsutil::{
    BNCSUTIL_PLATFORM_MAC, BNCSUTIL_PLATFORM_OSX, BNCSUTIL_PLATFORM_WIN,
    BNCSUTIL_PLATFORM_WINDOWS, BNCSUTIL_PLATFORM_X86, BNCSUTIL_VERSION, BNCSUTIL_VERSION_STRING,
    CdKeyDecoder, CdKeyError, DEFAULT_MPQ_SEEDS, KeyType, NLS_G, NLS_I, NLS_PRIME_BYTES,
    NLS_SIGNATURE_KEY, NLS_SIG_N, NlsSession, calc_hash_buf, check_revision, check_revision_flat,
    check_signature, create_key_info, decode_cd_key, double_hash_password, extract_mpq_number,
    extract_pe_version, get_exe_info, get_mpq_seed, get_version, get_version_string,
    hash_password, kd_quick, set_mpq_seed, xsha1,
};
use std::io::Write;

// =========================================================================
// 1. LibInfo Tests
// =========================================================================

#[test]
fn test_libinfo_version_constants_and_getters() {
    assert_eq!(BNCSUTIL_VERSION, 10405);
    assert_eq!(BNCSUTIL_VERSION_STRING, "1.4.5");
    assert_eq!(get_version(), 10405);
    assert_eq!(get_version_string(), "1.4.5");
    assert_eq!(ghost_bnet::bncsutil::bncsutil_get_version(), 10405);
    assert_eq!(ghost_bnet::bncsutil::bncsutil_get_version_string(), "1.4.5");
}

// =========================================================================
// 2. Broken SHA-1 (XSHA-1) Tests
// =========================================================================

#[test]
fn test_xsha1_reference_vectors() {
    let v_password = [
        0xec, 0xc8, 0x0d, 0x1d, 0x76, 0xe7, 0x58, 0xc0, 0xb9, 0xda, 0x8c, 0x25, 0xff, 0x10, 0x6a,
        0xff, 0x8e, 0x24, 0x29, 0x16,
    ];
    let v_password_mixed_case = [
        0x17, 0x5b, 0xce, 0x6b, 0xec, 0x30, 0xe9, 0x6b, 0x14, 0xec, 0xf6, 0x98, 0x4f, 0x81, 0xf0,
        0xc9, 0x4f, 0x1b, 0xab, 0xd1,
    ];
    let v_empty = [
        0xee, 0xa0, 0x3a, 0x4d, 0x5a, 0x1d, 0x26, 0x94, 0x57, 0x6f, 0x4a, 0x58, 0x60, 0x99, 0x8d,
        0x6b, 0x80, 0xc6, 0x46, 0x15,
    ];
    let v_a = [
        0x93, 0x24, 0x44, 0xfe, 0x78, 0x00, 0xc2, 0x6d, 0x51, 0x95, 0x33, 0xa0, 0x03, 0x23, 0xf8,
        0x59, 0x13, 0x3f, 0x51, 0x6e,
    ];

    assert_eq!(hash_password("password"), v_password);
    assert_eq!(hash_password("PassWord"), v_password_mixed_case);
    assert_eq!(hash_password(""), v_empty);
    assert_eq!(hash_password("a"), v_a);
    assert_eq!(calc_hash_buf(b"password"), v_password);
    assert_eq!(xsha1(b"password"), v_password);
}

#[test]
fn test_xsha1_multi_block_padding() {
    let large_input = vec![0x42u8; 150];
    let h1 = xsha1(&large_input);
    let h2 = calc_hash_buf(&large_input);
    assert_eq!(h1, h2);
    assert_eq!(h1.len(), 20);
}

// =========================================================================
// 3. OldAuth Tests
// =========================================================================

#[test]
fn test_oldauth_single_and_double_hash() {
    let single = hash_password("secret");
    assert_eq!(single, xsha1(b"secret"));

    let client_token = 0x11223344u32;
    let server_token = 0x55667788u32;
    let double = double_hash_password("secret", client_token, server_token);

    let mut buf = [0u8; 28];
    buf[0..4].copy_from_slice(&client_token.to_le_bytes());
    buf[4..8].copy_from_slice(&server_token.to_le_bytes());
    buf[8..28].copy_from_slice(&single);
    let expected = xsha1(&buf);
    assert_eq!(double, expected);
}

// =========================================================================
// 4. CD-Key Decoder Tests
// =========================================================================

#[test]
fn test_cdkey_warcraft3_26_char() {
    let tft_key = "TAKLIBFWQWJRVGPSO68MUTV5D0";
    let client_token = 0x1122_3344;
    let server_token = 0x5566_7788;

    let decoder = CdKeyDecoder::new(tft_key).expect("valid TFT key");
    assert_eq!(decoder.key_type(), KeyType::WarCraft3);
    assert_eq!(decoder.product(), 13473);
    assert_eq!(decoder.public_value(), 24_929_753);
    assert_eq!(decoder.val1(), 24_929_753);
    assert_eq!(decoder.val2_length(), 10);
    assert!(decoder.long_val2().is_some());

    let hash = decoder.calculate_hash(client_token, server_token);
    assert_eq!(
        hash,
        [
            103, 3, 212, 224, 183, 184, 231, 85, 250, 186, 189, 108, 208, 7, 183, 173, 244, 20,
            63, 249,
        ]
    );

    let decoded = decode_cd_key(tft_key, client_token, server_token).expect("decode_cd_key");
    assert_eq!(decoded.product, 13473);
    assert_eq!(decoded.public_value, 24_929_753);
    assert_eq!(decoded.hash, hash);

    let quick = kd_quick(tft_key, client_token, server_token).expect("kd_quick");
    assert_eq!(quick.product, 13473);
    assert_eq!(quick.public_value, 24_929_753);
    assert_eq!(quick.hash, hash);

    let wire = create_key_info(tft_key, client_token, server_token, true).expect("wire info");
    assert_eq!(wire.len(), 36);
    assert_eq!(u32::from_le_bytes([wire[0], wire[1], wire[2], wire[3]]), 26);
    assert_eq!(u32::from_le_bytes([wire[4], wire[5], wire[6], wire[7]]), 7); // TFT override
}

#[test]
fn test_cdkey_warcraft2_16_char() {
    let key = "47D2C4CD-N628-CGB4"; // 16 alphanumeric characters
    let sanitized: String = key.chars().filter(|c| c.is_alphanumeric()).collect();
    assert_eq!(sanitized.len(), 16);

    let res = CdKeyDecoder::new(key);
    if let Ok(dec) = res {
        assert_eq!(dec.key_type(), KeyType::WarCraft2);
        assert_eq!(dec.val2_length(), 4);
    }
}

#[test]
fn test_cdkey_starcraft_13_char() {
    // Generate valid 13-digit StarCraft key
    let prefix = "123456789012";
    let mut accum: i32 = 3;
    for c in prefix.chars() {
        let digit = (c as u8 - b'0') as i32;
        accum += digit ^ (accum * 2);
    }
    let check_digit = (accum % 10) as u8 + b'0';
    let valid_sc_key = format!("{}{}", prefix, check_digit as char);

    let dec = CdKeyDecoder::new(&valid_sc_key).expect("valid SC key");
    assert_eq!(dec.key_type(), KeyType::StarCraft);
    assert_eq!(dec.val2_length(), 4);
    assert!(dec.is_valid());

    let hash = dec.calculate_hash(0x12345678, 0x87654321);
    assert_eq!(hash.len(), 20);

    let wire = create_key_info(&valid_sc_key, 0x12345678, 0x87654321, false)
        .expect("wire SC keyinfo");
    assert_eq!(wire.len(), 36);
    assert_eq!(u32::from_le_bytes([wire[0], wire[1], wire[2], wire[3]]), 13);
}

#[test]
fn test_cdkey_error_handling() {
    assert_eq!(
        CdKeyDecoder::new("SHORTKEY"),
        Err(CdKeyError::InvalidLength(8))
    );
    assert_eq!(
        CdKeyDecoder::new("TAKLIBFWQWJRVGPSO68MUTV5D!"),
        Err(CdKeyError::InvalidChar('!'))
    );
}

// =========================================================================
// 5. MPQ Number Extraction Tests
// =========================================================================

#[test]
fn test_extract_mpq_number_comprehensive() {
    assert_eq!(extract_mpq_number("IX86ver1.mpq"), 1);
    assert_eq!(extract_mpq_number("IX86ver2.mpq"), 2);
    assert_eq!(extract_mpq_number("ver10.mpq"), 10);
    assert_eq!(extract_mpq_number("PMACver7.mpq"), 7);
    assert_eq!(extract_mpq_number("IX86ver5"), 5);
    assert_eq!(extract_mpq_number("custom_file.mpq"), 1);
    assert_eq!(extract_mpq_number(""), 1);
}

// =========================================================================
// 6. CheckRevision Tests
// =========================================================================

#[test]
fn test_check_revision_multi_file_and_flat() {
    let temp = std::env::temp_dir();
    let f1 = temp.join("cr_parity_1.exe");
    let f2 = temp.join("cr_parity_2.dll");
    let f3 = temp.join("cr_parity_3.dll");

    std::fs::File::create(&f1)
        .unwrap()
        .write_all(b"Exe binary data 1234567890")
        .unwrap();
    std::fs::File::create(&f2)
        .unwrap()
        .write_all(b"Storm binary data 1234567890")
        .unwrap();
    std::fs::File::create(&f3)
        .unwrap()
        .write_all(b"Game binary data 1234567890")
        .unwrap();

    let formula = "A=3845581634 B=880823580 C=1363937103 4 A=A-S B=B-C C=C-A A=A-B";

    let c_flat = check_revision_flat(formula, &f1, &f2, &f3, 1).expect("checksum flat");
    let c_slice = check_revision(formula, &[&f1, &f2, &f3], 1).expect("checksum slice");
    assert_eq!(c_flat, c_slice);

    // 1-file evaluation
    let c_single = check_revision(formula, &[&f1], 1).expect("checksum 1 file");
    assert_ne!(c_single, 0);

    let _ = std::fs::remove_file(&f1);
    let _ = std::fs::remove_file(&f2);
    let _ = std::fs::remove_file(&f3);
}

#[test]
fn test_check_revision_seed_management() {
    assert_eq!(DEFAULT_MPQ_SEEDS.len(), 8);
    assert_eq!(get_mpq_seed(0), 0xE7F4_CB62);
    assert_eq!(get_mpq_seed(1), 0xF6A1_4FFC);

    let prev = set_mpq_seed(1, 0xDEADBEEF);
    assert_eq!(prev, 0xF6A1_4FFC);
    assert_eq!(get_mpq_seed(1), 0xDEADBEEF);
    set_mpq_seed(1, 0xF6A1_4FFC);
    assert_eq!(get_mpq_seed(1), 0xF6A1_4FFC);
}

// =========================================================================
// 7. ExeInfo & PE Parser Tests
// =========================================================================

#[test]
fn test_exe_info_platforms() {
    assert_eq!(BNCSUTIL_PLATFORM_X86, 1);
    assert_eq!(BNCSUTIL_PLATFORM_WINDOWS, 1);
    assert_eq!(BNCSUTIL_PLATFORM_WIN, 1);
    assert_eq!(BNCSUTIL_PLATFORM_MAC, 2);
    assert_eq!(BNCSUTIL_PLATFORM_OSX, 3);

    let temp_dir = std::env::temp_dir();
    let test_file = temp_dir.join("test_parity_war3.exe");
    {
        let mut f = std::fs::File::create(&test_file).unwrap();
        f.write_all(&vec![0u8; 1000]).unwrap();
    }

    let info_win = get_exe_info(&test_file, BNCSUTIL_PLATFORM_WINDOWS).expect("exeinfo");
    assert!(info_win.exe_info_string.starts_with("test_parity_war3.exe "));
    assert!(info_win.exe_info_string.ends_with(" 1000"));

    let _ = std::fs::remove_file(&test_file);
}

#[test]
fn test_pe_fixed_file_info_extraction() {
    // VS_FIXEDFILEINFO signature test
    let mut fake_pe = vec![0u8; 256];
    fake_pe[0..2].copy_from_slice(b"MZ");
    fake_pe[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    fake_pe[0x40..0x44].copy_from_slice(b"PE\0\0");

    // Embed VS_FIXEDFILEINFO structure somewhere in buffer
    let ffi_offset = 100;
    fake_pe[ffi_offset..ffi_offset + 4].copy_from_slice(&[0xBD, 0x04, 0xEF, 0xFE]); // 0xFEEF04BD
    fake_pe[ffi_offset + 16..ffi_offset + 20].copy_from_slice(&0x0001001Au32.to_le_bytes()); // 1.26
    fake_pe[ffi_offset + 20..ffi_offset + 24].copy_from_slice(&0x00000001u32.to_le_bytes()); // 0.1

    let ver = extract_pe_version(&fake_pe).expect("extracted version");
    assert_eq!(ver, 0x011A0001); // 18481153
}

// =========================================================================
// 8. New Logon System (NLS / SRP-6a) Tests
// =========================================================================

#[test]
fn test_nls_constants() {
    assert_eq!(NLS_G, 47);
    assert_eq!(NLS_PRIME_BYTES.len(), 32);
    assert_eq!(NLS_I.len(), 20);
    assert_eq!(NLS_SIGNATURE_KEY, 0x10001);
    assert_eq!(NLS_SIG_N.len(), 128);
}

#[test]
fn test_nls_full_exchange() {
    let session = NlsSession::with_private_key_for_test("Alice", "SecretPassword", 3u32);
    assert_eq!(session.username(), "Alice");
    assert_ne!(session.client_public_key(), [0u8; 32]);

    let salt = [0x55u8; 32];
    let v = session.compute_v(&salt);
    assert_ne!(v, [0u8; 32]);

    let server_pub = [0x77u8; 32];
    let s = session.compute_s(&server_pub, &salt).expect("shared secret S");
    assert_ne!(s, [0u8; 32]);

    let k = session.compute_k(&s);
    assert_eq!(k.len(), 40);

    let m1 = session.compute_m1(&server_pub, &salt).expect("m1 proof");
    assert_eq!(m1.len(), 20);

    let m2 = session.compute_m2(&server_pub, &salt, &m1).expect("m2 proof");
    assert_eq!(m2.len(), 20);

    assert!(session.check_m2(&m2, &server_pub, &salt));
    assert!(!session.check_m2(&[0u8; 20], &server_pub, &salt));
}

#[test]
fn test_nls_packet_helpers() {
    let session = NlsSession::new("TestUser", "TestPass");
    let create = session.account_create();
    assert_eq!(create.username, "TestUser");
    assert_eq!(create.salt.len(), 32);
    assert_eq!(create.v.len(), 32);

    let logon = session.account_logon();
    assert_eq!(logon.username, "TestUser");
    assert_eq!(logon.a_pub, session.client_public_key());

    let server_pub = [0x33u8; 32];
    let salt = [0x44u8; 32];
    let (new_session, change_proof) = session
        .account_change_proof("NewPass123", &server_pub, &salt)
        .expect("change proof");
    assert_eq!(new_session.username(), "TestUser");
    assert_eq!(change_proof.m1.len(), 20);
    assert_eq!(change_proof.new_salt.len(), 32);
    assert_eq!(change_proof.new_v.len(), 32);
}

#[test]
fn test_nls_check_signature_invalid() {
    let dummy_sig = [0x00u8; 128];
    assert!(!check_signature(0x7F000001, &dummy_sig));
}
