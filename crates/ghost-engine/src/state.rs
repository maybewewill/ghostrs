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
}

impl MapInfo {
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
}

/// GHost++ steps the countdown every 500 ms (`game_base.cpp:707`), so five steps
/// "5 . . . 4 . . . 3 . . . 2 . . . 1" take 2.5 s in total — not 5 s, and not 75 ms.
pub const COUNTDOWN_STEP: Duration = Duration::from_millis(500);
pub const COUNTDOWN_STEPS: u8 = 5;
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
    pub relay: Option<ghost_spectator::RelayHandle>,
    pub replay: Option<ghost_spectator::ReplayBody>,
    pub jitter_histogram: [u64; 5],
    pub last_jitter_report: Instant,
    pub dota: Option<crate::stats_dota::StatsDotA>,
    pub game_over_time: Option<tokio::time::Instant>,
    pub w3mmd: Option<crate::stats_w3mmd::StatsW3MMD>,
    pub hcl: Option<String>,
    pub muted_all: bool,
    pub draw_votes: Vec<u8>,
    pub start_votes: Vec<u8>,
    pub last_player_left: Option<(String, String)>,
    pub holds: std::collections::HashMap<u8, String>,
    /// PID of the fake "bot" player shown in the lobby, or 255 when absent.
    /// Mirrors GHost++ `m_VirtualHostPID` (game_base.cpp:4702).
    pub virtual_host_pid: u8,
    pub started_loading_at: Option<Instant>,
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
        let mut replay = ghost_spectator::ReplayBody::new(1, &cfg.virtual_host_name);
        replay.set_game(&cfg.name, &[0u8; 4], cfg.map.game_type);
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
            lagging: false,
            finished: false,
            downloads: Vec::new(),
            relay,
            replay: Some(replay),
            jitter_histogram: [0; 5],
            last_jitter_report: Instant::now(),
            dota: Some(crate::stats_dota::StatsDotA::new(cfg.name.clone())),
            game_over_time: None,
            w3mmd: Some(crate::stats_w3mmd::StatsW3MMD::new(
                cfg.name.clone(),
                "default".into(),
            )),
            hcl: crate::hcl::Hcl::parse_from_gamename(&cfg.name),
            muted_all: false,
            draw_votes: Vec::new(),
            start_votes: Vec::new(),
            last_player_left: None,
            holds: std::collections::HashMap::new(),
            virtual_host_pid: 255,
            started_loading_at: None,
            cfg,
        }
    }

    pub fn add_conn(&mut self, conn_id: u64, link: PlayerLink, external_ip: [u8; 4]) {
        self.pending.push((conn_id, link, external_ip));
    }

    /// Queues `bytes` for every seated player. Never awaits: a peer that cannot
    /// keep up is marked for removal rather than allowed to stall the tick.
    pub fn broadcast(&mut self, bytes: Bytes) {
        for p in self.players.iter_mut() {
            if p.left.is_some() || p.virtual_host {
                continue;
            }
            if let Some(buf) = p.gproxy_buffer.as_mut() {
                buf.push(bytes.clone());
            }
            if p.disconnected_since.is_some() {
                continue;
            }
            match p.link.try_send(bytes.clone()) {
                Ok(()) => {}
                Err(LinkError::Backpressure) => {
                    tracing::warn!(pid = p.pid, name = %p.name, "write queue full, dropping player");
                    p.left = Some("lagged out (write queue full)".into());
                }
                Err(LinkError::Closed) => {
                    if p.gproxy {
                        p.disconnected_since = Some(Instant::now());
                    } else {
                        p.left = Some("connection closed".into());
                    }
                }
            }
        }
    }

    pub fn send_to(&mut self, pid: u8, bytes: Bytes) {
        if let Some(p) = self.players.by_pid_mut(pid)
            && p.link.try_send(bytes).is_err()
        {
            p.left = Some("connection closed".into());
        }
    }

    pub fn send_chat_all(&mut self, message: &str) {
        let flag = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
            0x10
        } else {
            0x20
        };
        let extra: &[u8] = if flag == 0x20 { &[0, 0, 0, 0] } else { &[] };
        let from = if self.virtual_host_pid != 255 {
            self.virtual_host_pid
        } else {
            255
        };
        let pids: Vec<u8> = self
            .players
            .iter()
            .filter(|p| !p.virtual_host)
            .map(|p| p.pid)
            .collect();
        if pids.is_empty() {
            return;
        }
        if let Some(rep) = self.replay.as_mut() {
            rep.add_chat(from, flag, 0, message);
        }
        match outgoing::chat_from_host(from, &pids, flag, extra, message) {
            Ok(b) => self.broadcast(b),
            Err(e) => tracing::warn!(error = %e, "failed to build chat packet"),
        }
    }

    /// Removes everyone marked as left and tells the rest. Called once per tick
    /// and after every batch of inbound frames, never mid-iteration.
    pub fn reap_left_players(&mut self) {
        let gone: Vec<(u8, String)> = self
            .players
            .iter()
            .filter(|p| !p.virtual_host)
            .filter_map(|p| p.left.as_ref().map(|r| (p.pid, r.clone())))
            .collect();

        if gone.is_empty() {
            return;
        }

        if matches!(self.phase, GamePhase::Countdown { .. }) {
            tracing::info!(game = %self.cfg.name, "player left during countdown, aborting to lobby");
            self.phase = GamePhase::Lobby;
            self.send_chat_all(&crate::lang::countdown_aborted());
        }

        for (pid, reason) in gone {
            if let Some(rep) = self.replay.as_mut() {
                rep.add_leaver(pid, 13, 0); // 13 = PLAYERLEAVE_LOBBY / disconnect
            }
            self.players.remove_pid(pid);
            self.slots.release(pid);
            tracing::info!(game = %self.cfg.name, pid, %reason, "player left");
            self.broadcast(outgoing::player_leave_others(pid, 13));
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
            Err(e) => tracing::warn!(error = %e, "failed to build slot info"),
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
        self.broadcast(outgoing::player_leave_others(pid, 13));
    }
}
