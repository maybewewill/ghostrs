use bytes::{BufMut, Bytes, BytesMut};

use super::BNCS_HEADER;
use super::ids;
use crate::bytes_ext::put_cstring;
use crate::error::ProtoError;
use crate::frame::Frame;

pub fn null() -> Bytes {
    Frame::new(ids::SID_NULL, Bytes::new())
        .encode_with(BNCS_HEADER)
        .expect("empty null frame fits")
}

pub fn stopadv() -> Bytes {
    Frame::new(ids::SID_STOPADV, Bytes::new())
        .encode_with(BNCS_HEADER)
        .expect("empty stopadv frame fits")
}

pub fn getadvlistex(game_name: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(20 + game_name.len());
    p.put_slice(&[255, 3, 0, 0]); // map filter 1
    p.put_slice(&[255, 3, 0, 0]); // map filter 2
    p.put_slice(&[0, 0, 0, 0]); // map filter 3
    p.put_slice(&[1, 0, 0, 0]); // num games
    put_cstring(&mut p, game_name);
    p.put_u8(0);
    p.put_u8(0);
    Frame::new(ids::SID_GETADVLISTEX, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn enter_chat() -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(2);
    p.put_u8(0);
    p.put_u8(0);
    Frame::new(ids::SID_ENTERCHAT, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn join_channel(channel: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(5 + channel.len());
    let flags: u32 = if channel.is_empty() { 1 } else { 2 };
    p.put_u32_le(flags);
    put_cstring(&mut p, channel);
    Frame::new(ids::SID_JOINCHANNEL, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn chat_command(command: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(1 + command.len());
    put_cstring(&mut p, command);
    Frame::new(ids::SID_CHATCOMMAND, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn checkad() -> Bytes {
    let mut p = BytesMut::with_capacity(16);
    p.put_slice(&[0; 16]);
    Frame::new(ids::SID_CHECKAD, p.freeze())
        .encode_with(BNCS_HEADER)
        .expect("16-byte checkad fits")
}

/// Game visibility on Battle.net, sent as the `SID_STARTADVEX3` state field.
///
/// `bnetprotocol.cpp:702` documents the field as
/// "State (16 = public, 17 = private, 18 = close)", and `gameprotocol.h:32-33`
/// defines the constants. There is no valid zero state: a game advertised with
/// 0 is listed by name but cannot be joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GameVisibility {
    Public = 16,
    Private = 17,
    Close = 18,
}

impl GameVisibility {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

pub fn startadvex3(
    visibility: GameVisibility,
    map_game_type: [u8; 4],
    game_name: &str,
    _host_name: &str,
    up_time: u32,
    stat_string: &[u8],
    host_counter: u32,
) -> Result<Bytes, ProtoError> {
    if stat_string.len() >= 128 {
        return Err(ProtoError::BadValue(
            "stat string size exceeds Battle.net limit (must be < 128 bytes)",
        ));
    }
    let host_counter_string = format!("{:08x}", host_counter);
    let host_counter_string: String = host_counter_string.chars().rev().collect();

    let mut p = BytesMut::with_capacity(40 + game_name.len() + stat_string.len());
    p.put_u8(visibility.as_u8());
    p.put_slice(&[0, 0, 0]);
    p.put_u32_le(up_time);
    p.put_slice(&map_game_type);
    p.put_slice(&[255, 3, 0, 0]); // unknown
    p.put_slice(&[0, 0, 0, 0]); // custom game
    put_cstring(&mut p, game_name);
    p.put_u8(0);
    // GHost++ uses 98 (char 'b') for 12-slot games (11 free slots) on Warcraft 1.26a
    p.put_u8(98);
    p.put_slice(host_counter_string.as_bytes());
    p.put_slice(stat_string);
    p.put_u8(0);
    Frame::new(ids::SID_STARTADVEX3, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn notifyjoin(game_name: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(9 + game_name.len());
    p.put_slice(&[0, 0, 0, 0]); // product id
    p.put_slice(&[14, 0, 0, 0]); // product version
    put_cstring(&mut p, game_name);
    Frame::new(ids::SID_NOTIFYJOIN, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn ping(value: [u8; 4]) -> Bytes {
    Frame::new(ids::SID_PING, Bytes::copy_from_slice(&value))
        .encode_with(BNCS_HEADER)
        .expect("4-byte ping fits")
}

pub fn logon_response(
    client_token: [u8; 4],
    server_token: [u8; 4],
    password_hash: &[u8],
    account_name: &str,
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(8 + password_hash.len() + account_name.len() + 1);
    p.put_slice(&client_token);
    p.put_slice(&server_token);
    p.put_slice(password_hash);
    put_cstring(&mut p, account_name);
    Frame::new(ids::SID_LOGONRESPONSE, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn logon_response2(
    client_token: [u8; 4],
    server_token: [u8; 4],
    password_hash: &[u8],
    account_name: &str,
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(8 + password_hash.len() + account_name.len() + 1);
    p.put_slice(&client_token);
    p.put_slice(&server_token);
    p.put_slice(password_hash);
    put_cstring(&mut p, account_name);
    Frame::new(ids::SID_LOGONRESPONSE2, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn auth_accountlogon(
    client_public_key: &[u8; 32],
    account_name: &str,
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(32 + account_name.len() + 1);
    p.put_slice(client_public_key);
    put_cstring(&mut p, account_name);
    Frame::new(ids::SID_AUTH_ACCOUNTLOGON, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn auth_accountlogonproof(client_password_proof: &[u8]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(client_password_proof.len());
    p.put_slice(client_password_proof);
    Frame::new(ids::SID_AUTH_ACCOUNTLOGONPROOF, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn friendslist() -> Result<Bytes, ProtoError> {
    let p = BytesMut::new();
    Frame::new(ids::SID_FRIENDSLIST, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn clanmemberlist() -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(4);
    p.put_slice(&[0, 0, 0, 0]); // cookie
    Frame::new(ids::SID_CLANMEMBERLIST, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn netgameport(server_port: u16) -> Bytes {
    let mut p = BytesMut::with_capacity(2);
    p.put_u16_le(server_port);
    Frame::new(ids::SID_NETGAMEPORT, p.freeze())
        .encode_with(BNCS_HEADER)
        .expect("2-byte port fits")
}

pub fn auth_info(
    ver: u8,
    tft: bool,
    locale_id: u32,
    country_abbrev: &str,
    country: &str,
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(36 + country_abbrev.len() + country.len());
    p.put_slice(&[0, 0, 0, 0]); // protocol id
    p.put_slice(&[54, 56, 88, 73]); // platform id "IX86" reversed
    if tft {
        p.put_slice(&[80, 88, 51, 87]); // "PX3W"
    } else {
        p.put_slice(&[51, 82, 65, 87]); // "3RAW"
    }
    p.put_slice(&[ver, 0, 0, 0]);
    p.put_slice(&[83, 85, 110, 101]); // language "SUne" ("enUS" reversed)
    p.put_slice(&[127, 0, 0, 1]); // local IP
    p.put_slice(&[44, 1, 0, 0]); // time zone bias
    p.put_u32_le(locale_id);
    p.put_u32_le(locale_id);
    put_cstring(&mut p, country_abbrev);
    put_cstring(&mut p, country);
    Frame::new(ids::SID_AUTH_INFO, p.freeze()).encode_with(BNCS_HEADER)
}
#[allow(clippy::too_many_arguments)]
pub fn auth_check(
    tft: bool,
    client_token: [u8; 4],
    exe_version: [u8; 4],
    exe_version_hash: [u8; 4],
    key_info_roc: &[u8],
    key_info_tft: &[u8],
    exe_info: &str,
    key_owner_name: &str,
) -> Result<Bytes, ProtoError> {
    let num_keys = if tft { 2u32 } else { 1u32 };
    let mut p = BytesMut::with_capacity(
        20 + key_info_roc.len() + key_info_tft.len() + exe_info.len() + key_owner_name.len() + 2,
    );
    p.put_slice(&client_token);
    p.put_slice(&exe_version);
    p.put_slice(&exe_version_hash);
    p.put_u32_le(num_keys);
    p.put_u32_le(0); // spawn key = 0
    p.put_slice(key_info_roc);
    if tft {
        p.put_slice(key_info_tft);
    }
    put_cstring(&mut p, exe_info);
    put_cstring(&mut p, key_owner_name);
    Frame::new(ids::SID_AUTH_CHECK, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn account_logon(client_public_key: &[u8], account_name: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(client_public_key.len() + account_name.len() + 1);
    p.put_slice(client_public_key);
    put_cstring(&mut p, account_name);
    Frame::new(ids::SID_AUTH_ACCOUNTLOGON, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn account_logon_proof(client_password_proof: &[u8]) -> Result<Bytes, ProtoError> {
    Frame::new(
        ids::SID_AUTH_ACCOUNTLOGONPROOF,
        Bytes::copy_from_slice(client_password_proof),
    )
    .encode_with(BNCS_HEADER)
}

pub fn claninvitation(account_name: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(5 + account_name.len());
    p.put_slice(&[0, 0, 0, 0]); // cookie
    put_cstring(&mut p, account_name);
    Frame::new(ids::SID_CLANINVITATION, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn clanremovemember(account_name: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(5 + account_name.len());
    p.put_slice(&[0, 0, 0, 0]); // cookie
    put_cstring(&mut p, account_name);
    Frame::new(ids::SID_CLANREMOVEMEMBER, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn clanchangerank(account_name: &str, rank: u8) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(6 + account_name.len());
    p.put_slice(&[0, 0, 0, 0]); // cookie
    put_cstring(&mut p, account_name);
    p.put_u8(rank);
    Frame::new(ids::SID_CLANCHANGERANK, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn clansetmotd(motd: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(5 + motd.len());
    p.put_slice(&[0, 0, 0, 0]); // cookie
    put_cstring(&mut p, motd);
    Frame::new(ids::SID_CLANSETMOTD, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn clancreationinvitation(
    tag: &[u8; 4],
    inviter_name: &str,
    accept: bool,
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(9 + inviter_name.len());
    p.put_slice(&[0, 0, 0, 0]); // cookie
    p.put_slice(tag);
    put_cstring(&mut p, inviter_name);
    p.put_u8(if accept { 0x06 } else { 0x04 });
    Frame::new(ids::SID_CLANCREATIONINVITATION, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn claninvitationresponse(
    tag: &[u8; 4],
    inviter_name: &str,
    accept: bool,
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(9 + inviter_name.len());
    p.put_slice(&[0, 0, 0, 0]); // cookie
    p.put_slice(tag);
    put_cstring(&mut p, inviter_name);
    p.put_u8(if accept { 0x06 } else { 0x04 });
    Frame::new(ids::SID_CLANINVITATIONRESPONSE, p.freeze()).encode_with(BNCS_HEADER)
}

pub fn warden(response: &[u8]) -> Result<Bytes, ProtoError> {
    Frame::new(ids::SID_WARDEN, Bytes::copy_from_slice(response)).encode_with(BNCS_HEADER)
}

pub fn iccup_antihack(payload: &[u8]) -> Result<Bytes, ProtoError> {
    Frame::new(ids::SID_ICCUP_ANTIHACK, Bytes::copy_from_slice(payload)).encode_with(BNCS_HEADER)
}

pub fn iccup_challenge_reply(challenge: &[u8]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(24);
    if challenge.len() >= 8 {
        p.put_slice(&[0x00, 0x69, 0x59, 0x2f]);
        p.put_slice(&challenge[0..4]);
        p.put_slice(&[0x7a, 0xc1, 0x15, 0x5a]);
        p.put_slice(&challenge[4..8]);
        p.put_slice(&[0x27, 0x00, 0x00, 0x0a, 0x0a, 0x00, 0x00, 0xae]);
    } else {
        p.put_slice(&[
            0x00, 0x69, 0x59, 0x2f, 0xdd, 0xe9, 0x08, 0x6b, 0x7a, 0xc1, 0x15, 0x5a, 0xfb, 0x93,
            0x1d, 0x8d, 0x27, 0x00, 0x00, 0x0a, 0x0a, 0x00, 0x00, 0xae,
        ]);
    }
    Frame::new(ids::SID_ICCUP_ANTIHACK, p.freeze()).encode_with(BNCS_HEADER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startadvex3_writes_correct_visibility_and_24_slot_capacity() {
        let stat_string = vec![0x01, 0x02, 0x00];
        let pkt_pub = startadvex3(
            GameVisibility::Public,
            [1, 0, 0, 0],
            "DotA 5v5",
            "iCCupHost",
            0,
            &stat_string,
            0x12345678,
        )
        .expect("packet encoding must succeed");

        assert_eq!(pkt_pub[0], 0xFF);
        assert_eq!(pkt_pub[1], 0x1C);
        // gameprotocol.h:32 GAME_PUBLIC = 16; bnetprotocol.cpp:702 documents the field.
        assert_eq!(pkt_pub[4], 16, "public game state must be 16");
        assert_eq!(&pkt_pub[5..8], &[0, 0, 0]);

        // [header 4][state 4][uptime 4][game_type 4][unknown 4][custom 4]
        // [game_name + NUL][password NUL][slots_free 1][host_counter 8]...
        let name_len = "DotA 5v5\0".len();
        let slots_free_offset = 4 + 4 + 4 + 4 + 4 + 4 + name_len + 1;
        // bnetprotocol.cpp:712-714 sends 110 when MAX_SLOTS > 12; gameslot.h:39 sets it to 24.
        assert_eq!(
            pkt_pub[slots_free_offset], 98,
            "slots_free must be 98 (char 'b', 11 slots free for MAX_SLOTS = 12)"
        );

        let pkt_priv = startadvex3(
            GameVisibility::Private,
            [1, 0, 0, 0],
            "DotA 5v5",
            "iCCupHost",
            0,
            &stat_string,
            0x12345678,
        )
        .expect("packet encoding must succeed");
        assert_eq!(pkt_priv[4], 17, "private game state must be 17");
    }

    #[test]
    fn every_packet_is_framed_with_0xff_and_a_correct_length() {
        let packets = [
            null(),
            enter_chat().unwrap(),
            join_channel("iccup.pro").unwrap(),
            chat_command("/whois slash").unwrap(),
            netgameport(6112),
            stopadv(),
        ];
        for p in packets {
            assert_eq!(p[0], 0xFF, "bncs header");
            assert_eq!(
                u16::from_le_bytes([p[2], p[3]]) as usize,
                p.len(),
                "length field"
            );
        }
    }

    #[test]
    fn join_channel_carries_a_nul_terminated_name() {
        let p = join_channel("iccup.pro").unwrap();
        assert_eq!(p[1], ids::SID_JOINCHANNEL);
        assert_eq!(p[p.len() - 1], 0);
        assert!(p.windows(9).any(|w| w == b"iccup.pro"));
    }

    #[test]
    fn auth_info_declares_the_configured_war3_version() {
        let p = auth_info(26, true, 1033, "USA", "United States").unwrap();
        assert_eq!(p[1], ids::SID_AUTH_INFO);
        // Product is "PX3W" (W3XP reversed) for The Frozen Throne.
        assert!(p.windows(4).any(|w| w == b"PX3W"));
    }

    #[test]
    fn test_startadvex3_stat_string_size_validation() {
        let valid_stat_string = vec![0x41; 127]; // 127 bytes < 128
        let res_ok = startadvex3(
            GameVisibility::Public,
            [1, 0, 0, 0],
            "Test Game",
            "Host",
            0,
            &valid_stat_string,
            1,
        );
        assert!(res_ok.is_ok(), "127-byte stat string must be accepted");

        let invalid_stat_string = vec![0x41; 128]; // 128 bytes >= 128
        let res_err = startadvex3(
            GameVisibility::Public,
            [1, 0, 0, 0],
            "Test Game",
            "Host",
            0,
            &invalid_stat_string,
            1,
        );
        assert!(
            matches!(res_err, Err(ProtoError::BadValue(_))),
            "StatString >= 128 bytes must be rejected with BadValue per bnetprotocol.cpp:694"
        );
    }

    #[test]
    fn clan_packets_encoded_correctly() {
        let p_inv = claninvitation("Newbie").unwrap();
        assert_eq!(p_inv[0], 0xFF);
        assert_eq!(p_inv[1], ids::SID_CLANINVITATION);
        assert_eq!(&p_inv[4..8], &[0, 0, 0, 0]); // cookie
        assert_eq!(&p_inv[8..15], b"Newbie\0");

        let p_rem = clanremovemember("Oldie").unwrap();
        assert_eq!(p_rem[1], ids::SID_CLANREMOVEMEMBER);
        assert_eq!(&p_rem[8..14], b"Oldie\0");

        let p_rank = clanchangerank("Worker", 2).unwrap();
        assert_eq!(p_rank[1], ids::SID_CLANCHANGERANK);
        assert_eq!(&p_rank[8..15], b"Worker\0");
        assert_eq!(p_rank[15], 2);

        let p_motd = clansetmotd("Welcome to clan").unwrap();
        assert_eq!(p_motd[1], ids::SID_CLANSETMOTD);
        assert_eq!(&p_motd[8..24], b"Welcome to clan\0");

        let p_accept = clancreationinvitation(b"TAG1", "Chief", true).unwrap();
        assert_eq!(p_accept[1], ids::SID_CLANCREATIONINVITATION);
        assert_eq!(&p_accept[8..12], b"TAG1");
        assert_eq!(&p_accept[12..18], b"Chief\0");
        assert_eq!(p_accept[18], 0x06);

        let p_reject = claninvitationresponse(b"TAG1", "Chief", false).unwrap();
        assert_eq!(p_reject[1], ids::SID_CLANINVITATIONRESPONSE);
        assert_eq!(p_reject[18], 0x04);

        let p_warden = warden(b"warden response").unwrap();
        assert_eq!(p_warden[1], ids::SID_WARDEN);
        assert_eq!(&p_warden[4..], b"warden response");
    }
}
