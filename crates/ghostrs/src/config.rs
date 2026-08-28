use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context;
use ghost_bnet::BnetConfig;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct BotConfig {
    pub war3_path: String,
    pub map_path: String,
    pub default_map: Option<String>,
    pub max_games: usize,
    pub tft: bool,
    pub bind_address: String,
    pub host_port: u16,
    pub port_pool_start: Option<u16>,
    pub port_pool_end: Option<u16>,
    pub gproxy_reconnect_port: u16,
    pub udp_broadcast_target: String,
}

impl BotConfig {
    #[must_use]
    pub fn resolved_udp_broadcast_target(&self) -> std::net::Ipv4Addr {
        let trimmed = self.udp_broadcast_target.trim();
        if trimmed.is_empty() {
            std::net::Ipv4Addr::BROADCAST
        } else {
            trimmed.parse().unwrap_or(std::net::Ipv4Addr::BROADCAST)
        }
    }

    #[must_use]
    pub fn is_port_pool_enabled(&self) -> bool {
        self.port_pool_start.is_some() && self.port_pool_end.is_some()
    }

    #[must_use]
    pub fn port_pool_range(&self) -> (u16, u16) {
        let start = self.port_pool_start.unwrap_or(self.host_port);
        let end = self.port_pool_end.unwrap_or(start);
        (start.min(end), start.max(end))
    }
}

#[derive(Debug, Clone)]
pub struct GameDefaults {
    pub latency: Duration,
    pub sync_limit: u32,
    pub virtual_host_name: String,
    pub reconnect_wait: Duration,
    pub max_downloaders: u32,
    pub max_download_speed: u32,
    pub allow_downloads: u8,
    pub autokick_ping: u32,
    pub lc_pings: bool,
    pub spoof_checks: u8,
    pub require_spoof_checks: bool,
    pub lobby_time_limit: u32,
}

#[derive(Debug, Clone)]
pub struct SpectatorConfig {
    pub enabled: bool,
    pub port: u16,
    /// Live replay-stream spectating (`-loadfile` bootstrap + chunk feed).
    /// Independent of the W3GS relay above and needs its own port.
    pub dotatv_enabled: bool,
    pub dotatv_port: u16,
    pub delay: Duration,
    pub max_viewers: usize,
    pub history_max_mb: usize,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bot: BotConfig,
    pub bnet: BnetConfig,
    pub game: GameDefaults,
    pub spectator: SpectatorConfig,
    pub db_path: PathBuf,
    pub maps: HashMap<String, ghost_engine::MapOverride>,
}

fn default_war3_path() -> String {
    "war3".into()
}
fn default_map_path() -> String {
    "maps".into()
}
fn default_max_games() -> usize {
    20
}
fn default_true() -> bool {
    true
}
fn default_bind_address() -> String {
    "0.0.0.0".into()
}
fn default_host_port() -> u16 {
    6112
}
fn default_bnet_server() -> String {
    "wc3.theabyss.ru".into()
}
fn default_bnet_port() -> u16 {
    6112
}
fn default_bnet_username() -> String {
    "BOT".into()
}
fn default_cdkey() -> String {
    "FFFFFFFFFFFFFFFFFFFFFFFFFF".into()
}
fn default_first_channel() -> String {
    "The Abyss".into()
}
fn default_command_trigger() -> char {
    '!'
}
fn default_war3_version() -> u8 {
    26
}
fn default_reconnect_delay_sec() -> u64 {
    5
}
fn default_password_hash_type() -> String {
    "pvpgn".into()
}
fn default_latency_ms() -> u64 {
    100
}
fn default_sync_limit() -> u32 {
    50
}
fn default_virtual_host_name() -> String {
    "|cFF4080C0Ghost".into()
}
fn default_reconnect_wait_sec() -> u64 {
    180
}
fn default_gproxy_reconnect_port() -> u16 {
    6114
}
fn default_dotatv_port() -> u16 {
    6116
}
fn default_spectator_port() -> u16 {
    6115
}
fn default_spectator_delay_sec() -> u64 {
    120
}
fn default_max_viewers() -> usize {
    32
}
fn default_history_max_mb() -> usize {
    64
}
fn default_db_path() -> PathBuf {
    PathBuf::from("ghost.db")
}

fn default_max_downloaders() -> u32 {
    3
}
fn default_max_download_speed() -> u32 {
    100
}
fn default_allow_downloads() -> u8 {
    1
}
fn default_autokick_ping() -> u32 {
    400
}
fn default_lc_pings() -> bool {
    true
}
fn default_spoof_checks() -> u8 {
    0
}
fn default_false() -> bool {
    false
}
fn default_lobby_time_limit() -> u32 {
    10
}

#[derive(Debug, Clone, Deserialize)]
pub struct TomlBot {
    #[serde(default = "default_war3_path")]
    pub war3_path: String,
    #[serde(default = "default_map_path")]
    pub map_path: String,
    pub default_map: Option<String>,
    #[serde(default = "default_max_games")]
    pub max_games: usize,
    #[serde(default = "default_true")]
    pub tft: bool,
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    #[serde(default = "default_host_port")]
    pub host_port: u16,
    #[serde(default)]
    pub port_pool_start: Option<u16>,
    #[serde(default)]
    pub port_pool_end: Option<u16>,
    #[serde(default = "default_gproxy_reconnect_port")]
    pub gproxy_reconnect_port: u16,
    #[serde(default)]
    pub udp_broadcast_target: Option<String>,
}

impl Default for TomlBot {
    fn default() -> Self {
        Self {
            war3_path: default_war3_path(),
            map_path: default_map_path(),
            default_map: None,
            max_games: default_max_games(),
            tft: default_true(),
            bind_address: default_bind_address(),
            host_port: default_host_port(),
            port_pool_start: None,
            port_pool_end: None,
            gproxy_reconnect_port: default_gproxy_reconnect_port(),
            udp_broadcast_target: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TomlBnet {
    #[serde(default = "default_bnet_server")]
    pub server: String,
    #[serde(default = "default_bnet_port")]
    pub port: u16,
    #[serde(default = "default_bnet_username")]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_cdkey")]
    pub cdkey_roc: String,
    #[serde(default = "default_cdkey")]
    pub cdkey_tft: String,
    #[serde(default = "default_first_channel")]
    pub first_channel: String,
    #[serde(default)]
    pub root_admins: Vec<String>,
    #[serde(default = "default_command_trigger")]
    pub command_trigger: char,
    #[serde(default = "default_war3_version")]
    pub war3_version: u8,
    #[serde(default = "default_reconnect_delay_sec")]
    pub reconnect_delay_sec: u64,
    #[serde(default = "default_password_hash_type")]
    pub password_hash_type: String,
    #[serde(default)]
    pub server_alias: Option<String>,
    #[serde(default)]
    pub pvpgn_realm_name: Option<String>,
}

impl Default for TomlBnet {
    fn default() -> Self {
        Self {
            server: default_bnet_server(),
            port: default_bnet_port(),
            username: default_bnet_username(),
            password: String::new(),
            cdkey_roc: default_cdkey(),
            cdkey_tft: default_cdkey(),
            first_channel: default_first_channel(),
            root_admins: Vec::new(),
            command_trigger: default_command_trigger(),
            war3_version: default_war3_version(),
            reconnect_delay_sec: default_reconnect_delay_sec(),
            password_hash_type: default_password_hash_type(),
            server_alias: None,
            pvpgn_realm_name: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TomlGame {
    #[serde(default = "default_latency_ms")]
    pub latency_ms: u64,
    #[serde(default = "default_sync_limit")]
    pub sync_limit: u32,
    #[serde(default = "default_virtual_host_name")]
    pub virtual_host_name: String,
    #[serde(default = "default_reconnect_wait_sec")]
    pub reconnect_wait_sec: u64,
    #[serde(default = "default_max_downloaders")]
    pub max_downloaders: u32,
    #[serde(default = "default_max_download_speed")]
    pub max_download_speed: u32,
    #[serde(default = "default_allow_downloads")]
    pub allow_downloads: u8,
    #[serde(default = "default_autokick_ping")]
    pub autokick_ping: u32,
    #[serde(default = "default_lc_pings")]
    pub lc_pings: bool,
    #[serde(default = "default_spoof_checks")]
    pub spoof_checks: u8,
    #[serde(default = "default_false")]
    pub require_spoof_checks: bool,
    #[serde(default = "default_lobby_time_limit")]
    pub lobby_time_limit: u32,
}

impl Default for TomlGame {
    fn default() -> Self {
        Self {
            latency_ms: default_latency_ms(),
            sync_limit: default_sync_limit(),
            virtual_host_name: default_virtual_host_name(),
            reconnect_wait_sec: default_reconnect_wait_sec(),
            max_downloaders: default_max_downloaders(),
            max_download_speed: default_max_download_speed(),
            allow_downloads: default_allow_downloads(),
            autokick_ping: default_autokick_ping(),
            lc_pings: default_lc_pings(),
            spoof_checks: default_spoof_checks(),
            require_spoof_checks: default_false(),
            lobby_time_limit: default_lobby_time_limit(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TomlSpectator {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_spectator_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub dotatv_enabled: bool,
    #[serde(default = "default_dotatv_port")]
    pub dotatv_port: u16,
    #[serde(default = "default_spectator_delay_sec")]
    pub delay_sec: u64,
    #[serde(default = "default_max_viewers")]
    pub max_viewers: usize,
    #[serde(default = "default_history_max_mb")]
    pub history_max_mb: usize,
}

impl Default for TomlSpectator {
    fn default() -> Self {
        Self {
            enabled: false,
            port: default_spectator_port(),
            dotatv_enabled: true,
            dotatv_port: default_dotatv_port(),
            delay_sec: default_spectator_delay_sec(),
            max_viewers: default_max_viewers(),
            history_max_mb: default_history_max_mb(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct TomlDatabase {
    #[serde(default = "default_db_path")]
    pub path: PathBuf,
}

impl Default for TomlDatabase {
    fn default() -> Self {
        Self {
            path: default_db_path(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlMapOverride {
    pub speed: Option<u8>,
    pub visibility: Option<u8>,
    pub observers: Option<u8>,
    pub flags: Option<u8>,
    pub game_type: Option<u32>,
    pub filter_maker: Option<u8>,
    pub filter_type: Option<u8>,
    pub filter_size: Option<u8>,
    pub filter_obs: Option<u8>,
    pub options: Option<u32>,
    pub width: Option<u16>,
    pub height: Option<u16>,
    pub num_players: Option<u8>,
    pub num_teams: Option<u8>,
    pub map_type: Option<String>,
    pub matchmaking_category: Option<String>,
    pub stats_w3mmd_category: Option<String>,
    pub default_hcl: Option<String>,
    pub default_player_score: Option<u32>,
    pub loading_in_game: Option<bool>,
    pub local_path: Option<String>,
    pub max_slots: Option<u32>,
}

impl From<TomlMapOverride> for ghost_engine::MapOverride {
    fn from(t: TomlMapOverride) -> Self {
        Self {
            speed: t.speed,
            visibility: t.visibility,
            observers: t.observers,
            flags: t.flags,
            game_type: t.game_type,
            filter_maker: t.filter_maker,
            filter_type: t.filter_type,
            filter_size: t.filter_size,
            filter_obs: t.filter_obs,
            options: t.options,
            width: t.width,
            height: t.height,
            num_players: t.num_players,
            num_teams: t.num_teams,
            custom_slots: None,
            map_type: t.map_type,
            matchmaking_category: t.matchmaking_category,
            stats_w3mmd_category: t.stats_w3mmd_category,
            default_hcl: t.default_hcl,
            default_player_score: t.default_player_score,
            loading_in_game: t.loading_in_game,
            local_path: t.local_path,
            max_slots: t.max_slots,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TomlConfig {
    #[serde(default)]
    pub bot: Option<TomlBot>,
    #[serde(default)]
    pub bnet: Option<TomlBnet>,
    #[serde(default)]
    pub game: Option<TomlGame>,
    #[serde(default)]
    pub spectator: Option<TomlSpectator>,
    #[serde(default)]
    pub database: Option<TomlDatabase>,
    #[serde(default)]
    pub maps: HashMap<String, TomlMapOverride>,
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read config file at {}", path.display()))?;
        if path.extension().and_then(|e| e.to_str()) == Some("toml") {
            Self::from_toml(&content)
        } else {
            Self::parse(&content)
        }
    }

    pub fn from_toml(s: &str) -> anyhow::Result<Self> {
        let toml_cfg: TomlConfig =
            toml::from_str(s).context("failed to parse TOML configuration")?;

        let bot = toml_cfg.bot.unwrap_or_default();
        let bnet = toml_cfg.bnet.unwrap_or_default();
        let game = toml_cfg.game.unwrap_or_default();
        let spectator = toml_cfg.spectator.unwrap_or_default();
        let database = toml_cfg.database.unwrap_or_default();

        let mut maps = HashMap::new();
        for (k, v) in toml_cfg.maps {
            maps.insert(k, v.into());
        }

        Ok(Config {
            bot: BotConfig {
                war3_path: bot.war3_path,
                map_path: bot.map_path,
                default_map: bot.default_map,
                max_games: bot.max_games,
                tft: bot.tft,
                bind_address: bot.bind_address,
                host_port: bot.host_port,
                port_pool_start: bot.port_pool_start,
                port_pool_end: bot.port_pool_end,
                gproxy_reconnect_port: bot.gproxy_reconnect_port,
                udp_broadcast_target: bot.udp_broadcast_target.unwrap_or_default(),
            },
            bnet: BnetConfig {
                server_alias: bnet.server_alias.unwrap_or_else(|| bnet.server.clone()),
                pvpgn_realm_name: bnet.pvpgn_realm_name.unwrap_or_default(),
                server: bnet.server,
                port: bnet.port,
                host_port: bot.host_port,
                username: bnet.username,
                password: bnet.password,
                cdkey_roc: bnet.cdkey_roc,
                cdkey_tft: bnet.cdkey_tft,
                first_channel: bnet.first_channel,
                root_admins: bnet.root_admins,
                command_trigger: bnet.command_trigger,
                war3_version: bnet.war3_version,
                exe_version: [1, 0, 26, 1],
                exe_version_hash: [0, 0, 0, 0],
                password_hash_type: bnet.password_hash_type,
                reconnect_delay: Duration::from_secs(bnet.reconnect_delay_sec),
            },
            game: GameDefaults {
                latency: Duration::from_millis(game.latency_ms),
                sync_limit: game.sync_limit,
                virtual_host_name: game.virtual_host_name,
                reconnect_wait: Duration::from_secs(game.reconnect_wait_sec),
                max_downloaders: game.max_downloaders,
                max_download_speed: game.max_download_speed,
                allow_downloads: game.allow_downloads,
                autokick_ping: game.autokick_ping,
                lc_pings: game.lc_pings,
                spoof_checks: game.spoof_checks,
                require_spoof_checks: game.require_spoof_checks,
                lobby_time_limit: game.lobby_time_limit,
            },
            spectator: SpectatorConfig {
                enabled: spectator.enabled,
                port: spectator.port,
                dotatv_enabled: spectator.dotatv_enabled,
                dotatv_port: spectator.dotatv_port,
                delay: Duration::from_secs(spectator.delay_sec),
                max_viewers: spectator.max_viewers,
                history_max_mb: spectator.history_max_mb,
            },
            db_path: database.path,
            maps,
        })
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

        let bnet_server = map
            .get("bnet_server")
            .cloned()
            .unwrap_or_else(|| "wc3.theabyss.ru".into());
        let bnet_port = parse_int(&map, "bnet_serverport", 6112)?;
        let bnet_username = map
            .get("bnet_username")
            .cloned()
            .unwrap_or_else(|| "BOT".into());
        let bnet_password = map.get("bnet_password").cloned().unwrap_or_default();
        let bnet_cdkey_roc = map
            .get("bnet_cdkeyroc")
            .cloned()
            .unwrap_or_else(|| "FFFFFFFFFFFFFFFFFFFFFFFFFF".into());
        let bnet_cdkey_tft = map
            .get("bnet_cdkeytft")
            .cloned()
            .unwrap_or_else(|| "FFFFFFFFFFFFFFFFFFFFFFFFFF".into());
        let bnet_first_channel = map
            .get("bnet_firstchannel")
            .cloned()
            .unwrap_or_else(|| "The Abyss".into());
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
        let max_downloaders = parse_int(&map, "bot_maxdownloaders", 3)?;
        let max_download_speed = parse_int(&map, "bot_maxdownloadspeed", 100)?;
        let allow_downloads = parse_int(&map, "bot_allowdownloads", 1)? as u8;
        let autokick_ping = parse_int(&map, "bot_autokickping", 400)?;
        let lc_pings = parse_bool(&map, "bot_lcpings", true);
        let spoof_checks = parse_int(&map, "bnet_spoofchecks", 0)? as u8;
        let require_spoof_checks = parse_bool(&map, "bnet_requirespoofchecks", false);
        let lobby_time_limit = parse_int(&map, "bot_lobbytimelimit", 10).unwrap_or(10) as u32;

        let spectator_enabled = parse_bool(&map, "spectator_enabled", false);
        let spectator_port = parse_int(&map, "spectator_port", 6115)?;
        let spectator_dotatv_enabled = parse_bool(&map, "spectator_dotatv_enabled", true);
        let spectator_dotatv_port = parse_int(&map, "spectator_dotatv_port", 6116)?;
        let spectator_delay_sec = parse_int(&map, "spectator_delay", 120)?;
        let spectator_max_viewers = parse_int(&map, "spectator_maxviewers", 32)?;
        let spectator_history_max_mb = parse_int(&map, "spectator_history_max_mb", 64)?;
        let bot_reconnect_port = parse_int(&map, "bot_reconnectport", 6114)?;

        let db_path = map
            .get("db_path")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("ghost.db"));

        Ok(Config {
            bot: BotConfig {
                war3_path: map.get("bot_war3path").cloned().unwrap_or_default(),
                map_path: map
                    .get("bot_mappath")
                    .cloned()
                    .unwrap_or_else(|| "maps/".into()),
                default_map: map.get("bot_defaultmap").cloned(),
                max_games: bot_maxgames,
                tft: bot_tft,
                bind_address: map
                    .get("bot_bindaddress")
                    .cloned()
                    .unwrap_or_else(|| "0.0.0.0".into()),
                host_port: bot_hostport,
                port_pool_start: None,
                port_pool_end: None,
                gproxy_reconnect_port: bot_reconnect_port,
                udp_broadcast_target: map
                    .get("udp_broadcasttarget")
                    .or_else(|| map.get("udp_broadcast_target"))
                    .cloned()
                    .unwrap_or_default(),
            },
            bnet: BnetConfig {
                server_alias: map
                    .get("bnet_serveralias")
                    .cloned()
                    .unwrap_or_else(|| bnet_server.clone()),
                pvpgn_realm_name: map
                    .get("bnet_custom_pvpgnrealmname")
                    .cloned()
                    .unwrap_or_default(),
                server: bnet_server,
                port: bnet_port as u16,
                host_port: bot_hostport as u16,
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
                password_hash_type: map
                    .get("bnet_custom_passwordhashtype")
                    .cloned()
                    .unwrap_or_else(|| "pvpgn".into()),
                reconnect_delay: Duration::from_secs(5),
            },
            game: GameDefaults {
                latency: Duration::from_millis(latency_ms),
                sync_limit: sync_limit as u32,
                virtual_host_name: map
                    .get("bot_virtualhostname")
                    .cloned()
                    .unwrap_or_else(|| "|cFF4080C0Ghost".into()),
                reconnect_wait: Duration::from_secs(reconnect_wait_sec),
                max_downloaders,
                max_download_speed,
                allow_downloads,
                autokick_ping,
                lc_pings,
                spoof_checks,
                require_spoof_checks,
                lobby_time_limit,
            },
            spectator: SpectatorConfig {
                enabled: spectator_enabled,
                port: spectator_port,
                dotatv_enabled: spectator_dotatv_enabled,
                dotatv_port: spectator_dotatv_port,
                delay: Duration::from_secs(spectator_delay_sec),
                max_viewers: spectator_max_viewers,
                history_max_mb: spectator_history_max_mb,
            },
            db_path,
            maps: HashMap::new(),
        })
    }
}

fn parse_int<T: std::str::FromStr>(
    map: &HashMap<String, String>,
    key: &str,
    default: T,
) -> anyhow::Result<T> {
    match map.get(key) {
        Some(v) => v
            .parse()
            .map_err(|_| anyhow::anyhow!("failed to parse integer for key {key}: {v}")),
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

    const SAMPLE_CFG: &str = "\
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

    const SAMPLE_TOML: &str = r#"
[bot]
bind_address = "127.0.0.1"
host_port = 6112
gproxy_reconnect_port = 6114
max_games = 15
default_map = "iCCup DotA 454.w3x"
map_path = "maps"
war3_path = "war3"

[bnet]
server = "wc3.theabyss.ru"
port = 6112
username = "MY_BOT"
password = "supersecretpassword"
root_admins = ["slash", "bonjour"]
command_trigger = "!"
war3_version = 26

[game]
latency_ms = 50
sync_limit = 500
virtual_host_name = "|cFFEB0000iCCup"
reconnect_wait_sec = 180

[spectator]
enabled = true
port = 6115
delay_sec = 120
max_viewers = 32
history_max_mb = 128

[database]
path = "custom.db"
"#;

    #[test]
    fn parses_toml_correctly() {
        let c = Config::from_toml(SAMPLE_TOML).unwrap();
        assert_eq!(c.bot.bind_address, "127.0.0.1");
        assert_eq!(c.bot.host_port, 6112);
        assert_eq!(c.bot.gproxy_reconnect_port, 6114);
        assert_eq!(c.bot.max_games, 15);
        assert_eq!(c.bot.default_map.as_deref(), Some("iCCup DotA 454.w3x"));
        assert_eq!(c.bnet.username, "MY_BOT");
        assert_eq!(c.bnet.password, "supersecretpassword");
        assert_eq!(c.bnet.root_admins, vec!["slash", "bonjour"]);
        assert_eq!(c.game.latency.as_millis(), 50);
        assert_eq!(c.game.sync_limit, 500);
        assert!(c.spectator.enabled);
        assert_eq!(c.spectator.port, 6115);
        assert_eq!(c.spectator.delay.as_secs(), 120);
        assert_eq!(c.spectator.max_viewers, 32);
        assert_eq!(c.spectator.history_max_mb, 128);
        assert_eq!(c.db_path, PathBuf::from("custom.db"));
    }

    #[test]
    fn parses_empty_toml_with_defaults() {
        let c = Config::from_toml("").unwrap();
        assert_eq!(c.bot.host_port, 6112);
        assert_eq!(c.bot.gproxy_reconnect_port, 6114);
        assert_eq!(c.bnet.server, "wc3.theabyss.ru");
        assert_eq!(c.game.latency.as_millis(), 100);
        assert_eq!(c.game.sync_limit, 50);
        assert_eq!(c.spectator.port, 6115);
        assert_eq!(c.spectator.history_max_mb, 64);
    }

    #[test]
    fn parses_legacy_cfg_types_and_lists() {
        let c = Config::parse(SAMPLE_CFG).unwrap();
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
    fn an_unparseable_number_is_an_error_not_a_silent_zero() {
        assert!(Config::parse("bot_maxgames = twenty\n").is_err());
    }

    #[test]
    fn test_udp_broadcast_target_parsing() {
        let toml_custom = r#"
[bot]
udp_broadcast_target = "13.36.52.2"
"#;
        let c = Config::from_toml(toml_custom).unwrap();
        assert_eq!(c.bot.udp_broadcast_target, "13.36.52.2");
        assert_eq!(
            c.bot.resolved_udp_broadcast_target(),
            std::net::Ipv4Addr::new(13, 36, 52, 2)
        );

        let toml_empty = r#"
[bot]
udp_broadcast_target = ""
"#;
        let c_empty = Config::from_toml(toml_empty).unwrap();
        assert_eq!(
            c_empty.bot.resolved_udp_broadcast_target(),
            std::net::Ipv4Addr::BROADCAST
        );

        let toml_invalid = r#"
[bot]
udp_broadcast_target = "invalid_subnet"
"#;
        let c_invalid = Config::from_toml(toml_invalid).unwrap();
        assert_eq!(
            c_invalid.bot.resolved_udp_broadcast_target(),
            std::net::Ipv4Addr::BROADCAST
        );

        let c_default = Config::from_toml("").unwrap();
        assert_eq!(
            c_default.bot.resolved_udp_broadcast_target(),
            std::net::Ipv4Addr::BROADCAST
        );
    }
}
