use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bytes::Bytes;
use ghost_net::PlayerLink;
use ghost_protocol::gps::{ReconnectReq, reconnect_ok, reject, reject_reason};

use crate::state::GameState;

/// Ring buffer of packets sent to one GProxy client, so a reconnecting client
/// can be replayed exactly what it missed.
#[derive(Debug, Clone)]
pub struct GProxyBuffer {
    capacity: usize,
    /// Sequence number of the oldest packet still held.
    first_packet_id: u32,
    packets: VecDeque<Bytes>,
}

impl GProxyBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            first_packet_id: 0,
            packets: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push(&mut self, packet: Bytes) {
        if self.packets.len() == self.capacity {
            self.packets.pop_front();
            self.first_packet_id += 1;
        }
        self.packets.push_back(packet);
    }

    /// Total packets ever pushed, i.e. the sequence number of the newest one.
    pub fn total_sent(&self) -> u32 {
        self.first_packet_id + self.packets.len() as u32
    }

    /// Packets the client has not confirmed, or None when they were evicted or
    /// the client claims to have more than we ever sent.
    pub fn replay_from(&self, last_received: u32) -> Option<Vec<Bytes>> {
        if last_received > self.total_sent() || last_received < self.first_packet_id {
            return None;
        }
        let skip = (last_received - self.first_packet_id) as usize;
        Some(self.packets.iter().skip(skip).cloned().collect())
    }
}

impl GameState {
    pub fn gproxy_empty_actions(&self) -> u8 {
        let secs = self.cfg.reconnect_wait.as_secs();
        if secs == 0 {
            return 0;
        }
        let mins = if secs >= 60 { secs / 60 } else { secs };
        (mins.saturating_sub(1)).min(9) as u8
    }

    pub fn handle_gps_reconnect(
        &mut self,
        conn_id: u64,
        req: ReconnectReq,
        link: PlayerLink,
    ) -> bool {
        let Some(p) = self.players.by_pid_mut(req.pid) else {
            let _ = link.try_send(reject(reject_reason::NOT_FOUND));
            return false;
        };
        if !p.gproxy || p.reconnect_key != req.reconnect_key {
            let _ = link.try_send(reject(reject_reason::INVALID_KEY));
            return false;
        }
        let Some(replay) = p
            .gproxy_buffer
            .as_ref()
            .and_then(|b| b.replay_from(req.last_packet))
        else {
            let _ = link.try_send(reject(reject_reason::NOT_FOUND));
            return false;
        };

        let received = p
            .gproxy_buffer
            .as_ref()
            .map(|b| b.total_sent())
            .unwrap_or(0);
        p.conn_id = conn_id;
        p.link = link;
        p.disconnected_since = None;
        p.left = None;

        let _ = p.link.try_send(reconnect_ok(received));
        for packet in replay {
            if p.link.try_send(packet).is_err() {
                break;
            }
        }
        tracing::info!(game = %self.cfg.name, pid = req.pid, "gproxy client reconnected");
        true
    }

    /// Removes GProxy players who never came back within the grace period,
    /// and periodically broadcasts wait notices every 20 seconds.
    pub fn reap_gproxy_timeouts(&mut self, grace: Duration) {
        let mut notices: Vec<String> = Vec::new();
        for p in self.players.iter_mut() {
            if let Some(since) = p.disconnected_since {
                let elapsed = since.elapsed();
                if elapsed >= grace {
                    p.left = Some("failed to reconnect in time".into());
                    p.left_code = ghost_protocol::w3gs::ids::PLAYERLEAVE_GPROXY;
                    p.disconnected_since = None;
                } else if !p.gproxy_disconnect_notice_sent {
                    p.gproxy_disconnect_notice_sent = true;
                    p.last_gproxy_wait_notice = Some(Instant::now());
                    notices.push(format!(
                        "Player [{}] has lost connection but is using GProxy++ and may reconnect.",
                        p.name
                    ));
                } else if p
                    .last_gproxy_wait_notice
                    .is_some_and(|t| t.elapsed() >= Duration::from_secs(20))
                {
                    let remaining = grace.saturating_sub(elapsed).as_secs();
                    p.last_gproxy_wait_notice = Some(Instant::now());
                    notices.push(format!(
                        "Waiting for reconnect ({} seconds remain)...",
                        remaining
                    ));
                }
            }
        }
        for notice in notices {
            self.send_chat_all(&notice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn replay_returns_everything_after_the_acknowledged_packet() {
        let mut b = GProxyBuffer::new(10);
        for i in 0..5u8 {
            b.push(Bytes::from(vec![i]));
        }
        assert_eq!(b.total_sent(), 5);
        let replay = b.replay_from(3).expect("packets 4 and 5");
        assert_eq!(replay.len(), 2);
        assert_eq!(&replay[0][..], &[3]);
    }

    #[test]
    fn replay_of_everything_returns_the_whole_buffer() {
        let mut b = GProxyBuffer::new(10);
        b.push(Bytes::from_static(&[1]));
        assert_eq!(b.replay_from(0).unwrap().len(), 1);
    }

    #[test]
    fn replay_fails_once_the_needed_packets_have_been_evicted() {
        let mut b = GProxyBuffer::new(3);
        for i in 0..10u8 {
            b.push(Bytes::from(vec![i]));
        }
        // Packet 2 is long gone: the client cannot be resynchronised.
        assert!(b.replay_from(2).is_none());
        assert!(b.replay_from(7).is_some());
    }

    #[test]
    fn a_client_claiming_more_packets_than_we_sent_is_rejected() {
        let mut b = GProxyBuffer::new(10);
        b.push(Bytes::from_static(&[1]));
        assert!(b.replay_from(99).is_none());
    }

    #[tokio::test]
    async fn a_valid_reconnect_reattaches_the_player_and_replays() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.begin_playing();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.players.by_pid_mut(1).unwrap().gproxy_buffer = Some(GProxyBuffer::new(100));
        let key = st.players.by_pid(1).unwrap().reconnect_key;
        st.on_tick(0); // one action packet is buffered
        st.players.by_pid_mut(1).unwrap().disconnected_since = Some(Instant::now());

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let ok = st.handle_gps_reconnect(
            99,
            ReconnectReq {
                pid: 1,
                reconnect_key: key,
                last_packet: 0,
            },
            PlayerLink::for_test(tx),
        );

        assert!(ok);
        assert_eq!(st.players.by_pid(1).unwrap().conn_id, 99);
        assert!(st.players.by_pid(1).unwrap().disconnected_since.is_none());
        assert!(rx.try_recv().is_ok(), "buffered packets must be replayed");
    }

    #[tokio::test]
    async fn broadcast_buffers_packets_and_does_not_drop_gproxy_players_on_closed_links() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.begin_playing();
        let p = st.players.by_pid_mut(1).unwrap();
        p.gproxy = true;
        p.gproxy_buffer = Some(GProxyBuffer::new(10));
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        drop(rx); // dead link
        p.link = PlayerLink::for_test(tx);

        st.broadcast(Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]));
        st.broadcast(Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]));

        let p = st.players.by_pid(1).unwrap();
        assert!(
            p.left.is_none(),
            "gproxy player must not be dropped immediately on a closed link"
        );
        assert!(
            p.disconnected_since.is_some(),
            "gproxy player must enter the reconnect grace period"
        );
        assert_eq!(
            p.gproxy_buffer.as_ref().unwrap().total_sent(),
            2,
            "packets must be buffered for replay on reconnect"
        );
    }

    #[tokio::test]
    async fn a_wrong_reconnect_key_is_refused() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.begin_playing();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let ok = st.handle_gps_reconnect(
            99,
            ReconnectReq {
                pid: 1,
                reconnect_key: 0xBAD,
                last_packet: 0,
            },
            PlayerLink::for_test(tx),
        );
        assert!(!ok);
        assert_ne!(st.players.by_pid(1).unwrap().conn_id, 99);
    }

    #[test]
    fn gproxy_empty_actions_formula_matches_ghostpp() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.cfg.reconnect_wait = Duration::from_secs(180); // 3 minutes
        assert_eq!(st.gproxy_empty_actions(), 2);

        st.cfg.reconnect_wait = Duration::from_secs(600); // 10 minutes (clamped to 9)
        assert_eq!(st.gproxy_empty_actions(), 9);

        st.cfg.reconnect_wait = Duration::from_secs(3); // 3 raw minutes / units
        assert_eq!(st.gproxy_empty_actions(), 2);

        st.cfg.reconnect_wait = Duration::from_secs(60); // 1 minute -> 0 empty actions
        assert_eq!(st.gproxy_empty_actions(), 0);

        st.cfg.reconnect_wait = Duration::ZERO;
        assert_eq!(st.gproxy_empty_actions(), 0);
    }

    #[test]
    fn gproxy_init_sends_configured_port_and_computed_empty_actions() {
        let (mut st, mut rxs) = crate::actor::tests_support::seated_game(1);
        st.cfg.reconnect_wait = Duration::from_secs(180);
        st.cfg.gproxy_reconnect_port = 6114;

        while rxs[0].try_recv().is_ok() {}

        let conn_id = st.players.by_pid(1).unwrap().conn_id;
        let init_frame = ghost_protocol::frame::Frame::new(
            ghost_protocol::gps::ids::INIT,
            bytes::Bytes::from_static(&[1, 0, 0, 0]),
        );

        st.on_gps_frame(conn_id, init_frame);

        let p = st.players.by_pid(1).unwrap();
        assert!(p.gproxy);

        let sent = rxs[0].try_recv().expect("must receive GPS_INIT reply");
        assert_eq!(sent[0], ghost_protocol::gps::GPS_HEADER);
        assert_eq!(sent[1], ghost_protocol::gps::ids::INIT);

        // Bytes 4..8: port (u32_le)
        let port = u32::from_le_bytes([sent[4], sent[5], sent[6], sent[7]]);
        assert_eq!(port, 6114);

        // Byte 8: PID
        assert_eq!(sent[8], 1);

        // Byte 13: num_empty_actions
        assert_eq!(sent[13], 2);
    }
}
