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

pub fn encode_game_statstring(map: &MapAdvert, _game_name: &str, host_name: &str) -> Vec<u8> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statstring_encoding_produces_non_empty_bytes() {
        let m = MapAdvert {
            path: "Maps\\test.w3x".into(),
            size: 1234,
            info: 1,
            crc: 0xDEAD,
            sha1: [0; 20],
            num_players: 10,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0,
        };
        let enc = encode_game_statstring(&m, "DotA", "Ghost");
        assert!(!enc.is_empty());
        assert!(!enc.contains(&0));
    }
}
