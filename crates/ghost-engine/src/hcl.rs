use bytes::{Bytes, BytesMut};
use ghost_protocol::w3gs::SlotInfo;

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

    /// Encodes HCL command string into slot handicaps matching GHost++ `CBaseGame::EventCountDownStart` (game_base.cpp:3326-3367).
    pub fn encode_hcl_into_slots(hcl: &str, slots: &mut [SlotInfo]) -> bool {
        let occupied_count = slots.iter().filter(|s| s.slot_status == 2).count();
        if hcl.is_empty() || hcl.len() > occupied_count {
            return false;
        }
        let hcl_chars = "abcdefghijklmnopqrstuvwxyz0123456789 -=,.";
        if !hcl.chars().all(|c| hcl_chars.contains(c)) {
            return false;
        }

        let mut encoding_map = [0u8; 256];
        let mut j: u8 = 0;
        for slot in &mut encoding_map {
            // The following 7 handicap values are forbidden by Warcraft 3
            if j == 0 || j == 50 || j == 60 || j == 70 || j == 80 || j == 90 || j == 100 {
                j = j.wrapping_add(1);
            }
            *slot = j;
            j = j.wrapping_add(1);
        }

        let mut current_slot = 0;
        for ch in hcl.chars() {
            while current_slot < slots.len() && slots[current_slot].slot_status != 2 {
                current_slot += 1;
            }
            if current_slot >= slots.len() {
                break;
            }
            let handicap_index = (slots[current_slot].handicap.saturating_sub(50) / 10) as usize;
            let char_index = hcl_chars.find(ch).unwrap_or(0);
            let map_index = (handicap_index + char_index * 6).min(255);
            slots[current_slot].handicap = encoding_map[map_index];
            current_slot += 1;
        }
        true
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
        assert_eq!(
            Hcl::parse_from_gamename("DotA -apem 5v5"),
            Some("apem".into())
        );
        assert_eq!(
            Hcl::parse_from_gamename("Legion TD -arso pro"),
            Some("arso".into())
        );
        assert_eq!(Hcl::parse_from_gamename("Castle Fight 3v3"), None);
    }

    #[test]
    fn encodes_hcl_characters_into_slot_handicaps() {
        let mut slots = vec![
            SlotInfo {
                pid: 2,
                download_status: 100,
                slot_status: 2, // occupied
                computer: 0,
                team: 0,
                colour: 1,
                race: 0x08,
                computer_type: 1,
                handicap: 100,
            },
            SlotInfo {
                pid: 3,
                download_status: 100,
                slot_status: 2, // occupied
                computer: 0,
                team: 0,
                colour: 2,
                race: 0x08,
                computer_type: 1,
                handicap: 100,
            },
        ];
        let ok = Hcl::encode_hcl_into_slots("ap", &mut slots);
        assert!(ok);
        assert_ne!(slots[0].handicap, 100);
        assert_ne!(slots[1].handicap, 100);
    }
}
