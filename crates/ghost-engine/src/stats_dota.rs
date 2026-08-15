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

    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        if action_data.len() < 4 {
            return false;
        }

        // Check for DotA stats markers embedded in custom script action packets
        let mut i = 0;
        while i + 8 <= action_data.len() {
            // Check for game over or objective signatures
            if &action_data[i..i + 4] == b"TheT" { // Frozen Throne damaged
                if i + 8 <= action_data.len() {
                    let hp = u32::from_le_bytes([action_data[i+4], action_data[i+5], action_data[i+6], action_data[i+7]]);
                    self.throne_hp = hp.min(100);
                    if self.throne_hp == 0 {
                        self.winner = 1; // Sentinel victory
                    }
                }
                i += 8;
                continue;
            } else if &action_data[i..i + 4] == b"WorT" { // World Tree damaged
                if i + 8 <= action_data.len() {
                    let hp = u32::from_le_bytes([action_data[i+4], action_data[i+5], action_data[i+6], action_data[i+7]]);
                    self.tree_hp = hp.min(100);
                    if self.tree_hp == 0 {
                        self.winner = 2; // Scourge victory
                    }
                }
                i += 8;
                continue;
            } else if &action_data[i..i + 4] == b"Hero" { // Hero selection
                if i + 12 <= action_data.len() {
                    let colour = u32::from_le_bytes([action_data[i+4], action_data[i+5], action_data[i+6], action_data[i+7]]);
                    let hero_code = String::from_utf8_lossy(&action_data[i+8..i+12]).to_string();
                    if let Some(p) = self.players.get_mut(&colour) {
                        p.hero = hero_code;
                    }
                    i += 12;
                    continue;
                }
            } else if &action_data[i..i + 4] == b"Kill" { // Kill event
                if i + 12 <= action_data.len() {
                    let killer_col = u32::from_le_bytes([action_data[i+4], action_data[i+5], action_data[i+6], action_data[i+7]]);
                    let victim_col = u32::from_le_bytes([action_data[i+8], action_data[i+9], action_data[i+10], action_data[i+11]]);
                    if let Some(killer) = self.players.get_mut(&killer_col) {
                        killer.kills += 1;
                    }
                    if let Some(victim) = self.players.get_mut(&victim_col) {
                        victim.deaths += 1;
                    }
                    i += 12;
                    continue;
                }
            }
            i += 1;
        }

        true
    }

    pub fn format_player_stats(&self, name: &str) -> Option<String> {
        let p = self.players.values().find(|p| p.name.eq_ignore_ascii_case(name))?;
        let hero = if p.hero.is_empty() { "None" } else { &p.hero };
        Some(format!(
            "[{}] Hero: {}, K/D/A: {}/{}/{}, CS: {}/{}, Neutrals: {}, Gold: {}",
            p.name, hero, p.kills, p.deaths, p.assists, p.creep_kills, p.creep_denies, p.neutral_kills, p.gold
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

    #[test]
    fn processes_dota_hero_selection_and_kills() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        dota.add_player(1, "PlayerOne".into());
        dota.add_player(7, "PlayerTwo".into());

        // Hero selection action
        let mut hero_act = Vec::new();
        hero_act.extend_from_slice(b"Hero");
        hero_act.extend_from_slice(&1u32.to_le_bytes());
        hero_act.extend_from_slice(b"E001");
        dota.process_action(&hero_act);

        assert_eq!(dota.players.get(&1).unwrap().hero, "E001");

        // Kill event
        let mut kill_act = Vec::new();
        kill_act.extend_from_slice(b"Kill");
        kill_act.extend_from_slice(&1u32.to_le_bytes()); // killer = 1
        kill_act.extend_from_slice(&7u32.to_le_bytes()); // victim = 7
        dota.process_action(&kill_act);

        let p1 = dota.players.get(&1).unwrap();
        let p2 = dota.players.get(&7).unwrap();
        assert_eq!(p1.kills, 1);
        assert_eq!(p2.deaths, 1);
    }

    #[test]
    fn throne_destruction_detects_sentinel_win() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        let mut throne_act = Vec::new();
        throne_act.extend_from_slice(b"TheT");
        throne_act.extend_from_slice(&0u32.to_le_bytes()); // 0 HP
        dota.process_action(&throne_act);

        assert_eq!(dota.winner, 1);
        assert_eq!(dota.format_winner(), "Sentinel");
    }
}
