use std::collections::HashMap;

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
    /// 0 = unknown/unfinished, 1 = Sentinel, 2 = Scourge
    pub winner: u32,
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
            tree_hp: 100,
            throne_hp: 100,
            ..Default::default()
        }
    }

    pub fn add_player(&mut self, colour: u32, name: String) {
        let mut p = DotAPlayerStats::new(colour);
        p.name = name;
        self.players.insert(colour, p);
    }

    /// Parses DotA real-time replay data actions.
    /// Equivalent to GHost++ `CStatsDOTA::ProcessAction` (statsdota.cpp:51-371).
    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        let mut i = 0;
        let dota_sig = [0x6b, b'd', b'r', b'.', b'x', 0x00];

        while i + 6 <= action_data.len() {
            if action_data[i..i + 6] == dota_sig {
                let start = i + 6;
                // Extract null-terminated Data string
                let Some(data_null) = action_data[start..].iter().position(|&b| b == 0) else {
                    i += 1;
                    continue;
                };
                let data_bytes = &action_data[start..start + data_null];
                let key_start = start + data_null + 1;

                // Extract null-terminated Key string
                let Some(key_null) = action_data[key_start..].iter().position(|&b| b == 0) else {
                    i += 1;
                    continue;
                };
                let key_bytes = &action_data[key_start..key_start + key_null];
                let val_start = key_start + key_null + 1;

                if val_start + 4 > action_data.len() {
                    i += 1;
                    continue;
                }

                let value_int = u32::from_le_bytes([
                    action_data[val_start],
                    action_data[val_start + 1],
                    action_data[val_start + 2],
                    action_data[val_start + 3],
                ]);
                let value_raw = &action_data[val_start..val_start + 4];

                let data_str = String::from_utf8_lossy(data_bytes);
                let key_str = String::from_utf8_lossy(key_bytes);

                if data_str == "Data" {
                    if key_str.starts_with("Courier") {
                        if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                            self.players
                                .entry(value_int)
                                .or_insert_with(|| DotAPlayerStats::new(value_int))
                                .courier_kills += 1;
                        }
                    } else if key_str.starts_with("Tower") {
                        if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                            self.players
                                .entry(value_int)
                                .or_insert_with(|| DotAPlayerStats::new(value_int))
                                .tower_kills += 1;
                        }
                    } else if key_str.starts_with("Rax") {
                        if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                            self.players
                                .entry(value_int)
                                .or_insert_with(|| DotAPlayerStats::new(value_int))
                                .rax_kills += 1;
                        }
                    } else if key_str.starts_with("Throne") {
                        self.throne_hp = value_int.min(100);
                    } else if key_str.starts_with("Tree") {
                        self.tree_hp = value_int.min(100);
                    }
                } else if data_str == "Global" {
                    if key_str == "Winner" {
                        self.winner = value_int; // 1 = Sentinel, 2 = Scourge (statsdota.cpp:271)
                    } else if key_str == "m" {
                        self.duration_min = value_int;
                    } else if key_str == "s" {
                        self.duration_sec = value_int;
                    }
                } else if let Ok(id) = data_str.parse::<u32>()
                    && ((1..=5).contains(&id) || (7..=11).contains(&id))
                {
                    let p = self
                        .players
                        .entry(id)
                        .or_insert_with(|| DotAPlayerStats::new(id));
                    match key_str.as_ref() {
                        "1" => p.kills = value_int,
                        "2" => p.deaths = value_int,
                        "3" => p.creep_kills = value_int,
                        "4" => p.creep_denies = value_int,
                        "5" => p.assists = value_int,
                        "6" => p.gold = value_int,
                        "7" => p.neutral_kills = value_int,
                        "8_0" => {
                            p.items[0] = String::from_utf8_lossy(&[
                                value_raw[3],
                                value_raw[2],
                                value_raw[1],
                                value_raw[0],
                            ])
                            .to_string()
                        }
                        "8_1" => {
                            p.items[1] = String::from_utf8_lossy(&[
                                value_raw[3],
                                value_raw[2],
                                value_raw[1],
                                value_raw[0],
                            ])
                            .to_string()
                        }
                        "8_2" => {
                            p.items[2] = String::from_utf8_lossy(&[
                                value_raw[3],
                                value_raw[2],
                                value_raw[1],
                                value_raw[0],
                            ])
                            .to_string()
                        }
                        "8_3" => {
                            p.items[3] = String::from_utf8_lossy(&[
                                value_raw[3],
                                value_raw[2],
                                value_raw[1],
                                value_raw[0],
                            ])
                            .to_string()
                        }
                        "8_4" => {
                            p.items[4] = String::from_utf8_lossy(&[
                                value_raw[3],
                                value_raw[2],
                                value_raw[1],
                                value_raw[0],
                            ])
                            .to_string()
                        }
                        "8_5" => {
                            p.items[5] = String::from_utf8_lossy(&[
                                value_raw[3],
                                value_raw[2],
                                value_raw[1],
                                value_raw[0],
                            ])
                            .to_string()
                        }
                        "9" => {
                            p.hero = String::from_utf8_lossy(&[
                                value_raw[3],
                                value_raw[2],
                                value_raw[1],
                                value_raw[0],
                            ])
                            .to_string()
                        }
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

                i = val_start + 4;
            } else {
                i += 1;
            }
        }

        self.winner != 0
    }

    pub fn format_player_stats(&self, name: &str) -> Option<String> {
        let p = self
            .players
            .values()
            .find(|p| p.name.eq_ignore_ascii_case(name))?;
        let hero = if p.hero.is_empty() { "None" } else { &p.hero };
        Some(format!(
            "[{}] Hero: {}, K/D/A: {}/{}/{}, CS: {}/{}, Neutrals: {}, Gold: {}",
            p.name,
            hero,
            p.kills,
            p.deaths,
            p.assists,
            p.creep_kills,
            p.creep_denies,
            p.neutral_kills,
            p.gold
        ))
    }

    pub fn format_winner(&self) -> &'static str {
        match self.winner {
            1 => "Sentinel",
            2 => "Scourge",
            _ => "Unfinished",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dr_x_action(data: &str, key: &str, value: u32) -> Vec<u8> {
        let mut pkt = Vec::new();
        // DotA custom action marker: 0x6b "dr.x\0" (statsdota.cpp:67)
        pkt.extend_from_slice(&[0x6b, b'd', b'r', b'.', b'x', 0x00]);
        pkt.extend_from_slice(data.as_bytes());
        pkt.push(0x00);
        pkt.extend_from_slice(key.as_bytes());
        pkt.push(0x00);
        pkt.extend_from_slice(&value.to_le_bytes());
        pkt
    }

    #[test]
    fn parses_real_dota_winner_and_duration_from_global_stream() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        dota.add_player(1, "PlayerOne".into());
        dota.add_player(7, "PlayerTwo".into());

        // Winner event: Data="Global", Key="Winner", Value=1 (Sentinel)
        let winner_act = make_dr_x_action("Global", "Winner", 1);
        let finished = dota.process_action(&winner_act);
        assert!(
            finished,
            "process_action must return true when winner is set"
        );
        assert_eq!(dota.winner, 1);
        assert_eq!(dota.format_winner(), "Sentinel");

        // Duration: Data="Global", Key="m", Value=42; Key="s", Value=15
        dota.process_action(&make_dr_x_action("Global", "m", 42));
        dota.process_action(&make_dr_x_action("Global", "s", 15));
        assert_eq!(dota.duration_min, 42);
        assert_eq!(dota.duration_sec, 15);
    }

    #[test]
    fn parses_end_game_player_kda_and_item_records() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        dota.add_player(1, "Alice".into());

        // Player "1" stats: Kills=12, Deaths=3, Creeps=145, Denies=18, Assists=7, Gold=2400
        dota.process_action(&make_dr_x_action("1", "1", 12));
        dota.process_action(&make_dr_x_action("1", "2", 3));
        dota.process_action(&make_dr_x_action("1", "3", 145));
        dota.process_action(&make_dr_x_action("1", "4", 18));
        dota.process_action(&make_dr_x_action("1", "5", 7));
        dota.process_action(&make_dr_x_action("1", "6", 2400));
        // Item 0: "I001" (stored reversed on wire)
        let item_val = u32::from_le_bytes([b'1', b'0', b'0', b'I']);
        dota.process_action(&make_dr_x_action("1", "8_0", item_val));

        let p = dota.players.get(&1).expect("player 1 must exist");
        assert_eq!(p.kills, 12);
        assert_eq!(p.deaths, 3);
        assert_eq!(p.creep_kills, 145);
        assert_eq!(p.creep_denies, 18);
        assert_eq!(p.assists, 7);
        assert_eq!(p.gold, 2400);
        assert_eq!(p.items[0], "I001");
    }

    #[test]
    fn parses_in_game_tower_and_rax_destruction_events() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        dota.add_player(1, "Alice".into());

        // In-game Data="Data", Key="Tower010" (Alliance 0=Sentinel, Level 1, Side 0=top), Value=1 (Player 1 destroyed it)
        dota.process_action(&make_dr_x_action("Data", "Tower010", 1));
        let p = dota.players.get(&1).unwrap();
        assert_eq!(p.tower_kills, 1);
    }
}
