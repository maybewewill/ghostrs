use std::collections::HashMap;

pub fn tokenize_key(key: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut token = String::new();
    let mut escaping = false;

    for ch in key.chars() {
        if escaping {
            if ch == ' ' {
                token.push(' ');
            } else if ch == '\\' {
                token.push('\\');
            } else {
                // invalid escape sequence
                return Vec::new();
            }
            escaping = false;
        } else if ch == ' ' {
            if token.is_empty() {
                return Vec::new();
            }
            tokens.push(token);
            token = String::new();
        } else if ch == '\\' {
            escaping = true;
        } else {
            token.push(ch);
        }
    }

    if token.is_empty() {
        return Vec::new();
    }
    tokens.push(token);
    tokens
}

#[derive(Debug, Clone, Default)]
pub struct StatsW3MMD {
    pub category: String,
    pub next_value_id: u32,
    pub next_check_id: u32,
    pub pid_to_name: HashMap<u32, String>,
    pub flags: HashMap<u32, String>,
    pub flags_leaver: HashMap<u32, bool>,
    pub flags_practicing: HashMap<u32, bool>,
    pub def_vars: HashMap<String, String>, // var_name -> type (int, real, string)
    pub var_ints: HashMap<(u32, String), i32>,
    pub var_reals: HashMap<(u32, String), f64>,
    pub var_strings: HashMap<(u32, String), String>,
    pub events: HashMap<String, Vec<String>>,
    pub game_name: String,
}

impl StatsW3MMD {
    pub fn new(game_name: String, category: String) -> Self {
        Self {
            game_name,
            category,
            ..Default::default()
        }
    }

    pub fn set_player(&mut self, pid: u32, name: String) {
        self.pid_to_name.insert(pid, name);
    }

    /// Parses W3MMD actions matching GHost++ `CStatsW3MMD::ProcessAction` (statsw3mmd.cpp:44-344).
    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        let mut i = 0;
        let sig = b"kMMD.Dat\0";

        while i + 9 <= action_data.len() {
            if &action_data[i..i + 9] == sig {
                let start = i + 9;
                // Extract null-terminated MissionKey
                let Some(mkey_null) = action_data[start..].iter().position(|&b| b == 0) else {
                    i += 1;
                    continue;
                };
                let mkey_bytes = &action_data[start..start + mkey_null];
                let key_start = start + mkey_null + 1;

                // Extract null-terminated Key
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

                let _value_int = u32::from_le_bytes([
                    action_data[val_start],
                    action_data[val_start + 1],
                    action_data[val_start + 2],
                    action_data[val_start + 3],
                ]);

                let mkey_str = String::from_utf8_lossy(mkey_bytes);
                let key_str = String::from_utf8_lossy(key_bytes);

                if mkey_str.starts_with("val:") {
                    let tokens = tokenize_key(&key_str);
                    if !tokens.is_empty() {
                        match tokens[0].as_str() {
                            "init" => {
                                if tokens.len() == 4 && tokens[1] == "pid" {
                                    if let Ok(pid) = tokens[2].parse::<u32>() {
                                        self.pid_to_name.insert(pid, tokens[3].clone());
                                    }
                                }
                            }
                            "DefVarP" => {
                                if tokens.len() == 5
                                    && (tokens[2] == "int" || tokens[2] == "real" || tokens[2] == "string")
                                {
                                    self.def_vars.insert(tokens[1].clone(), tokens[2].clone());
                                }
                            }
                            "VarP" => {
                                if tokens.len() == 5 {
                                    if let Ok(pid) = tokens[1].parse::<u32>() {
                                        let var_name = tokens[2].clone();
                                        if let Some(vtype) = self.def_vars.get(&var_name).cloned() {
                                            let op = &tokens[3];
                                            let val_str = &tokens[4];
                                            match vtype.as_str() {
                                                "int" => {
                                                    if let Ok(val) = val_str.parse::<i32>() {
                                                        let entry = self.var_ints.entry((pid, var_name)).or_insert(0);
                                                        match op.as_str() {
                                                            "=" => *entry = val,
                                                            "+=" => *entry += val,
                                                            "-=" => *entry -= val,
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                "real" => {
                                                    if let Ok(val) = val_str.parse::<f64>() {
                                                        let entry = self.var_reals.entry((pid, var_name)).or_insert(0.0);
                                                        match op.as_str() {
                                                            "=" => *entry = val,
                                                            "+=" => *entry += val,
                                                            "-=" => *entry -= val,
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                _ => {
                                                    if op == "=" {
                                                        self.var_strings.insert((pid, var_name), val_str.clone());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            "FlagP" => {
                                if tokens.len() == 3 {
                                    if let Ok(pid) = tokens[1].parse::<u32>() {
                                        match tokens[2].as_str() {
                                            "winner" | "loser" | "drawer" => {
                                                self.flags.insert(pid, tokens[2].clone());
                                            }
                                            "leaver" => {
                                                self.flags_leaver.insert(pid, true);
                                            }
                                            "practicing" => {
                                                self.flags_practicing.insert(pid, true);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                            "DefEvent" => {
                                if tokens.len() >= 4 {
                                    let event_name = tokens[1].clone();
                                    let args = tokens[3..].to_vec();
                                    self.events.insert(event_name, args);
                                }
                            }
                            "Event" => {
                                if tokens.len() >= 2 {
                                    let event_name = &tokens[1];
                                    if let Some(def) = self.events.get(event_name).cloned()
                                        && !def.is_empty()
                                    {
                                        let mut format = def.last().cloned().unwrap_or_default();
                                        let num_args = tokens.len() - 2;
                                        if num_args == def.len() - 1 {
                                            for idx in 0..num_args {
                                                let arg_token = &tokens[idx + 2];
                                                let marker = format!("{{{}}}", idx);
                                                if def[idx].starts_with("pid:") {
                                                    if let Ok(pid) = arg_token.parse::<u32>() {
                                                        let name = self
                                                            .pid_to_name
                                                            .get(&pid)
                                                            .cloned()
                                                            .unwrap_or_else(|| format!("PID:{}", pid));
                                                        format = format.replace(&marker, &name);
                                                    } else {
                                                        format = format.replace(&marker, arg_token);
                                                    }
                                                } else {
                                                    format = format.replace(&marker, arg_token);
                                                }
                                            }
                                            tracing::info!("[STATSW3MMD: {}] {}", self.game_name, format);
                                        }
                                    }
                                }
                            }
                            "Blank" | "Custom" => {}
                            _ => {}
                        }
                    }
                    self.next_value_id += 1;
                } else if mkey_str.starts_with("chk:") {
                    self.next_check_id += 1;
                }

                i = val_start + 4;
            } else {
                i += 1;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_w3mmd_action(mission_key: &str, key: &str, value: u32) -> Vec<u8> {
        let mut pkt = Vec::new();
        pkt.extend_from_slice(b"kMMD.Dat\0");
        pkt.extend_from_slice(mission_key.as_bytes());
        pkt.push(0x00);
        pkt.extend_from_slice(key.as_bytes());
        pkt.push(0x00);
        pkt.extend_from_slice(&value.to_le_bytes());
        pkt
    }

    #[test]
    fn parses_w3mmd_variables_and_flags() {
        let mut mmd = StatsW3MMD::new("Legion TD".into(), "legion".into());
        mmd.set_player(0, "Slash".into());

        mmd.process_action(&make_w3mmd_action("val:0", "DefVarP wave int none none", 0));
        mmd.process_action(&make_w3mmd_action("val:1", "VarP 0 wave = 30", 0));
        mmd.process_action(&make_w3mmd_action("val:2", "FlagP 0 winner", 0));

        assert_eq!(mmd.var_ints.get(&(0, "wave".into())), Some(&30));
        assert_eq!(mmd.flags.get(&0), Some(&"winner".to_string()));
    }
}
