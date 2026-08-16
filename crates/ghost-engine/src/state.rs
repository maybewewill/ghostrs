use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use ghost_net::{LinkError, PlayerLink};
use ghost_protocol::w3gs::{ActionBlock, outgoing};

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
    /// Present only when map downloads are enabled.
    pub data: Option<Arc<Vec<u8>>>,
    pub layout_style: u8,
    /// Raw w3i map options masked to the bits the bot cares about
    /// (MAPOPT_MELEE | MAPOPT_FIXEDPLAYERSETTINGS | MAPOPT_CUSTOMFORCES),
    /// mirroring GHost++ `CMap::m_MapOptions`.
    pub options: u32,
    pub map_type: String,
    pub matchmaking_category: String,
    pub stats_w3mmd_category: String,
    pub default_hcl: String,
    pub default_player_score: u32,
    pub loading_in_game: bool,
    pub local_path: String,
    pub max_slots: u32,
}

impl MapInfo {
    /// MAPOPT_FIXEDPLAYERSETTINGS, exactly like GHost++ `GetMapOptions()`.
    pub fn has_fixed_player_settings(&self) -> bool {
        self.options & crate::map::MAPOPT_FIXEDPLAYERSETTINGS != 0
    }

    /// MAPOPT_CUSTOMFORCES, exactly like GHost++ `GetMapOptions()`.
    pub fn has_custom_forces(&self) -> bool {
        self.options & crate::map::MAPOPT_CUSTOMFORCES != 0
    }

    /// MAPOBS_* observer setting, decoded from the game flags that
    /// `calculate_game_flags` baked it into.
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

    /// MAPFLAG_RANDOMRACES, decoded from the game flags that
    /// `calculate_game_flags` baked it into (0x0400_0000).
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
        if let Some(ref data) = self.data {
            if self.size != data.len() as u32 {
                return Err(format!(
                    "invalid map_size [{}]: size mismatch with actual map data [{}]",
                    self.size,
                    data.len()
                ));
            }
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
            stats_w3mmd_category: "default".into(),
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
    pub custom_slots: Option<Vec<ghost_protocol::w3gs::SlotInfo>>,
    pub replay_path: std::path::PathBuf,
    pub relay: Option<ghost_spectator::RelayHandle>,
    pub max_downloaders: u32,
    pub max_download_speed: u32,
    pub allow_downloads: u8,
    pub autokick_ping: u32,
    pub lc_pings: bool,
    pub spoof_checks: u8,
    pub require_spoof_checks: bool,
    pub host_port: u16,
    pub gproxy_reconnect_port: u16,
    pub store: Option<ghost_store::Store>,
    pub stat_string: Vec<u8>,
    pub event_tx: Option<tokio::sync::mpsc::Sender<crate::handle::GameEvent>>,
    pub lobby_time_limit: u32,
    pub load_in_game: bool,
    pub auto_save: bool,
    pub creator_name: String,
    pub creator_server: String,
    pub min_score: f64,
    pub max_score: f64,
    pub matchmaking: bool,
}

/// GHost++ steps the countdown every 500 ms (`game_base.cpp:707`), starting at 10 down to 1 (5.0s in total).

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

/// All mutable game state, owned exclusively by one actor task. No locks.
pub struct GameState {
    pub cfg: GameConfig,
    pub phase: GamePhase,
    pub slots: SlotTable,
    pub players: PlayerTable,
    pub tick: TickScheduler,
    /// Connections that have not sent REQ_JOIN yet.
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
    pub relay: Option<ghost_spectator::RelayHandle>,
    pub replay: Option<ghost_spectator::ReplayBody>,
    pub store: Option<ghost_store::Store>,
    pub jitter_histogram: [u64; 5],
    pub last_jitter_report: Instant,
    pub dota: Option<crate::stats_dota::StatsDotA>,
    pub game_over_time: Option<tokio::time::Instant>,
    pub w3mmd: Option<crate::stats_w3mmd::StatsW3MMD>,
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
    /// PID of the fake "bot" player shown in the lobby, or 255 when absent.
    /// Mirrors GHost++ `m_VirtualHostPID` (game_base.cpp:4702).
    pub virtual_host_pid: u8,
    pub started_loading_at: Option<Instant>,
    pub last_ping_at: Instant,
    pub last_lag_screen_reset: Instant,
    pub last_reserved_seen: Instant,
    pub start_players: usize,
    pub load_in_game: bool,
    pub auto_save: bool,
    pub mute_lobby: bool,
    pub local_admin_messages: bool,
    pub last_game_name: String,
    pub refresh_rehosted: bool,
    pub fake_player_pid: Option<u8>,
}

impl GameState {
    pub fn new(cfg: GameConfig) -> Self {
        let slots = if let Some(cs) = cfg.custom_slots.clone() {
            SlotTable::from_slots(cs)
        } else {
            SlotTable::new(cfg.num_slots)
        };
        let tick = TickScheduler::new(cfg.latency);
        let relay = cfg.relay.clone();
        let stat_string = if !cfg.stat_string.is_empty() {
            cfg.stat_string.clone()
        } else {
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
            raw.push(0);
            ghost_protocol::encode_statstring(&raw)
        };
        let mut replay = ghost_spectator::ReplayBody::new(255, &cfg.virtual_host_name);
        replay.set_game(&cfg.name, &stat_string, cfg.map.game_type);
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
            store: cfg.store.clone(),
            jitter_histogram: [0; 5],
            last_jitter_report: Instant::now(),
            dota: if cfg.map.map_type == "dota" || (cfg.map.map_type.is_empty() && cfg.name.to_lowercase().contains("dota")) {
                Some(crate::stats_dota::StatsDotA::new(cfg.name.clone()))
            } else {
                None
            },
            game_over_time: None,
            w3mmd: if cfg.map.map_type == "w3mmd" {
                Some(crate::stats_w3mmd::StatsW3MMD::new(
                    cfg.name.clone(),
                    if cfg.map.stats_w3mmd_category.is_empty() {
                        "default".into()
                    } else {
                        cfg.map.stats_w3mmd_category.clone()
                    },
                ))
            } else {
                None
            },
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
            load_in_game: cfg.load_in_game,
            auto_save: cfg.auto_save,
            mute_lobby: false,
            local_admin_messages: true,
            last_game_name: cfg.name.clone(),
            refresh_rehosted: false,
            fake_player_pid: None,
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

    /// Queues `bytes` for every seated player. Never awaits: a peer that cannot
    /// keep up is marked for removal rather than allowed to stall the tick.
    /// GProxy++ clients get every packet mirrored into their reconnect buffer
    /// first, and a dropped gproxy link enters the reconnect grace period
    /// (`disconnected_since`) instead of being removed immediately.
    pub fn broadcast(&mut self, bytes: Bytes) {
        for p in self.players.iter_mut() {
            if p.left.is_some() || p.virtual_host {
                continue;
            }
            if let Some(buf) = p.gproxy_buffer.as_mut() {
                buf.push(bytes.clone());
            }
            if self.load_in_game && !p.loaded && self.phase == GamePhase::Loading {
                p.load_in_game_data.push(bytes.clone());
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

    /// Marks a player as kicked. Mirrors GHost++ `OpenSlot(sid, kick=true)`
    /// (`game_base.cpp`): `SetDeleteMe` + `SetLeftReason` + `SetLeftCode`, and
    /// nothing on the wire beyond the usual PLAYERLEAVE_OTHERS that
    /// `reap_left_players` broadcasts afterwards.
    ///
    /// Deliberately does NOT send W3GS_HOST_KICK_PLAYER (0x1C): GHost++ only
    /// declares that id (`gameprotocol.h:78`) and has no `SEND_` function for
    /// it, so emitting it would be a divergence from the reference, not parity.
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
            let msg = if message.len() > 254 { &message[..254] } else { message };
            if let Ok(b) = outgoing::chat_from_host(from, &pids, 0x10, &[], msg) {
                self.broadcast(b);
            }
        } else {
            let msg = if message.len() > 127 { &message[..127] } else { message };
            let extra: [u8; 4] = [0, 0, 0, 0];
            if let Some(rep) = self.replay.as_mut() {
                rep.add_chat(from, 0x20, 0, msg);
            }
            if let Ok(b) = outgoing::chat_from_host(from, &pids, 0x20, &extra, msg) {
                self.broadcast(b);
            }
        }
    }

    /// Removes everyone marked as left and tells the rest. Called once per tick
    /// and after every batch of inbound frames, never mid-iteration.
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
            self.send_chat_all(&crate::lang::countdown_aborted());
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

    /// Seats a socket-less player so clients have a sender to attribute bot chat
    /// to, and so the lobby count matches GHost++. No-op when one already exists.
    pub fn create_virtual_host(&mut self) {
        if self.virtual_host_pid != 255 {
            return;
        }
        let Some(pid) = self.players.next_free_pid() else {
            return;
        };
        // The virtual host has no socket. `broadcast` and `reap_left_players`
        // both skip it, so nothing ever sends through this link; it exists only
        // to satisfy the Player type. Dropping the receiver is fine and must not
        // be worked around with a leak.
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

    /// Removes the virtual host, freeing its PID for a real player.
    pub fn delete_virtual_host(&mut self) {
        if self.virtual_host_pid == 255 {
            return;
        }
        let pid = self.virtual_host_pid;
        self.virtual_host_pid = 255;
        self.players.remove_pid(pid);
        // PLAYERLEAVE_LOBBY == 13, matching game_base.cpp:4721.
        self.broadcast(outgoing::player_leave_others(pid, ghost_protocol::w3gs::ids::PLAYERLEAVE_LOBBY));
    }
}

