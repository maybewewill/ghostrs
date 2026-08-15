use std::collections::HashMap;
use crate::logger::*;
use crate::util::byte_array_to_uint32;

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

    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        let mut i = 0;
        let sig = b"kMMD.Dat\0";

        while action_data.len() >= i + 9 {
            if &action_data[i..i+9] == sig {
                let pos = i + 9;
                if let Some((mission_key, next_pos)) = Self::extract_c_string(action_data, pos) {
                    if let Some((key_str, next_pos2)) = Self::extract_c_string(action_data, next_pos) {
                        if action_data.len() >= next_pos2 + 4 {
                            let value_bytes = &action_data[next_pos2..next_pos2+4];
                            let value_int = byte_array_to_uint32(&value_bytes.to_vec(), false, 0);

                            self.handle_mmd(&mission_key, &key_str, value_int, value_bytes);
                            i = next_pos2 + 4;
                            continue;
                        }
                    }
                }
            }
            i += 1;
        }
        false
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

    fn handle_mmd(&mut self, mission_key: &str, key: &str, value_int: u32, _value_raw: &[u8]) {
        if mission_key.starts_with("val:") {
            let tokens: Vec<&str> = key.split_whitespace().collect();
            if tokens.is_empty() {
                return;
            }

            match tokens[0] {
                "init" => {
                    // init version
                    log_info(&format!("[STATSW3MMD: {}] init: {}", self.game_name, key));
                }
                "DefVarP" if tokens.len() >= 3 => {
                    let var_name = tokens[1].to_string();
                    let var_type = tokens[2].to_string();
                    self.def_vars.insert(var_name, var_type);
                }
                "VarP" if tokens.len() >= 4 => {
                    if let (Ok(pid), Ok(val)) = (tokens[1].parse::<u32>(), tokens[3].parse::<i32>()) {
                        let var_name = tokens[2].to_string();
                        self.var_ints.insert((pid, var_name), val);
                    }
                }
                "FlagP" if tokens.len() >= 3 => {
                    if let Ok(pid) = tokens[1].parse::<u32>() {
                        let flag = tokens[2].to_string();
                        if flag == "leaver" {
                            self.flags_leaver.insert(pid, true);
                        } else if flag == "practicing" {
                            self.flags_practicing.insert(pid, true);
                        } else {
                            self.flags.insert(pid, flag);
                        }
                    }
                }
                "Event" if tokens.len() >= 2 => {
                    let event_name = tokens[1].to_string();
                    let args: Vec<String> = tokens[2..].iter().map(|s| s.to_string()).collect();
                    self.events.entry(event_name).or_default().extend(args);
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_w3mmd_parsing() {
        let mut mmd = StatsW3MMD::new("LegionTD".to_string(), "ltd".to_string());

        // Packet format: "kMMD.Dat\0" + "val:0\0" + "VarP 0 kills 42\0" + [0,0,0,0]
        let mut packet = b"kMMD.Dat\0".to_vec();
        packet.extend_from_slice(b"val:0\0");
        packet.extend_from_slice(b"VarP 0 kills 42\0");
        packet.extend_from_slice(&0u32.to_le_bytes());

        mmd.process_action(&packet);
        assert_eq!(mmd.var_ints.get(&(0, "kills".to_string())), Some(&42));
    }
}
