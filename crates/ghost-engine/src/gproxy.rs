use std::collections::VecDeque;
use std::time::Duration;

use bytes::Bytes;
use ghost_net::PlayerLink;
use ghost_protocol::gps::{ReconnectReq, ack, reject, reject_reason};

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
        Self { capacity, first_packet_id: 0, packets: VecDeque::with_capacity(capacity) }
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

        let received = p.gproxy_buffer.as_ref().map(|b| b.total_sent()).unwrap_or(0);
        p.conn_id = conn_id;
        p.link = link;
        p.disconnected_since = None;
        p.left = None;

        let _ = p.link.try_send(ack(received));
        for packet in replay {
            if p.link.try_send(packet).is_err() {
                break;
            }
        }
        tracing::info!(game = %self.cfg.name, pid = req.pid, "gproxy client reconnected");
        true
    }

    /// Removes GProxy players who never came back within the grace period.
    pub fn reap_gproxy_timeouts(&mut self, grace: Duration) {
        for p in self.players.iter_mut() {
            if p.disconnected_since.is_some_and(|t| t.elapsed() >= grace) {
                p.left = Some("failed to reconnect in time".into());
                p.disconnected_since = None;
            }
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
            ReconnectReq { pid: 1, reconnect_key: key, last_packet: 0 },
            PlayerLink::for_test(tx),
        );

        assert!(ok);
        assert_eq!(st.players.by_pid(1).unwrap().conn_id, 99);
        assert!(st.players.by_pid(1).unwrap().disconnected_since.is_none());
        assert!(rx.try_recv().is_ok(), "buffered packets must be replayed");
    }

    #[tokio::test]
    async fn a_wrong_reconnect_key_is_refused() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.begin_playing();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let ok = st.handle_gps_reconnect(
            99,
            ReconnectReq { pid: 1, reconnect_key: 0xBAD, last_packet: 0 },
            PlayerLink::for_test(tx),
        );
        assert!(!ok);
        assert_ne!(st.players.by_pid(1).unwrap().conn_id, 99);
    }
}
