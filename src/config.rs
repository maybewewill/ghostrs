use std::collections::HashMap;
use std::fs;
use std::sync::OnceLock;

static CONFIG: OnceLock<Config> = OnceLock::new();

#[derive(Debug)]
pub struct Config {
    values: HashMap<String, String>,
}

impl Config {
    fn new(filename: &str) -> Self {
        let contents = fs::read_to_string(filename)
            .unwrap_or_else(|_| panic!("Failed to read config file: {}", filename));

        let values = contents
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(2, '=').collect();
                if parts.len() == 2 {
                    Some((parts[0].trim().to_string(), parts[1].trim().to_string()))
                } else {
                    None
                }
            })
            .collect();

        Config { values }
    }

    fn get_string(&self, key: &str, default: &str) -> String {
        self.values.get(key).cloned().unwrap_or_else(|| default.to_string())
    }

    fn get_int(&self, key: &str, default: i32) -> i32 {
        self.values.get(key)
            .and_then(|v| v.parse::<i32>().ok())
            .unwrap_or(default)
    }

    fn get_bool(&self, key: &str, default: bool) -> bool {
        self.values
            .get(key)
            .map(|v| {
                match v.trim().to_lowercase().as_str() {
                    "true" | "yes" | "1" => true,
                    "false" | "no" | "0" => false,
                    _ => default,
                }
            })
            .unwrap_or(default)
    }
    
}

/// Initialize config (call once in main)
pub fn init(filename: &str) {
    CONFIG.get_or_init(|| Config::new(filename));
}

/// Get string value with default
pub fn get_string(key: &str, default: &str) -> String {
    CONFIG.get().expect("Config not initialized").get_string(key, default)
}

/// Get int value with default
pub fn get_int(key: &str, default: i32) -> i32 {
    CONFIG.get().expect("Config not initialized").get_int(key, default)
}

/// Get bool value with default
pub fn get_bool(key: &str, default: bool) -> bool {
    CONFIG.get().expect("Config not initialized").get_bool(key, default)
}
