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
}

impl MapInfo {
    #[cfg(test)]
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePhase {
    Lobby,
    Countdown { remaining: u8 },
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
}

impl GameState {
    pub fn new(cfg: GameConfig) -> Self {
        let slots = SlotTable::new(cfg.num_slots);
        let tick = TickScheduler::new(cfg.latency);
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
            if p.left.is_some() {
                continue;
            }
            match p.link.try_send(bytes.clone()) {
                Ok(()) => {}
                Err(LinkError::Backpressure) => {
                    tracing::warn!(pid = p.pid, name = %p.name, "write queue full, dropping player");
                    p.left = Some("lagged out (write queue full)".into());
                }
                Err(LinkError::Closed) => {
                    p.left = Some("connection closed".into());
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
        let pids: Vec<u8> = self.players.iter().map(|p| p.pid).collect();
        if pids.is_empty() {
            return;
        }
        let flag = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
            0x10
        } else {
            0x20
        };
        let extra: &[u8] = if flag == 0x20 { &[0, 0, 0, 0] } else { &[] };
        match outgoing::chat_from_host(255, &pids, flag, extra, message) {
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
            .filter_map(|p| p.left.as_ref().map(|r| (p.pid, r.clone())))
            .collect();

        for (pid, reason) in gone {
            self.players.remove_pid(pid);
            self.slots.release(pid);
            tracing::info!(game = %self.cfg.name, pid, %reason, "player left");
            self.broadcast(outgoing::player_leave_others(pid, 13));
            if matches!(self.phase, GamePhase::Lobby) {
                self.send_all_slot_info();
            }
        }
    }

    pub fn send_all_slot_info(&mut self) {
        match outgoing::slot_info(
            self.slots.as_wire(),
            self.random_seed,
            self.cfg.map.num_teams,
            self.cfg.map.num_players,
        ) {
            Ok(b) => self.broadcast(b),
            Err(e) => tracing::warn!(error = %e, "failed to build slot info"),
        }
    }
}

