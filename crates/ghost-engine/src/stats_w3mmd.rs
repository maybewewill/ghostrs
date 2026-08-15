use std::collections::HashMap;

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

    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        let sig = b"kMMD.Dat\0";
        if let Some(pos) = action_data.windows(sig.len()).position(|w| w == sig) {
            let payload = &action_data[pos + sig.len()..];
            if let Ok(text) = std::str::from_utf8(payload) {
                for line in text.split('\0') {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.is_empty() {
                        continue;
                    }
                    match parts[0] {
                        "DefVarP" => {
                            if parts.len() >= 3 {
                                self.def_vars
                                    .insert(parts[1].to_string(), parts[2].to_string());
                            }
                        }
                        "VarP" => {
                            if parts.len() >= 4
                                && let Ok(pid) = parts[1].parse::<u32>()
                            {
                                let var = parts[2].to_string();
                                let val_str = parts[3];
                                if let Some(vtype) = self.def_vars.get(&var) {
                                    match vtype.as_str() {
                                        "int" => {
                                            if let Ok(v) = val_str.parse::<i32>() {
                                                self.var_ints.insert((pid, var), v);
                                            }
                                        }
                                        "real" => {
                                            if let Ok(v) = val_str.parse::<f64>() {
                                                self.var_reals.insert((pid, var), v);
                                            }
                                        }
                                        _ => {
                                            self.var_strings
                                                .insert((pid, var), val_str.to_string());
                                        }
                                    }
                                }
                            }
                        }
                        "FlagP" => {
                            if parts.len() >= 3
                                && let Ok(pid) = parts[1].parse::<u32>()
                            {
                                let flag = parts[2];
                                match flag {
                                    "winner" | "loser" | "drawer" => {
                                        self.flags.insert(pid, flag.to_string());
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
                        "Event" if parts.len() >= 2 => {
                            let event_name = parts[1].to_string();
                            let args = parts[2..].join(" ");
                            self.events.entry(event_name).or_default().push(args);
                        }
                        _ => {}
                    }
                }
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_w3mmd_variables_and_flags() {
        let mut mmd = StatsW3MMD::new("Legion TD".into(), "legion".into());
        mmd.set_player(0, "Slash".into());

        let mut data = Vec::new();
        data.extend_from_slice(b"kMMD.Dat\0");
        data.extend_from_slice(b"DefVarP wave int\0");
        data.extend_from_slice(b"VarP 0 wave 30\0");
        data.extend_from_slice(b"FlagP 0 winner\0");

        assert!(mmd.process_action(&data));
        assert_eq!(mmd.var_ints.get(&(0, "wave".into())), Some(&30));
        assert_eq!(mmd.flags.get(&0), Some(&"winner".to_string()));
    }
}
