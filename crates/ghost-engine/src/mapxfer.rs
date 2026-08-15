use std::time::Instant;

use bytes::Bytes;
use ghost_protocol::w3gs::{incoming::MapSizeReport, incoming, outgoing};

use crate::state::GameState;

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
        Self { pid, sent_upto: 0, acked_upto: 0, started: Instant::now() }
    }
}

impl GameState {
    pub fn handle_map_size(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else { return };
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
        if self.cfg.map.data.is_none() {
            tracing::info!(pid, "player lacks the map and downloads are disabled, dropping");
            if let Some(p) = self.players.by_pid_mut(pid) {
                p.left = Some("lacks map and downloads are disabled".into());
            }
            self.reap_left_players();
            return;
        }
        if self.downloads.iter().any(|d| d.pid == pid) {
            return;
        }

        let mut d = Download::new(pid);
        d.sent_upto = report.map_size;
        d.acked_upto = report.map_size;
        self.downloads.push(d);
        self.send_to(pid, outgoing::start_download(255));
        tracing::info!(game = %self.cfg.name, pid, "map download started");
    }

    pub fn handle_map_part_ok(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else { return };
        let Ok(acked) = incoming::decode_map_part_ok(payload) else { return };
        let total = self.cfg.map.size.max(1);
        if let Some(d) = self.downloads.iter_mut().find(|d| d.pid == pid) {
            d.acked_upto = acked;
        }
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.download_status = ((acked as u64 * 100) / total as u64).min(100) as u8;
        }
    }

    pub fn handle_pong(&mut self, conn_id: u64, payload: &Bytes) {
        let Ok(pong) = incoming::decode_pong_to_host(payload) else { return };
        let now = self.created_at.elapsed().as_millis() as u32;
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.record_ping(now.saturating_sub(pong) / 2);
        }
    }

    pub fn handle_drop_request(&mut self, conn_id: u64) {
        if !self.lagging {
            return;
        }
        tracing::info!(conn_id, "drop request while lagging, dropping laggers");
        for p in self.players.iter_mut() {
            if p.lagging && p.left.is_none() {
                p.left = Some("was dropped by vote".into());
            }
        }
    }

    /// Sends the next slice of every in-flight map download. Called once per tick.
    pub fn pump_downloads(&mut self) {
        let Some(data) = self.cfg.map.data.clone() else {
            self.downloads.clear();
            return;
        };
        let total = data.len() as u32;

        let mut packets: Vec<(u8, Bytes)> = Vec::new();
        self.downloads.retain_mut(|d| {
            if d.acked_upto >= total {
                tracing::info!(pid = d.pid, secs = d.started.elapsed().as_secs(), "map download finished");
                return false;
            }
            for _ in 0..MAX_PARTS_PER_TICK {
                if d.sent_upto >= total {
                    break;
                }
                let start = d.sent_upto as usize;
                let end = (start + MAP_CHUNK).min(data.len());
                match outgoing::map_part(255, d.pid, d.sent_upto, &data[start..end]) {
                    Ok(b) => packets.push((d.pid, b)),
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to build map part");
                        break;
                    }
                }
                d.sent_upto = end as u32;
            }
            true
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
        assert!(drain_ids(&mut rxs[0]).contains(&ids::START_DOWNLOAD));
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
    fn each_tick_sends_a_bounded_number_of_map_parts() {
        let (mut st, mut rxs) = seated_game(1);
        st.cfg.map.size = 100_000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 100_000]));
        st.downloads.push(Download::new(1));
        let _ = drain_ids(&mut rxs[0]);

        st.pump_downloads();

        let sent = drain_ids(&mut rxs[0]);
        assert_eq!(sent.len(), MAX_PARTS_PER_TICK);
        assert!(sent.iter().all(|&id| id == ids::MAP_PART));
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
}
