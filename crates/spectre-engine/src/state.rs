use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use spectre_net::{LinkError, PlayerLink};
use spectre_protocol::w3gs::{ActionBlock, outgoing};

use crate::players::PlayerTable;
use crate::slots::SlotTable;
use crate::tick::TickScheduler;

#[derive(Debug, Clone)]
pub struct MapInfo {
    pub path: String,
    pub size: u32,
    pub info: u32,
    pub crc: u32,
    pub sha1: [u8; 20],
    pub num_players: u8,
    pub num_teams: u8,
    pub width: u16,
    pub height: u16,
    pub game_type: u32,
    pub flags: u32,
    pub data: Option<Arc<Vec<u8>>>,
    pub layout_style: u8,
    pub options: u32,
    pub map_type: String,
    pub matchmaking_category: String,
    pub default_hcl: String,
    pub default_player_score: u32,
    pub loading_in_game: bool,
    pub local_path: String,
    pub max_slots: u32,
}

impl MapInfo {
    pub fn has_fixed_player_settings(&self) -> bool {
        self.options & crate::map::MAPOPT_FIXEDPLAYERSETTINGS != 0
    }

    pub fn has_custom_forces(&self) -> bool {
        self.options & crate::map::MAPOPT_CUSTOMFORCES != 0
    }

    pub fn observers(&self) -> u8 {
        use crate::map::{MAPOBS_ALLOWED, MAPOBS_NONE, MAPOBS_ONDEFEAT, MAPOBS_REFEREES};
        if self.flags & 0x4000_0000 != 0 {
            MAPOBS_REFEREES
        } else if self.flags & 0x0000_3000 == 0x0000_3000 {
            MAPOBS_ALLOWED
        } else if self.flags & 0x0000_2000 != 0 {
            MAPOBS_ONDEFEAT
        } else {
            MAPOBS_NONE
        }
    }

    pub fn has_random_races(&self) -> bool {
        self.flags & 0x0400_0000 != 0
    }

    pub fn check_valid(&self) -> Result<(), String> {
        if self.path.is_empty() || self.path.len() > 53 {
            return Err(format!(
                "invalid map_path [{}]: must not be empty and <= 53 characters",
                self.path
            ));
        }
        if self.width == 0 || self.height == 0 {
            return Err("invalid map dimensions: width and height must be > 0".into());
        }
        if let Some(ref data) = self.data
            && self.size != data.len() as u32
        {
            return Err(format!(
                "invalid map_size [{}]: size mismatch with actual map data [{}]",
                self.size,
                data.len()
            ));
        }
        Ok(())
    }

    pub fn test_default() -> Self {
        Self {
            path: "Maps\\Download\\test.w3x".into(),
            size: 1000,
            info: 1,
            crc: 0x1234_5678,
            sha1: [0; 20],
            num_players: 12,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0,
            data: None,
            layout_style: 0,
            options: 0,
            map_type: "dota".into(),
            matchmaking_category: String::new(),
            default_hcl: String::new(),
            default_player_score: 1000,
            loading_in_game: false,
            local_path: String::new(),
            max_slots: 24,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GameConfig {
    pub name: String,
    pub owner: String,
    pub host_counter: u32,
    pub num_slots: usize,
    pub latency: Duration,
    pub sync_limit: u32,
    pub map: MapInfo,
    pub virtual_host_name: String,
    pub reconnect_wait: Duration,
    pub custom_slots: Option<Vec<spectre_protocol::w3gs::SlotInfo>>,
    pub replay_path: std::path::PathBuf,
    pub relay: Option<spectre_spectator::RelayHandle>,
    pub max_downloaders: u32,
    pub max_download_speed: u32,
    pub allow_downloads: u8,
    pub autokick_ping: u32,
    pub lc_pings: bool,
    pub spoof_checks: u8,
    pub require_spoof_checks: bool,
    pub host_port: u16,
    pub gproxy_reconnect_port: u16,
    pub store: Option<spectre_store::Store>,
    pub stat_string: Vec<u8>,
    pub event_tx: Option<tokio::sync::mpsc::Sender<crate::handle::GameEvent>>,
    pub lobby_time_limit: u32,
    pub creator_name: String,
    pub creator_server: String,
    pub min_score: f64,
    pub max_score: f64,
    pub matchmaking: bool,
}

pub const COUNTDOWN_STEP: Duration = Duration::from_millis(500);
pub const COUNTDOWN_STEPS: u8 = 10;
pub const COUNTDOWN_TOTAL: Duration = Duration::from_millis(500 * COUNTDOWN_STEPS as u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePhase {
    Lobby,
    Countdown {
        started_at: Instant,
        total_duration: Duration,
        last_announced_step: u8,
    },
    Loading,
    Playing,
    Over,
}

pub struct GameState {
    pub cfg: GameConfig,
    pub phase: GamePhase,
    pub slots: SlotTable,
    pub players: PlayerTable,
    pub tick: TickScheduler,
    pub pending: Vec<(u64, PlayerLink, [u8; 4])>,
    pub actions: Vec<ActionBlock>,
    pub sync_counter: u32,
    pub game_ticks: u32,
    pub random_seed: u32,
    pub last_tick_at: Option<Instant>,
    pub created_at: Instant,
    pub lagging: bool,
    pub finished: bool,
    pub downloads: Vec<crate::mapxfer::Download>,
    pub download_counter: usize,
    pub last_download_counter_reset: Instant,
    pub last_download_tick: Instant,
    pub relay: Option<spectre_spectator::RelayHandle>,
    pub replay: Option<spectre_spectator::ReplayBody>,
    pub dotatv: Option<std::sync::Arc<spectre_spectator::DotaTvShared>>,
    pub dotatv_prologue_sent: bool,
    pub store: Option<spectre_store::Store>,
    pub jitter_histogram: [u64; 5],
    pub last_jitter_report: Instant,
    pub dota: Option<crate::stats_dota::StatsDotA>,
    pub game_over_time: Option<tokio::time::Instant>,
    pub hcl: Option<String>,
    pub muted_all: bool,
    pub locked: bool,
    pub draw_votes: Vec<u8>,
    pub start_votes: Vec<u8>,
    pub last_player_left: Option<(String, String)>,
    pub last_player_left_time: Option<Instant>,
    pub holds: std::collections::HashMap<u8, String>,
    pub votekick_target: Option<u8>,
    pub votekick_votes: Vec<u8>,
    pub autostart_players: Option<usize>,
    pub announce_message: Option<String>,
    pub announce_interval: Duration,
    pub last_announce_time: Option<Instant>,
    pub virtual_host_pid: u8,
    pub started_loading_at: Option<Instant>,
    pub last_ping_at: Instant,
    pub last_lag_screen_reset: Instant,
    pub last_reserved_seen: Instant,
    pub start_players: usize,
    pub mute_lobby: bool,
    pub local_admin_messages: bool,
    pub last_game_name: String,
    pub refresh_rehosted: bool,
    pub fake_player_pid: Option<u8>,
    pub full_history: crate::full_history::FullHistory,
    pub pending_full: std::collections::HashMap<u64, (u8, u32)>,
}

impl GameState {
    fn build_replay_stat_string(cfg: &GameConfig) -> Vec<u8> {
        let mut raw = Vec::with_capacity(44 + cfg.map.path.len() + cfg.virtual_host_name.len());
        raw.extend_from_slice(&cfg.map.flags.to_le_bytes());
        raw.push(0);
        raw.extend_from_slice(&cfg.map.width.to_le_bytes());
        raw.extend_from_slice(&cfg.map.height.to_le_bytes());
        raw.extend_from_slice(&cfg.map.crc.to_le_bytes());
        raw.extend_from_slice(cfg.map.path.as_bytes());
        raw.push(0);
        raw.extend_from_slice(cfg.virtual_host_name.as_bytes());
        raw.push(0);
        spectre_protocol::encode_statstring(&raw)
    }

    pub fn new(cfg: GameConfig) -> Self {
        let slots = if let Some(cs) = cfg.custom_slots.clone() {
            SlotTable::from_slots(cs)
        } else {
            SlotTable::new(cfg.num_slots)
        };
        let tick = TickScheduler::new(cfg.latency);
        let relay = cfg.relay.clone();
        let replay_stat_string = Self::build_replay_stat_string(&cfg);
        let mut replay = spectre_spectator::ReplayBody::new(255, &cfg.virtual_host_name);
        replay.set_game(&cfg.name, &replay_stat_string, cfg.map.game_type);
        Self {
            phase: GamePhase::Lobby,
            slots,
            players: PlayerTable::new(),
            tick,
            pending: Vec::new(),
            actions: Vec::new(),
            sync_counter: 0,
            game_ticks: 0,
            random_seed: rand::random(),
            last_tick_at: None,
            created_at: Instant::now(),
            last_ping_at: Instant::now(),
            last_lag_screen_reset: Instant::now(),
            last_reserved_seen: Instant::now(),
            start_players: 0,
            lagging: false,
            finished: false,
            downloads: Vec::new(),
            download_counter: 0,
            last_download_counter_reset: Instant::now(),
            last_download_tick: Instant::now(),
            relay,
            replay: Some(replay),
            dotatv: None,
            dotatv_prologue_sent: false,
            store: cfg.store.clone(),
            jitter_histogram: [0; 5],
            last_jitter_report: Instant::now(),
            dota: if cfg.map.map_type == "dota"
                || (cfg.map.map_type.is_empty() && cfg.name.to_lowercase().contains("dota"))
            {
                Some(crate::stats_dota::StatsDotA::new(cfg.name.clone()))
            } else {
                None
            },
            game_over_time: None,
            hcl: crate::hcl::Hcl::parse_from_gamename(&cfg.name).or_else(|| {
                if !cfg.map.default_hcl.is_empty() {
                    Some(cfg.map.default_hcl.clone())
                } else {
                    None
                }
            }),
            muted_all: false,
            locked: false,
            draw_votes: Vec::new(),
            start_votes: Vec::new(),
            last_player_left: None,
            last_player_left_time: None,
            holds: std::collections::HashMap::new(),
            votekick_target: None,
            votekick_votes: Vec::new(),
            autostart_players: None,
            announce_message: None,
            announce_interval: Duration::ZERO,
            last_announce_time: None,
            virtual_host_pid: 255,
            started_loading_at: None,
            mute_lobby: false,
            local_admin_messages: true,
            last_game_name: cfg.name.clone(),
            refresh_rehosted: false,
            fake_player_pid: None,
            full_history: crate::full_history::FullHistory::new(),
            pending_full: std::collections::HashMap::new(),
            cfg,
        }
    }

    pub fn add_conn(&mut self, conn_id: u64, link: PlayerLink, external_ip: [u8; 4]) {
        self.pending.push((conn_id, link, external_ip));
    }

    pub fn host_pid(&self) -> u8 {
        if self.virtual_host_pid != 255 {
            return self.virtual_host_pid;
        }
        for p in self.players.iter() {
            if p.left.is_none() {
                return p.pid;
            }
        }
        255
    }

    pub const MAX_CONSECUTIVE_DROPS: u32 = 100;

    pub fn broadcast(&mut self, bytes: Bytes) {
        if matches!(self.phase, GamePhase::Playing) {
            self.full_history.push(bytes.clone());
        }
        for p in self.players.iter_mut() {
            if p.left.is_some() || p.virtual_host {
                continue;
            }
            if let Some(buf) = p.gproxy_buffer.as_mut() {
                buf.push(bytes.clone());
            }
            if p.catchup_cursor.is_some() {
                continue;
            }
            if p.disconnected_since.is_some() {
                continue;
            }
            match p.link.try_send(bytes.clone()) {
                Ok(()) => {
                    p.consecutive_send_failures = 0;
                }
                Err(LinkError::Backpressure) => {
                    p.consecutive_send_failures = p.consecutive_send_failures.saturating_add(1);
                    if p.consecutive_send_failures >= Self::MAX_CONSECUTIVE_DROPS {
                        tracing::warn!(pid = p.pid, name = %p.name, "write queue full past threshold, dropping player");
                        p.left = Some("lagged out (write queue full)".into());
                    }
                }
                Err(LinkError::Closed) => {
                    if p.gproxy {
                        if p.disconnected_since.is_none() {
                            p.disconnected_since = Some(Instant::now());
                        }
                    } else {
                        p.left = Some("connection closed".into());
                    }
                }
            }
        }
    }

    /// Подаёт FULL-переджойнеру накопленную историю курсором, уважая backpressure.
    /// Когда курсор достигает конца лога — переключает игрока на живой эфир.
    pub fn pump_rejoin_catchup(&mut self) {
        let next_seq = self.full_history.next_seq();
        let first_seq = self.full_history.first_seq();
        for p in self.players.iter_mut() {
            let Some(cursor) = p.catchup_cursor else {
                continue;
            };
            if p.left.is_some() {
                p.catchup_cursor = None;
                continue;
            }
            // Догоняющий отстал: нужный пакет уже вытеснен из лога — молча пропустить
            // нельзя (это тот самый ложный десинк). Явно роняем с понятной причиной.
            if cursor < first_seq {
                tracing::warn!(
                    game = %self.cfg.name, pid = p.pid, cursor, first_seq,
                    "FULL rejoin catch-up fell behind history eviction, dropping rejoiner"
                );
                p.left = Some("catch-up fell behind history eviction".into());
                p.catchup_cursor = None;
                continue;
            }
            let mut cur = cursor;
            let pending = self.full_history.snapshot_from_seq(cur);
            for pkt in pending {
                match p.link.try_send(pkt) {
                    Ok(()) => {
                        cur += 1;
                        p.consecutive_send_failures = 0;
                    }
                    Err(spectre_net::LinkError::Backpressure) => break,
                    Err(spectre_net::LinkError::Closed) => {
                        if p.gproxy && p.disconnected_since.is_none() {
                            p.disconnected_since = Some(std::time::Instant::now());
                        } else {
                            p.left = Some("connection closed during catch-up".into());
                        }
                        p.catchup_cursor = None;
                        break;
                    }
                }
            }
            if p.catchup_cursor.is_some() {
                if cur >= next_seq {
                    p.catchup_cursor = None;
                    p.catching_up = true;
                } else {
                    p.catchup_cursor = Some(cur);
                }
            }
        }
    }

    pub fn send_to(&mut self, pid: u8, bytes: Bytes) {
        let Some(p) = self.players.by_pid_mut(pid) else {
            return;
        };
        if p.left.is_some() {
            return;
        }
        if let Some(buf) = p.gproxy_buffer.as_mut() {
            buf.push(bytes.clone());
        }
        if p.disconnected_since.is_some() {
            return;
        }
        match p.link.try_send(bytes) {
            Ok(()) => {
                p.consecutive_send_failures = 0;
            }
            Err(LinkError::Backpressure) => {
                p.consecutive_send_failures = p.consecutive_send_failures.saturating_add(1);
                if p.consecutive_send_failures >= Self::MAX_CONSECUTIVE_DROPS {
                    tracing::warn!(pid = p.pid, name = %p.name, "write queue full past threshold, dropping player");
                    p.left = Some("lagged out (write queue full)".into());
                }
            }
            Err(LinkError::Closed) => {
                if p.gproxy {
                    if p.disconnected_since.is_none() {
                        p.disconnected_since = Some(Instant::now());
                    }
                } else {
                    p.left = Some("connection closed".into());
                }
            }
        }
    }

    pub fn kick_player(&mut self, pid: u8, reason: &str, left_code: u32) {
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.left = Some(reason.to_string());
            p.left_code = left_code;
        }
    }

    pub fn send_chat_all(&mut self, message: &str) {
        let from = self.host_pid();
        if from == 255 {
            return;
        }
        let pids: Vec<u8> = self
            .players
            .iter()
            .filter(|p| !p.virtual_host && p.left.is_none())
            .map(|p| p.pid)
            .collect();
        if pids.is_empty() {
            return;
        }
        if let Some(r) = &self.relay {
            r.send_chat(&self.cfg.virtual_host_name, message);
        }
        if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
            let msg = if message.len() > 254 {
                &message[..254]
            } else {
                message
            };
            if let Ok(b) = outgoing::chat_from_host(from, &pids, 0x10, &[], msg) {
                self.broadcast(b);
            }
        } else {
            let msg = if message.len() > 127 {
                &message[..127]
            } else {
                message
            };
            let extra: [u8; 4] = [0, 0, 0, 0];
            if let Some(rep) = self.replay.as_mut() {
                rep.add_chat(from, 0x20, 0, msg);
            }
            if let Ok(b) = outgoing::chat_from_host(from, &pids, 0x20, &extra, msg) {
                self.broadcast(b);
            }
        }
    }

    pub fn reap_left_players(&mut self) {
        let gone: Vec<(u8, String, u32)> = self
            .players
            .iter()
            .filter(|p| !p.virtual_host)
            .filter_map(|p| p.left.as_ref().map(|r| (p.pid, r.clone(), p.left_code)))
            .collect();

        if gone.is_empty() {
            return;
        }

        if matches!(self.phase, GamePhase::Countdown { .. }) {
            tracing::info!(game = %self.cfg.name, "player left during countdown, aborting to lobby");
            self.phase = GamePhase::Lobby;
            self.send_chat_all("Countdown aborted.");
        }

        self.last_player_left_time = Some(Instant::now());

        for (pid, reason, left_code) in gone {
            if let Some(p) = self.players.by_pid(pid) {
                let ip_str = format!(
                    "{}.{}.{}.{}",
                    p.external_ip[0], p.external_ip[1], p.external_ip[2], p.external_ip[3]
                );
                self.last_player_left = Some((p.name.clone(), ip_str));
            }
            if let Some(rep) = self.replay.as_mut() {
                if matches!(self.phase, GamePhase::Loading) {
                    rep.add_leaver_loading(pid, 1, left_code);
                } else {
                    rep.add_leaver(pid, 1, left_code);
                }
            }
            self.players.remove_pid(pid);
            self.slots.release(pid);
            tracing::info!(game = %self.cfg.name, pid, %reason, left_code, "player left");
            self.broadcast(outgoing::player_leave_others(pid, left_code));
            if matches!(self.phase, GamePhase::Lobby) {
                self.send_all_slot_info();
            }
        }

        if matches!(self.phase, GamePhase::Loading)
            && !self.players.is_empty()
            && self
                .players
                .iter()
                .filter(|p| !p.virtual_host)
                .all(|p| p.loaded)
        {
            tracing::info!(game = %self.cfg.name, "all remaining players loaded after leaver, starting game");
            self.begin_playing();
        }
    }

    pub fn send_all_slot_info(&mut self) {
        match outgoing::slot_info(
            self.slots.as_wire(),
            self.random_seed,
            self.cfg.map.layout_style,
            self.cfg.map.num_players,
        ) {
            Ok(b) => self.broadcast(b),
            Err(e) => tracing::warn!(error = %e, "failed to build slot_info packet"),
        }
    }

    pub fn record_jitter(&mut self, jitter: Duration) {
        let ms = jitter.as_millis();
        if ms < 1 {
            self.jitter_histogram[0] += 1;
        } else if ms < 2 {
            self.jitter_histogram[1] += 1;
        } else if ms < 5 {
            self.jitter_histogram[2] += 1;
        } else if ms < 20 {
            self.jitter_histogram[3] += 1;
        } else {
            self.jitter_histogram[4] += 1;
        }
        if self.last_jitter_report.elapsed() >= Duration::from_secs(60) {
            tracing::info!(
                game = %self.cfg.name,
                lt_1ms = self.jitter_histogram[0],
                lt_2ms = self.jitter_histogram[1],
                lt_5ms = self.jitter_histogram[2],
                lt_20ms = self.jitter_histogram[3],
                gte_20ms = self.jitter_histogram[4],
                "tick jitter summary"
            );
            self.last_jitter_report = Instant::now();
        }
    }

    pub fn create_virtual_host(&mut self) {
        if self.virtual_host_pid != 255 {
            return;
        }
        let Some(pid) = self.players.next_free_pid() else {
            return;
        };

        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let mut p = crate::players::Player::new(
            pid,
            self.cfg.virtual_host_name.clone(),
            u64::MAX,
            PlayerLink::for_test(tx),
        );
        p.virtual_host = true;
        p.loaded = true;
        self.virtual_host_pid = pid;

        let ip = [0u8; 4];
        if let Ok(b) = outgoing::player_info(pid, &self.cfg.virtual_host_name, ip, ip) {
            self.broadcast(b);
        }
        self.players.insert(p);
    }

    pub fn delete_virtual_host(&mut self) {
        if self.virtual_host_pid == 255 {
            return;
        }
        let pid = self.virtual_host_pid;
        self.virtual_host_pid = 255;
        self.players.remove_pid(pid);

        self.broadcast(outgoing::player_leave_others(
            pid,
            spectre_protocol::w3gs::ids::PLAYERLEAVE_LOBBY,
        ));
    }
}

#[cfg(test)]
mod full_history_recording_tests {
    use super::*;
    use crate::actor::tests_support::seated_game;

    #[test]
    fn lobby_broadcasts_are_not_recorded() {
        let (mut st, _rxs) = seated_game(1);
        st.broadcast(Bytes::from_static(&[0xF7, 0x0F, 0x04, 0x00]));
        assert_eq!(
            st.full_history.len(),
            0,
            "lobby packets must not enter FullHistory"
        );
    }

    #[test]
    fn playing_broadcasts_are_recorded_byte_identical() {
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();
        let pkt = Bytes::from_static(&[0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00]);
        st.broadcast(pkt.clone());
        assert_eq!(st.full_history.len(), 1);
        assert_eq!(st.full_history.snapshot_from_seq(0)[0], pkt);
    }

    #[test]
    fn history_survives_gproxy_buffer_eviction() {
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.players.by_pid_mut(1).unwrap().gproxy_buffer =
            Some(crate::gproxy::GProxyBuffer::new(500));
        for i in 0..600u32 {
            st.broadcast(Bytes::from(i.to_le_bytes().to_vec()));
        }
        // per-player GProxyBuffer(500) вытеснил префикс, но глобальный лог держит всё
        assert_eq!(st.full_history.len(), 600);
    }
}
