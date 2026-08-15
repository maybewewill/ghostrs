use bytes::{Bytes, BytesMut};
use ghost_protocol::w3gs::ActionBlock;

/// HCL (Hostbot Command Line) utility for setting game modes in Warcraft 3 maps.
pub struct Hcl;

impl Hcl {
    /// Extracts game mode (e.g. "-apem", "-sd", "-arso") from game names like "DotA -apem 5v5".
    pub fn parse_from_gamename(game_name: &str) -> Option<String> {
        for word in game_name.split_whitespace() {
            if let Some(mode) = word.strip_prefix('-')
                && !mode.is_empty()
                && mode.chars().all(|c| c.is_alphanumeric())
            {
                return Some(mode.to_lowercase());
            }
        }
        None
    }

    /// Encodes HCL string into standard Warcraft 3 action block format (W3GS action opcode 0x61 or chat).
    pub fn encode_hcl_actions(hcl: &str, pid: u8) -> Vec<ActionBlock> {
        let mut blocks = Vec::new();
        for ch in hcl.chars() {
            let mut data = BytesMut::with_capacity(3);
            data.extend_from_slice(&[0x61, ch as u8, 0x00]);
            blocks.push(ActionBlock {
                pid,
                data: data.freeze(),
            });
        }
        blocks
    }

    /// Encodes HCL as map check handoff string.
    pub fn format_hcl_string(hcl: &str) -> Bytes {
        let mut b = BytesMut::new();
        b.extend_from_slice(hcl.as_bytes());
        b.freeze()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mode_from_game_name() {
        assert_eq!(Hcl::parse_from_gamename("DotA -apem 5v5"), Some("apem".into()));
        assert_eq!(Hcl::parse_from_gamename("Legion TD -arso pro"), Some("arso".into()));
        assert_eq!(Hcl::parse_from_gamename("Castle Fight 3v3"), None);
    }

    #[test]
    fn encodes_hcl_characters_into_actions() {
        let actions = Hcl::encode_hcl_actions("ap", 1);
        assert_eq!(actions.len(), 2);
        assert_eq!(&actions[0].data[..], &[0x61, b'a', 0x00]);
        assert_eq!(&actions[1].data[..], &[0x61, b'p', 0x00]);
    }
}
