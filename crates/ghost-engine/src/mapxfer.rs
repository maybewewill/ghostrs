use std::time::Instant;

use bytes::Bytes;
use ghost_protocol::w3gs::{incoming, incoming::MapSizeReport, outgoing};

use crate::state::{GamePhase, GameState};

/// Wire chunk size used by Warcraft III map downloads.
pub const MAP_CHUNK: usize = 1442;
/// Chunks sent per player per tick. Bounds how much of the tick budget map
/// downloads may consume; at 100 ms that is ~144 KB/s per downloader.
pub const MAX_PARTS_PER_TICK: usize = 10;

#[derive(Debug, Clone)]
pub struct Download {
    pub pid: u8,
    pub sent_upto: u32,
    pub acked_upto: u32,
    pub started: Instant,
}

impl Download {
    pub fn new(pid: u8) -> Self {
        Self {
            pid,
            sent_upto: 0,
            acked_upto: 0,
            started: Instant::now(),
        }
    }
}

impl GameState {
    pub fn handle_map_size(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        let report = match MapSizeReport::decode(payload) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(conn_id, error = %e, "malformed map size report");
                return;
            }
        };

        if report.map_size >= self.cfg.map.size {
            if let Some(p) = self.players.by_pid_mut(pid) {
                p.download_status = 100;
            }
            return;
        }
        if self.cfg.map.data.is_none() || self.cfg.allow_downloads == 0 {
            tracing::info!(
                pid,
                "player lacks the map and downloads are disabled, dropping"
            );
            self.kick_player(
                pid,
                "lacks map and downloads are disabled",
                ghost_protocol::w3gs::ids::PLAYERLEAVE_DISCONNECT,
            );
            self.reap_left_players();
            return;
        }
        if self.cfg.allow_downloads == 2 {
            let allowed = self.players.by_pid(pid).map(|p| p.download_allowed).unwrap_or(false);
            if !allowed {
                tracing::info!(
                    pid,
                    "player lacks map and conditional downloads (allow_downloads=2) requires permission"
                );
                return;
            }
        }
        if self.downloads.iter().any(|d| d.pid == pid) {
            return;
        }

        let host_pid = self.host_pid();
        let mut d = Download::new(pid);
        d.sent_upto = report.map_size;
        d.acked_upto = report.map_size;
        self.downloads.push(d);
        self.send_to(pid, outgoing::start_download(host_pid));
        tracing::info!(game = %self.cfg.name, pid, host_pid, "map download started");
    }

    pub fn handle_map_part_ok(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        let Ok(acked) = incoming::decode_map_part_ok(payload) else {
            return;
        };
        let total = self.cfg.map.size.max(1);
        if let Some(d) = self.downloads.iter_mut().find(|d| d.pid == pid) {
            d.acked_upto = acked;
        }
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.download_status = ((acked as u64 * 100) / total as u64).min(100) as u8;
        }
    }

    pub fn handle_map_part_not_ok(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        let Ok(nack_pos) = incoming::decode_map_part_not_ok(payload) else {
            return;
        };
        let total = self.cfg.map.size.max(1);
        if let Some(d) = self.downloads.iter_mut().find(|d| d.pid == pid) {
            d.sent_upto = nack_pos;
            d.acked_upto = nack_pos;
            tracing::info!(
                game = %self.cfg.name,
                pid,
                nack_pos,
                "received MAP_PART_NOT_OK, rewound transfer position"
            );
        }
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.download_status = ((nack_pos as u64 * 100) / total as u64).min(100) as u8;
        }
    }

    pub fn handle_pong(&mut self, conn_id: u64, payload: &Bytes) {
        let Ok(pong) = incoming::decode_pong_to_host(payload) else {
            return;
        };
        if pong != 1 {
            let now = self.created_at.elapsed().as_millis() as u32;
            let latency_raw = now.saturating_sub(pong);
            let ping = if self.cfg.lc_pings {
                latency_raw / 2
            } else {
                latency_raw
            };
            let mut kick_info: Option<(u8, String)> = None;
            if let Some(p) = self.players.by_conn_mut(conn_id) {
                p.record_ping(ping);
                if matches!(self.phase, GamePhase::Lobby)
                    && !p.reserved
                    && self.cfg.autokick_ping > 0
                    && p.ping_history.len() >= 3
                {
                    if let Some(avg) = p.average_ping() {
                        if avg > self.cfg.autokick_ping {
                            tracing::info!(
                                name = %p.name,
                                avg_ping = avg,
                                limit = self.cfg.autokick_ping,
                                "autokicking player due to high ping"
                            );
                            kick_info = Some((
                                p.pid,
                                format!("autokicked for high ping ({avg}ms > {}ms)", self.cfg.autokick_ping),
                            ));
                        }
                    }
                }
            }
            if let Some((kpid, reason)) = kick_info {
                self.kick_player(kpid, &reason, ghost_protocol::w3gs::ids::PLAYERLEAVE_DISCONNECT);
            }
        }
    }

    pub fn handle_drop_request(&mut self, conn_id: u64) {
        if !self.lagging {
            return;
        }
        tracing::info!(conn_id, "drop request while lagging, dropping laggers");
        let lagger_pids: Vec<u8> = self.players.iter().filter(|p| p.lagging && p.left.is_none()).map(|p| p.pid).collect();
        for lpid in lagger_pids {
            self.kick_player(lpid, "was dropped by vote", ghost_protocol::w3gs::ids::PLAYERLEAVE_DISCONNECT);
        }
    }

    /// Sends the next slice of every in-flight map download. Called once per tick.
    pub fn pump_downloads(&mut self) {
        if !matches!(self.phase, GamePhase::Lobby) {
            return;
        }
        let Some(data) = self.cfg.map.data.clone() else {
            self.downloads.clear();
            return;
        };
        let total = data.len() as u32;

        if self.last_download_counter_reset.elapsed() >= std::time::Duration::from_secs(1) {
            self.download_counter = 0;
            self.last_download_counter_reset = Instant::now();
        }

        let host_pid = self.host_pid();
        let max_downloaders = self.cfg.max_downloaders as usize;
        let max_speed_bytes = (self.cfg.max_download_speed as usize) * 1024;
        let mut downloaders_count = 0usize;
        let mut packets: Vec<(u8, Bytes)> = Vec::new();

        for d in self.downloads.iter_mut() {
            if d.acked_upto >= total {
                continue;
            }
            downloaders_count += 1;
            if max_downloaders > 0 && downloaders_count > max_downloaders {
                break;
            }

            // Up to 100 parts per 100ms cycle (matching GHost++ game_base.cpp:599-634)
            let burst_limit = d.acked_upto.saturating_add((MAP_CHUNK * 100) as u32);
            while d.sent_upto < burst_limit && d.sent_upto < total {
                if max_speed_bytes > 0 && self.download_counter >= max_speed_bytes {
                    break;
                }

                let start = d.sent_upto as usize;
                let end = (start + MAP_CHUNK).min(data.len());
                match outgoing::map_part(host_pid, d.pid, d.sent_upto, &data[start..end]) {
                    Ok(b) => {
                        packets.push((d.pid, b));
                        self.download_counter += end - start;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to build map part");
                        break;
                    }
                }
                d.sent_upto = end as u32;
            }
        }

        let store = self.store.clone();
        let map_path = self.cfg.map.path.clone();
        let map_size = self.cfg.map.size as u64;
        let players = &self.players;

        self.downloads.retain(|d| {
            if d.acked_upto >= total {
                let elapsed_secs = d.started.elapsed().as_secs();
                tracing::info!(
                    pid = d.pid,
                    secs = elapsed_secs,
                    "map download finished"
                );
                if let Some(s) = &store {
                    if let Some(p) = players.by_pid(d.pid) {
                        let ip_str = format!(
                            "{}.{}.{}.{}",
                            p.external_ip[0], p.external_ip[1], p.external_ip[2], p.external_ip[3]
                        );
                        s.record_download(
                            &map_path,
                            map_size,
                            &p.name,
                            &ip_str,
                            if p.spoofed { 1 } else { 0 },
                            total as u64,
                            elapsed_secs,
                        );
                    }
                }
                false
            } else {
                true
            }
        });

        for (pid, b) in packets {
            self.send_to(pid, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use ghost_protocol::w3gs::ids;

    #[test]
    fn a_client_reporting_a_partial_map_starts_a_download() {
        let (mut st, mut rxs) = seated_game(1);
        st.cfg.map.size = 100_000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 100_000]));
        let _ = drain_ids(&mut rxs[0]);

        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut p, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut p, 1);
        bytes::BufMut::put_u32_le(&mut p, 0); // has 0 of 100000 bytes
        st.handle_map_size(1, &p.freeze());

        assert_eq!(st.downloads.len(), 1);
        let first_pkt = rxs[0].try_recv().expect("must receive START_DOWNLOAD");
        assert_eq!(first_pkt[1], ids::START_DOWNLOAD);
        // Wire verification B1: fromPID is host PID (1), not 255
        assert_eq!(first_pkt[4], 1, "START_DOWNLOAD fromPID must be host PID");
    }

    #[test]
    fn map_part_packets_carry_host_pid() {
        let (mut st, mut rxs) = seated_game(1);
        st.cfg.map.size = 100_000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0xAA; 100_000]));
        st.downloads.push(Download::new(1));
        let _ = drain_ids(&mut rxs[0]);

        st.pump_downloads();

        let part_pkt = rxs[0].try_recv().expect("must receive MAP_PART");
        assert_eq!(part_pkt[1], ids::MAP_PART);
        // Wire verification B1: fromPID is host PID (1), toPID is 1
        assert_eq!(part_pkt[4], 1, "MAP_PART fromPID must be host PID");
        assert_eq!(part_pkt[5], 1, "MAP_PART toPID must be receiver PID");
    }

    #[test]
    fn a_client_with_the_whole_map_starts_no_download() {
        let (mut st, _rxs) = seated_game(1);
        st.cfg.map.size = 1000;
        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut p, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut p, 1);
        bytes::BufMut::put_u32_le(&mut p, 1000);
        st.handle_map_size(1, &p.freeze());
        assert!(st.downloads.is_empty());
    }

    #[test]
    fn download_throttling_respects_max_download_speed() {
        let (mut st, mut rxs) = seated_game(1);
        st.cfg.map.size = 100_000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 100_000]));
        st.cfg.max_download_speed = 5; // 5 KB/s = 5120 bytes max in 1 sec window
        st.downloads.push(Download::new(1));
        let _ = drain_ids(&mut rxs[0]);

        st.pump_downloads();

        let sent = drain_ids(&mut rxs[0]);
        // 5120 bytes / 1442 bytes per part = at most 4 parts sent
        assert!(sent.len() <= 4, "must throttle to max_download_speed");
    }

    #[test]
    fn a_finished_download_is_removed() {
        let (mut st, _rxs) = seated_game(1);
        st.cfg.map.size = 1000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 1000]));
        let mut d = Download::new(1);
        d.sent_upto = 1000;
        d.acked_upto = 1000;
        st.downloads.push(d);

        st.pump_downloads();

        assert!(st.downloads.is_empty());
    }

    #[test]
    fn client_without_map_is_dropped_when_downloads_are_disabled() {
        let (mut st, mut rxs) = seated_game(2);
        st.cfg.map.size = 50_000;
        st.cfg.map.data = None; // downloads disabled
        for rx in &mut rxs {
            let _ = drain_ids(rx);
        }

        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut p, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut p, 1);
        bytes::BufMut::put_u32_le(&mut p, 0); // client has 0 bytes
        st.handle_map_size(1, &p.freeze());

        // Player 1 must be dropped and slot freed
        assert!(st.players.by_pid(1).is_none());
        assert_eq!(st.slots.as_wire()[0].slot_status, 0); // open slot
        let sent = drain_ids(&mut rxs[1]);
        assert!(sent.contains(&ids::PLAYER_LEAVE_OTHERS));
        assert!(sent.contains(&ids::SLOT_INFO));
    }

    #[test]
    fn test_map_part_not_ok_rewinds_download_and_resends() {
        let (mut st, mut rxs) = seated_game(1);
        st.cfg.map.size = 10_000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 10_000]));
        let mut d = Download::new(1);
        d.sent_upto = 4000;
        d.acked_upto = 2884;
        st.downloads.push(d);

        let _ = drain_ids(&mut rxs[0]);

        // Client reports MAP_PART_NOT_OK at 1442 bytes (corrupted second part)
        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_u8(&mut p, 1); // to
        bytes::BufMut::put_u8(&mut p, 1); // from
        bytes::BufMut::put_u32_le(&mut p, 1442);
        st.handle_map_part_not_ok(1, &p.freeze());

        // Download state must be rewound to 1442
        let d_now = st.downloads.iter().find(|d| d.pid == 1).unwrap();
        assert_eq!(d_now.sent_upto, 1442);
        assert_eq!(d_now.acked_upto, 1442);

        // Next pump_downloads must send map part starting from 1442
        st.pump_downloads();
        let part_pkt = rxs[0].try_recv().expect("must receive MAP_PART after rewind");
        assert_eq!(part_pkt[1], ids::MAP_PART);
        let start_offset = u32::from_le_bytes([part_pkt[10], part_pkt[11], part_pkt[12], part_pkt[13]]);
        assert_eq!(start_offset, 1442, "MAP_PART must be resent starting from rewound offset");
    }
}
