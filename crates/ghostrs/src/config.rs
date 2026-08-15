use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use ghost_bnet::BnetConfig;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BotConfig {
    pub war3_path: String,
    pub map_path: String,
    pub max_games: usize,
    pub tft: bool,
    pub bind_address: String,
    pub host_port: u16,
}

#[derive(Debug, Clone)]
pub struct GameDefaults {
    pub latency: Duration,
    pub sync_limit: u32,
    pub virtual_host_name: String,
    pub reconnect_wait: Duration,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SpectatorConfig {
    pub enabled: bool,
    pub port: u16,
    pub delay: Duration,
    pub max_viewers: usize,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub bot: BotConfig,
    pub bnet: BnetConfig,
    pub game: GameDefaults,
    pub spectator: SpectatorConfig,
    pub db_path: PathBuf,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file at {}", path.display()))?;
        Self::parse(&content)
    }

    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let mut map = HashMap::new();
        for line in s.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                map.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }

        let bot_maxgames = parse_int(&map, "bot_maxgames", 20)?;
        let bot_tft = parse_bool(&map, "bot_tft", true);
        let bot_hostport = parse_int(&map, "bot_hostport", 6112)?;

        let bnet_server = map.get("bnet_server").cloned().unwrap_or_else(|| "wc3.theabyss.ru".into());
        let bnet_port = parse_int(&map, "bnet_serverport", 6112)?;
        let bnet_username = map.get("bnet_username").cloned().unwrap_or_else(|| "BOT".into());
        let bnet_password = map.get("bnet_password").cloned().unwrap_or_default();
        let bnet_cdkey_roc = map.get("bnet_cdkeyroc").cloned().unwrap_or_else(|| "FFFFFFFFFFFFFFFFFFFFFFFFFF".into());
        let bnet_cdkey_tft = map.get("bnet_cdkeytft").cloned().unwrap_or_else(|| "FFFFFFFFFFFFFFFFFFFFFFFFFF".into());
        let bnet_first_channel = map.get("bnet_firstchannel").cloned().unwrap_or_else(|| "The Abyss".into());
        let bnet_root_admins = map
            .get("bnet_rootadmin")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        let bnet_command_trigger = map
            .get("bnet_commandtrigger")
            .and_then(|s| s.chars().next())
            .unwrap_or('!');
        let bnet_war3_version = parse_int(&map, "bnet_custom_war3version", 26)? as u8;

        let latency_ms = parse_int(&map, "bot_latency", 100)?;
        let sync_limit = parse_int(&map, "bot_synclimit", 50)?;
        let reconnect_wait_sec = parse_int(&map, "bot_reconnectwaittime", 180)?;

        let spectator_enabled = parse_bool(&map, "spectator_enabled", false);
        let spectator_port = parse_int(&map, "spectator_port", 6114)?;
        let spectator_delay_sec = parse_int(&map, "spectator_delay", 120)?;
        let spectator_max_viewers = parse_int(&map, "spectator_maxviewers", 32)?;

        let db_path = map
            .get("db_path")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ghost.db"));

        Ok(Config {
            bot: BotConfig {
                war3_path: map.get("bot_war3path").cloned().unwrap_or_default(),
                map_path: map.get("bot_mappath").cloned().unwrap_or_else(|| "maps/".into()),
                max_games: bot_maxgames,
                tft: bot_tft,
                bind_address: map.get("bot_bindaddress").cloned().unwrap_or_else(|| "0.0.0.0".into()),
                host_port: bot_hostport,
            },
            bnet: BnetConfig {
                server: bnet_server,
                port: bnet_port,
                username: bnet_username,
                password: bnet_password,
                cdkey_roc: bnet_cdkey_roc,
                cdkey_tft: bnet_cdkey_tft,
                first_channel: bnet_first_channel,
                root_admins: bnet_root_admins,
                command_trigger: bnet_command_trigger,
                war3_version: bnet_war3_version,
                exe_version: [1, 0, 26, 1],
                exe_version_hash: [0, 0, 0, 0],
                reconnect_delay: Duration::from_secs(5),
            },
            game: GameDefaults {
                latency: Duration::from_millis(latency_ms),
                sync_limit: sync_limit as u32,
                virtual_host_name: map.get("bot_virtualhostname").cloned().unwrap_or_else(|| "|cFF4080C0Ghost".into()),
                reconnect_wait: Duration::from_secs(reconnect_wait_sec),
            },
            spectator: SpectatorConfig {
                enabled: spectator_enabled,
                port: spectator_port,
                delay: Duration::from_secs(spectator_delay_sec),
                max_viewers: spectator_max_viewers,
            },
            db_path,
        })
    }
}

fn parse_int<T: std::str::FromStr>(map: &HashMap<String, String>, key: &str, default: T) -> anyhow::Result<T> {
    match map.get(key) {
        Some(v) => v.parse().map_err(|_| anyhow::anyhow!("failed to parse integer for key {key}: {v}")),
        None => Ok(default),
    }
}

fn parse_bool(map: &HashMap<String, String>, key: &str, default: bool) -> bool {
    match map.get(key) {
        Some(v) => v == "1" || v.eq_ignore_ascii_case("true"),
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
bot_war3path = C:\\war3\\
bot_mappath = maps/
bot_maxgames = 20
bot_tft = 1
bnet_server = wc3.theabyss.ru
bnet_username = BOT
bnet_commandtrigger = !
# a comment
bnet_rootadmin = slash admin2
";

    #[test]
    fn parses_types_and_lists() {
        let c = Config::parse(SAMPLE).unwrap();
        assert_eq!(c.bot.max_games, 20);
        assert!(c.bot.tft);
        assert_eq!(c.bnet.server, "wc3.theabyss.ru");
        assert_eq!(c.bnet.command_trigger, '!');
        assert_eq!(c.bnet.root_admins, vec!["slash", "admin2"]);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let c = Config::parse("# nothing\n\n  \nbot_maxgames = 3\n").unwrap();
        assert_eq!(c.bot.max_games, 3);
    }

    #[test]
    fn missing_keys_fall_back_to_documented_defaults() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.game.latency.as_millis(), 100);
        assert_eq!(c.game.sync_limit, 50);
    }

    #[test]
    fn an_unparseable_number_is_an_error_not_a_silent_zero() {
        assert!(Config::parse("bot_maxgames = twenty\n").is_err());
    }
}
