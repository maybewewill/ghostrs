use std::collections::VecDeque;
use std::time::Instant;

use ghost_net::PlayerLink;

use crate::slots::SlotTable;

/// How many recent ping samples feed the average shown by `!ping`.
const PING_HISTORY: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameMatch {
    None,
    Ambiguous(usize),
}

#[derive(Debug)]
pub struct Player {
    pub pid: u8,
    pub name: String,
    pub conn_id: u64,
    pub link: PlayerLink,
    pub external_ip: [u8; 4],
    pub internal_ip: [u8; 4],
    /// How many action ticks this player has confirmed via keepalive.
    pub sync_counter: u32,
    pub lagging: bool,
    pub started_lagging: Option<Instant>,
    pub loaded: bool,
    /// 0..100 while downloading the map, 255 when not downloading.
    pub download_status: u8,
    pub ping_history: VecDeque<u32>,
    pub reconnect_key: u32,
    pub gproxy: bool,
    /// Set once the player is scheduled for removal; carries the reason.
    pub left: Option<String>,
}

impl Player {
    pub fn new(pid: u8, name: String, conn_id: u64, link: PlayerLink) -> Self {
        Self {
            pid,
            name,
            conn_id,
            link,
            external_ip: [0; 4],
            internal_ip: [0; 4],
            sync_counter: 0,
            lagging: false,
            started_lagging: None,
            loaded: false,
            download_status: 255,
            ping_history: VecDeque::with_capacity(PING_HISTORY),
            reconnect_key: 0,
            gproxy: false,
            left: None,
        }
    }

    pub fn record_ping(&mut self, ping_ms: u32) {
        if self.ping_history.len() == PING_HISTORY {
            self.ping_history.pop_front();
        }
        self.ping_history.push_back(ping_ms);
    }

    pub fn average_ping(&self) -> Option<u32> {
        if self.ping_history.is_empty() {
            return None;
        }
        let sum: u32 = self.ping_history.iter().sum();
        Some(sum / self.ping_history.len() as u32)
    }
}

#[derive(Debug, Default)]
pub struct PlayerTable {
    players: Vec<Player>,
}

impl PlayerTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, p: Player) {
        self.players.push(p);
    }

    pub fn remove_pid(&mut self, pid: u8) -> Option<Player> {
        let i = self.players.iter().position(|p| p.pid == pid)?;
        Some(self.players.remove(i))
    }

    pub fn by_pid(&self, pid: u8) -> Option<&Player> {
        self.players.iter().find(|p| p.pid == pid)
    }

    pub fn by_pid_mut(&mut self, pid: u8) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.pid == pid)
    }

    pub fn by_conn(&self, conn_id: u64) -> Option<&Player> {
        self.players.iter().find(|p| p.conn_id == conn_id)
    }

    pub fn by_conn_mut(&mut self, conn_id: u64) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.conn_id == conn_id)
    }

    /// Exact match wins; otherwise a unique case-insensitive prefix match.
    pub fn by_name_partial(&self, needle: &str) -> Result<&Player, NameMatch> {
        if let Some(p) = self.players.iter().find(|p| p.name == needle) {
            return Ok(p);
        }
        let lower = needle.to_lowercase();
        let hits: Vec<&Player> = self
            .players
            .iter()
            .filter(|p| p.name.to_lowercase().starts_with(&lower))
            .collect();
        match hits.len() {
            0 => Err(NameMatch::None),
            1 => Ok(hits[0]),
            n => Err(NameMatch::Ambiguous(n)),
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Player> {
        self.players.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Player> {
        self.players.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// PIDs run 1..=254; 255 is reserved for the virtual host player.
    pub fn next_free_pid(&self) -> Option<u8> {
        (1u8..=254).find(|c| !self.players.iter().any(|p| p.pid == *c))
    }

    pub fn next_free_colour(&self, slots: &SlotTable) -> u8 {
        let taken: Vec<u8> = slots.as_wire().iter().map(|s| s.colour).collect();
        (0u8..=11).find(|c| !taken.contains(c)).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn test_player(pid: u8, name: &str) -> Player {
        let (tx, _rx) = mpsc::channel(8);
        Player::new(pid, name.to_string(), 0, ghost_net::PlayerLink::for_test(tx))
    }

    #[test]
    fn pids_are_allocated_from_the_lowest_free_value() {
        let mut t = PlayerTable::new();
        assert_eq!(t.next_free_pid(), Some(1));
        t.insert(test_player(1, "a"));
        t.insert(test_player(3, "b"));
        assert_eq!(t.next_free_pid(), Some(2));
    }

    #[test]
    fn partial_name_lookup_reports_ambiguity() {
        let mut t = PlayerTable::new();
        t.insert(test_player(1, "Slash"));
        t.insert(test_player(2, "Slasher"));
        t.insert(test_player(3, "Other"));

        assert_eq!(t.by_name_partial("Oth").unwrap().pid, 3);
        assert!(matches!(t.by_name_partial("Sla"), Err(NameMatch::Ambiguous(2))));
        assert!(matches!(t.by_name_partial("zzz"), Err(NameMatch::None)));
        // An exact match wins even when it is a prefix of another name.
        assert_eq!(t.by_name_partial("Slash").unwrap().pid, 1);
    }

    #[test]
    fn average_ping_ignores_an_empty_history() {
        let mut p = test_player(1, "a");
        assert_eq!(p.average_ping(), None);
        p.ping_history.push_back(40);
        p.ping_history.push_back(60);
        assert_eq!(p.average_ping(), Some(50));
    }

    #[test]
    fn removing_a_player_frees_the_pid() {
        let mut t = PlayerTable::new();
        t.insert(test_player(1, "a"));
        assert_eq!(t.next_free_pid(), Some(2));
        assert!(t.remove_pid(1).is_some());
        assert_eq!(t.next_free_pid(), Some(1));
        assert!(t.remove_pid(1).is_none());
    }
}
