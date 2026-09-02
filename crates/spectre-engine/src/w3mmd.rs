use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum MmdValue {
    Number(f64),
    Text(String),
}

#[derive(Debug, Clone, Default)]
pub struct W3Mmd {
    pub initialized: bool,
    pub flags: HashMap<u8, String>,
    pub player_vars: HashMap<(u8, String), MmdValue>,
    pub events: Vec<String>,
}

impl W3Mmd {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        let mut i = 0;
        let mut any_parsed = false;

        while i + 9 <= action_data.len() {
            let tag = action_data[i];
            let is_supported_tag = matches!(tag, 0x6B | 0x6C | 0x6D | 0x6F);

            if is_supported_tag
                && action_data[i + 1..i + 8].eq_ignore_ascii_case(b"mmd.dat")
                && action_data[i + 8] == 0
            {
                let start = i + 9;
                let Some(mission_null) = action_data[start..].iter().position(|&b| b == 0) else {
                    i += 1;
                    continue;
                };
                let mission_bytes = &action_data[start..start + mission_null];
                let key_start = start + mission_null + 1;

                let Some(key_null) = action_data[key_start..].iter().position(|&b| b == 0) else {
                    i += 1;
                    continue;
                };
                let key_bytes = &action_data[key_start..key_start + key_null];
                let val_start = key_start + key_null + 1;

                let mission_str = String::from_utf8_lossy(mission_bytes);
                let key_str = String::from_utf8_lossy(key_bytes);

                let mut int_val = None;
                if tag != 0x6F && val_start + 4 <= action_data.len() {
                    int_val = Some(u32::from_le_bytes([
                        action_data[val_start],
                        action_data[val_start + 1],
                        action_data[val_start + 2],
                        action_data[val_start + 3],
                    ]));
                }

                let parsed = self.parse_statement(&mission_str, &key_str, int_val);
                if parsed {
                    any_parsed = true;
                }

                i = val_start + if tag == 0x6F { 0 } else { 4 };
                continue;
            }

            i += 1;
        }

        any_parsed
    }

    fn parse_statement(&mut self, mission: &str, key: &str, int_val: Option<u32>) -> bool {
        let stmt = if key.contains(' ') || key.starts_with("init") || key.starts_with("VarP") || key.starts_with("FlagP") {
            key
        } else {
            mission
        };

        let mut parts = stmt.split_whitespace();
        let Some(verb) = parts.next() else {
            return false;
        };

        if verb.eq_ignore_ascii_case("init") {
            self.initialized = true;
            return true;
        }

        if verb.eq_ignore_ascii_case("FlagP")
            && let (Some(pid_str), Some(flag)) = (parts.next(), parts.next())
            && let Ok(pid) = pid_str.parse::<u8>()
        {
            self.flags.insert(pid, flag.to_lowercase());
            return true;
        }

        if verb.eq_ignore_ascii_case("VarP") {
            let Some(pid_str) = parts.next() else { return false };
            let Some(var_name) = parts.next() else { return false };
            let Ok(pid) = pid_str.parse::<u8>() else { return false };

            let op = parts.next();
            let val_str = parts.next();

            if let Some(op_sym) = op {
                let current_num = match self.player_vars.get(&(pid, var_name.to_string())) {
                    Some(MmdValue::Number(n)) => *n,
                    _ => 0.0,
                };

                let target_num = val_str
                    .and_then(|s| s.parse::<f64>().ok())
                    .or_else(|| int_val.map(|v| v as f64));

                match op_sym {
                    "=" => {
                        if let Some(n) = target_num {
                            self.player_vars
                                .insert((pid, var_name.to_string()), MmdValue::Number(n));
                        } else if let Some(text) = val_str {
                            self.player_vars
                                .insert((pid, var_name.to_string()), MmdValue::Text(text.to_string()));
                        }
                        return true;
                    }
                    "+=" => {
                        if let Some(n) = target_num {
                            self.player_vars
                                .insert((pid, var_name.to_string()), MmdValue::Number(current_num + n));
                            return true;
                        }
                    }
                    "-=" => {
                        if let Some(n) = target_num {
                            self.player_vars
                                .insert((pid, var_name.to_string()), MmdValue::Number(current_num - n));
                            return true;
                        }
                    }
                    _ => {
                        if let Ok(n) = op_sym.parse::<f64>() {
                            self.player_vars
                                .insert((pid, var_name.to_string()), MmdValue::Number(n));
                            return true;
                        }
                    }
                }
            } else if let Some(v) = int_val {
                self.player_vars
                    .insert((pid, var_name.to_string()), MmdValue::Number(v as f64));
                return true;
            }
        }

        if verb.eq_ignore_ascii_case("Event") {
            let rest = parts.collect::<Vec<_>>().join(" ");
            self.events.push(rest);
            return true;
        }

        false
    }

    #[must_use]
    pub fn get_player_val(&self, pid: u8, var_name: &str) -> Option<&MmdValue> {
        self.player_vars.get(&(pid, var_name.to_string()))
    }

    #[must_use]
    pub fn is_winner(&self, pid: u8) -> bool {
        self.flags
            .get(&pid)
            .map(|f| f == "winner" || f == "won")
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_mmd_action(mission: &str, key: &str, val: u32) -> Vec<u8> {
        let mut b = Vec::new();
        b.push(0x6B);
        b.extend_from_slice(b"MMD.Dat\0");
        b.extend_from_slice(mission.as_bytes());
        b.push(0);
        b.extend_from_slice(key.as_bytes());
        b.push(0);
        b.extend_from_slice(&val.to_le_bytes());
        b
    }

    #[test]
    fn parses_mmd_init_and_flags() {
        let mut mmd = W3Mmd::new();
        assert!(!mmd.initialized);

        let init_act = make_mmd_action("val", "init version 1 1", 0);
        assert!(mmd.process_action(&init_act));
        assert!(mmd.initialized);

        let win_act = make_mmd_action("val", "FlagP 0 winner", 0);
        let lose_act = make_mmd_action("val", "FlagP 1 loser", 0);
        assert!(mmd.process_action(&win_act));
        assert!(mmd.process_action(&lose_act));

        assert!(mmd.is_winner(0));
        assert!(!mmd.is_winner(1));
    }

    #[test]
    fn parses_mmd_varp_numeric_and_arithmetic() {
        let mut mmd = W3Mmd::new();

        let set_kills = make_mmd_action("val", "VarP 0 kills = 5", 0);
        assert!(mmd.process_action(&set_kills));
        assert_eq!(
            mmd.get_player_val(0, "kills"),
            Some(&MmdValue::Number(5.0))
        );

        let add_kills = make_mmd_action("val", "VarP 0 kills += 3", 0);
        assert!(mmd.process_action(&add_kills));
        assert_eq!(
            mmd.get_player_val(0, "kills"),
            Some(&MmdValue::Number(8.0))
        );

        let sub_kills = make_mmd_action("val", "VarP 0 kills -= 2", 0);
        assert!(mmd.process_action(&sub_kills));
        assert_eq!(
            mmd.get_player_val(0, "kills"),
            Some(&MmdValue::Number(6.0))
        );
    }
}
