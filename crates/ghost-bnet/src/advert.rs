use ghost_protocol::encode_statstring;

#[derive(Debug, Clone)]
pub struct MapAdvert {
    pub path: String,
    pub size: u32,
    pub info: u32,
    pub crc: u32,
    pub sha1: [u8; 20],
    pub num_players: u8,
    pub num_teams: u8,
    pub width: u16,
    pub height: u16,
    pub game_type: u32,
    pub flags: u32,
}

/// Encodes stat string for Battle.net (BNCS SID_STARTADVEX3), which includes the 20-byte map SHA1 at the end.
pub fn encode_bnet_statstring(map: &MapAdvert, _game_name: &str, host_name: &str) -> Vec<u8> {
    let mut raw = Vec::with_capacity(64 + map.path.len() + host_name.len());
    raw.extend_from_slice(&map.flags.to_le_bytes());
    raw.push(0);
    raw.extend_from_slice(&map.width.to_le_bytes());
    raw.extend_from_slice(&map.height.to_le_bytes());
    raw.extend_from_slice(&map.crc.to_le_bytes());
    raw.extend_from_slice(map.path.as_bytes());
    raw.push(0);
    raw.extend_from_slice(host_name.as_bytes());
    raw.push(0);
    raw.push(0);
    raw.extend_from_slice(&map.sha1);
    encode_statstring(&raw)
}

/// Encodes stat string for LAN (W3GS_GAMEINFO), which does NOT include the map SHA1.
pub fn encode_lan_statstring(map: &MapAdvert, _game_name: &str, host_name: &str) -> Vec<u8> {
    let mut raw = Vec::with_capacity(44 + map.path.len() + host_name.len());
    raw.extend_from_slice(&map.flags.to_le_bytes());
    raw.push(0);
    raw.extend_from_slice(&map.width.to_le_bytes());
    raw.extend_from_slice(&map.height.to_le_bytes());
    raw.extend_from_slice(&map.crc.to_le_bytes());
    raw.extend_from_slice(map.path.as_bytes());
    raw.push(0);
    raw.extend_from_slice(host_name.as_bytes());
    raw.push(0);
    raw.push(0);
    encode_statstring(&raw)
}


#[cfg(test)]
mod tests {
    use super::*;
    use ghost_protocol::decode_statstring;

    #[test]
    fn test_bnet_and_lan_statstring_parity() {
        let sha1 = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        ];
        let m = MapAdvert {
            path: "Maps\\Download\\dota.w3x".into(),
            size: 1234,
            info: 1,
            crc: 0x1234_5678,
            sha1,
            num_players: 10,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0x0006_4802,
        };

        let bnet_enc = encode_bnet_statstring(&m, "DotA", "Ghost");
        let lan_enc = encode_lan_statstring(&m, "DotA", "Ghost");

        assert!(!bnet_enc.is_empty());
        assert!(!lan_enc.is_empty());
        assert!(!bnet_enc.contains(&0));
        assert!(!lan_enc.contains(&0));

        // LAN statstring must be strictly smaller than BNCS statstring by the encoded SHA1 size
        assert!(
            lan_enc.len() < bnet_enc.len(),
            "lan statstring len {} must be < bnet statstring len {}",
            lan_enc.len(),
            bnet_enc.len()
        );

        let bnet_dec = decode_statstring(&bnet_enc);
        let lan_dec = decode_statstring(&lan_enc);

        // BNCS raw payload: 4(flags) + 1(0) + 2(w) + 2(h) + 4(crc) + path + 1(0) + host + 1(0) + 1(0) + 20(sha1)
        // LAN raw payload: 4(flags) + 1(0) + 2(w) + 2(h) + 4(crc) + path + 1(0) + host + 1(0) + 1(0)
        assert_eq!(lan_dec.len() + 20, bnet_dec.len());
        assert_eq!(&bnet_dec[..lan_dec.len()], &lan_dec[..]);
        assert_eq!(&bnet_dec[lan_dec.len()..], &sha1);
    }
}

