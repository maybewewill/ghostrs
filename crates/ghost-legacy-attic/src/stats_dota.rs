use std::collections::HashMap;
use crate::logger::*;
use crate::util::byte_array_to_uint32;

#[derive(Debug, Clone, Default)]
pub struct DotAPlayerStats {
    pub colour: u32,
    pub new_colour: u32,
    pub name: String,
    pub hero: String,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_kills: u32,
    pub creep_denies: u32,
    pub neutral_kills: u32,
    pub gold: u32,
    pub items: [String; 6],
    pub courier_kills: u32,
    pub tower_kills: u32,
    pub rax_kills: u32,
}

impl DotAPlayerStats {
    pub fn new(colour: u32) -> Self {
        Self {
            colour,
            new_colour: colour,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StatsDotA {
    pub players: HashMap<u32, DotAPlayerStats>,
    pub winner: u32, // 0 = unknown/unfinished, 1 = Sentinel, 2 = Scourge
    pub duration_min: u32,
    pub duration_sec: u32,
    pub tree_hp: u32,
    pub throne_hp: u32,
    pub game_name: String,
}

impl StatsDotA {
    pub fn new(game_name: String) -> Self {
        Self {
            game_name,
            ..Default::default()
        }
    }

    /// Process incoming action bytes to extract "kdr.x" DotA metadata
    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        let mut i = 0;
        let sig = [0x6b, 0x64, 0x72, 0x2e, 0x78, 0x00]; // "kdr.x\0"

        while action_data.len() >= i + 6 {
            if action_data[i..i+6] == sig {
                // Found potential DotA real-time replay metadata
                let pos = i + 6;
                if let Some((data_str, next_pos)) = Self::extract_c_string(action_data, pos) {
                    if let Some((key_str, next_pos2)) = Self::extract_c_string(action_data, next_pos) {
                        if action_data.len() >= next_pos2 + 4 {
                            let value_bytes = &action_data[next_pos2..next_pos2+4];
                            let value_int = byte_array_to_uint32(&value_bytes.to_vec(), false, 0);

                            self.handle_key_value(&data_str, &key_str, value_int, value_bytes);
                            i = next_pos2 + 4;
                            continue;
                        }
                    }
                }
            }
            i += 1;
        }

        self.winner != 0
    }

    fn extract_c_string(data: &[u8], start: usize) -> Option<(String, usize)> {
        if start >= data.len() {
            return None;
        }
        if let Some(nul_pos) = data[start..].iter().position(|&b| b == 0) {
            let str_slice = &data[start..start + nul_pos];
            let string_val = String::from_utf8_lossy(str_slice).to_string();
            Some((string_val, start + nul_pos + 1))
        } else {
            None
        }
    }

    fn handle_key_value(&mut self, data: &str, key: &str, value_int: u32, value_raw: &[u8]) {
        match data {
            "Data" => {
                // In-game live events
                if key.starts_with("Hero") {
                    let victim_col_str = &key[4..];
                    if let Ok(victim_col) = victim_col_str.parse::<u32>() {
                        log_info(&format!("[STATSDOTA: {}] Hero death: killer_color={}, victim_color={}", self.game_name, value_int, victim_col));
                    }
                } else if key.starts_with("Courier") {
                    if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                        let p = self.players.entry(value_int).or_insert_with(|| DotAPlayerStats::new(value_int));
                        p.courier_kills += 1;
                    }
                } else if key.starts_with("Tower") {
                    if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                        let p = self.players.entry(value_int).or_insert_with(|| DotAPlayerStats::new(value_int));
                        p.tower_kills += 1;
                    }
                } else if key.starts_with("Rax") {
                    if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                        let p = self.players.entry(value_int).or_insert_with(|| DotAPlayerStats::new(value_int));
                        p.rax_kills += 1;
                    }
                } else if key == "Throne" {
                    self.throne_hp = value_int;
                    log_info(&format!("[STATSDOTA: {}] Frozen Throne HP: {}%", self.game_name, value_int));
                } else if key == "Tree" {
                    self.tree_hp = value_int;
                    log_info(&format!("[STATSDOTA: {}] World Tree HP: {}%", self.game_name, value_int));
                }
            }
            "Global" => {
                // End-game summary
                if key == "Winner" {
                    self.winner = value_int;
                    let winner_name = match value_int {
                        1 => "Sentinel",
                        2 => "Scourge",
                        _ => "Unknown",
                    };
                    log_info(&format!("[STATSDOTA: {}] Match Winner: {}", self.game_name, winner_name));
                } else if key == "m" {
                    self.duration_min = value_int;
                } else if key == "s" {
                    self.duration_sec = value_int;
                }
            }
            _ => {
                // Player ID stats (keys 1-5 for Sentinel, 7-11 for Scourge)
                if let Ok(player_id) = data.parse::<u32>() {
                    if (1..=5).contains(&player_id) || (7..=11).contains(&player_id) {
                        let p = self.players.entry(player_id).or_insert_with(|| DotAPlayerStats::new(player_id));
                        match key {
                            "1" => p.kills = value_int,
                            "2" => p.deaths = value_int,
                            "3" => p.creep_kills = value_int,
                            "4" => p.creep_denies = value_int,
                            "5" => p.assists = value_int,
                            "6" => p.gold = value_int,
                            "7" => p.neutral_kills = value_int,
                            "8_0" => p.items[0] = Self::format_rawcode(value_raw),
                            "8_1" => p.items[1] = Self::format_rawcode(value_raw),
                            "8_2" => p.items[2] = Self::format_rawcode(value_raw),
                            "8_3" => p.items[3] = Self::format_rawcode(value_raw),
                            "8_4" => p.items[4] = Self::format_rawcode(value_raw),
                            "8_5" => p.items[5] = Self::format_rawcode(value_raw),
                            "9" => p.hero = Self::format_rawcode(value_raw),
                            "id" => {
                                if value_int >= 6 {
                                    p.new_colour = value_int + 1;
                                } else {
                                    p.new_colour = value_int;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    fn format_rawcode(raw: &[u8]) -> String {
        let mut reversed = raw.to_vec();
        reversed.reverse();
        String::from_utf8_lossy(&reversed).to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dota_stats_winner_and_kills() {
        let mut stats = StatsDotA::new("TestDotA".to_string());

        // Build "kdr.x\0Global\0Winner\0[1,0,0,0]" -> Winner: 1 (Sentinel)
        let mut packet = vec![0x6b, 0x64, 0x72, 0x2e, 0x78, 0x00];
        packet.extend_from_slice(b"Global\0");
        packet.extend_from_slice(b"Winner\0");
        packet.extend_from_slice(&1u32.to_le_bytes());

        let finished = stats.process_action(&packet);
        assert!(finished);
        assert_eq!(stats.winner, 1);

        // Build "kdr.x\01\01\0[15,0,0,0]" -> Player 1 (Sentinel Player 1), Kills: 15
        let mut p_packet = vec![0x6b, 0x64, 0x72, 0x2e, 0x78, 0x00];
        p_packet.extend_from_slice(b"1\0");
        p_packet.extend_from_slice(b"1\0");
        p_packet.extend_from_slice(&15u32.to_le_bytes());

        stats.process_action(&p_packet);
        assert_eq!(stats.players.get(&1).unwrap().kills, 15);
    }
}
