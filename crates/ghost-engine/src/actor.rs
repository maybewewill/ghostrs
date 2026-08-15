use std::time::Instant;

use ghost_net::{AnyFrame, ConnEventKind};
use ghost_protocol::frame::Frame;
use ghost_protocol::w3gs::{ids, incoming};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::handle::{GameCmd, GameHandle};
use crate::state::{GameConfig, GamePhase, GameState};

/// Bounds how far the command queue may back up before the sender complains.
const CMD_CAPACITY: usize = 4096;

pub fn spawn_game(cfg: GameConfig) -> (GameHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(CMD_CAPACITY);
    let name = cfg.name.clone();
    let join = tokio::spawn(async move {
        let state = GameState::new(cfg);
        run(state, rx).await;
        tracing::info!(game = %name, "game actor exited");
    });
    (GameHandle::new(tx), join)
}

async fn run(mut state: GameState, mut rx: mpsc::Receiver<GameCmd>) {
    let sleep = tokio::time::sleep_until(state.tick.deadline().into());
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            // Commands first: actions that arrive just before a deadline should
            // make it into that tick rather than the next one.
            biased;

            cmd = rx.recv() => {
                match cmd {
                    Some(GameCmd::Shutdown) | None => break,
                    Some(c) => state.handle_cmd(c),
                }
            }

            () = &mut sleep => {
                let now = Instant::now();
                let deadline = state.tick.deadline();
                let jitter = now.saturating_duration_since(deadline);
                state.record_jitter(jitter);
                let skipped = state.tick.advance(now);
                if skipped > 0 {
                    tracing::warn!(game = %state.cfg.name, skipped, "tick deadline missed");
                }
                state.on_tick(skipped);
                sleep.as_mut().reset(state.tick.deadline().into());
            }
        }

        state.reap_left_players();
        if state.finished {
            break;
        }
    }
}

impl GameState {
    pub fn handle_cmd(&mut self, cmd: GameCmd) {
        match cmd {
            GameCmd::NewConn { conn_id, link, external_ip } => {
                self.add_conn(conn_id, link, external_ip)
            }
            GameCmd::Conn(ev) => match ev.kind {
                ConnEventKind::Frame(f) => self.on_frame(ev.conn_id, f),
                ConnEventKind::Closed(reason) => {
                    self.handle_conn_closed(ev.conn_id, format!("{reason:?}"))
                }
            },
            GameCmd::Start { by } => self.start_countdown(&by),
            GameCmd::Chat(msg) => self.send_chat_all(&msg),
            GameCmd::Unhost => {
                if matches!(self.phase, GamePhase::Lobby) {
                    self.finished = true;
                }
            }
            GameCmd::Shutdown => self.finished = true,
        }
    }

    pub fn on_frame(&mut self, conn_id: u64, frame: AnyFrame) {
        match frame {
            AnyFrame::W3gs(f) => self.on_w3gs_frame(conn_id, f),
            AnyFrame::Gps(f) => self.on_gps_frame(conn_id, f),
        }
    }

    pub fn on_w3gs_frame(&mut self, conn_id: u64, frame: Frame) {
        match frame.id {
            ids::REQ_JOIN => self.handle_req_join(conn_id, &frame.payload),
            ids::LEAVE_GAME => {
                let code = incoming::decode_leave_game(&frame.payload).unwrap_or(0);
                self.handle_leave(conn_id, code);
            }
            ids::OUTGOING_ACTION => self.handle_action(conn_id, &frame.payload),
            ids::OUTGOING_KEEPALIVE => self.handle_keepalive(conn_id, &frame.payload),
            ids::CHAT_TO_HOST => self.handle_chat_to_host(conn_id, &frame.payload),
            ids::GAME_LOADED_SELF => self.handle_loaded(conn_id),
            ids::PONG_TO_HOST => self.handle_pong(conn_id, &frame.payload),
            ids::MAP_SIZE => self.handle_map_size(conn_id, &frame.payload),
            ids::MAP_PART_OK => self.handle_map_part_ok(conn_id, &frame.payload),
            ids::DROP_REQ => self.handle_drop_request(conn_id),
            other => tracing::trace!(conn_id, id = format!("0x{other:02X}"), "ignoring packet"),
        }
    }

    pub fn on_gps_frame(&mut self, conn_id: u64, frame: Frame) {
        match frame.id {
            ghost_protocol::gps::ids::ACK => {
                if let Some(p) = self.players.by_conn_mut(conn_id) {
                    p.gproxy = true;
                    if p.gproxy_buffer.is_none() {
                        p.gproxy_buffer = Some(crate::gproxy::GProxyBuffer::new(500));
                    }
                }
            }
            ghost_protocol::gps::ids::RECONNECT => {
                if let Ok(req) = ghost_protocol::gps::decode_reconnect(&frame.payload)
                    && let Some(idx) = self.pending.iter().position(|(id, _, _)| *id == conn_id)
                {
                    let (_, link, _) = self.pending.remove(idx);
                    self.handle_gps_reconnect(conn_id, req, link);
                }
            }
            _ => {}
        }
    }

    pub fn start_countdown(&mut self, by: &str) {
        if matches!(self.phase, GamePhase::Lobby) {
            tracing::info!(game = %self.cfg.name, %by, "countdown started");
            self.phase = GamePhase::Countdown { remaining: 5 };
        }
    }
}

#[cfg(test)]
pub mod tests_support {
    use super::*;
    use std::time::Duration;
    use bytes::{BufMut, Bytes, BytesMut};
    use ghost_net::PlayerLink;
    use crate::state::MapInfo;

    pub fn test_cfg() -> GameConfig {
        GameConfig {
            name: "test".into(),
            owner: "slash".into(),
            host_counter: 1,
            num_slots: 12,
            latency: Duration::from_millis(100),
            sync_limit: 50,
            map: MapInfo::test_default(),
            virtual_host_name: "|cFF4080C0Ghost".into(),
            reconnect_wait: Duration::from_secs(180),
        }
    }

    pub fn reqjoin_bytes(name: &str) -> Bytes {
        let mut b = BytesMut::new();
        b.put_u32_le(1);
        b.put_u32_le(0);
        b.put_u8(0);
        b.put_u16_le(6112);
        b.put_u32_le(0);
        b.put_slice(name.as_bytes());
        b.put_u8(0);
        b.put_slice(&[0, 0, 0, 0, 0, 0]); // 6 bytes unknown/sockaddr prefix
        b.put_slice(&[127, 0, 0, 1]);
        b.freeze()
    }

    /// Drains one player's outbound queue into a list of (id, payload) pairs.
    pub fn drain_ids(rx: &mut mpsc::Receiver<Bytes>) -> Vec<u8> {
        let mut ids = Vec::new();
        while let Ok(b) = rx.try_recv() {
            ids.push(b[1]);
        }
        ids
    }

    pub fn seated_game(n: usize) -> (GameState, Vec<mpsc::Receiver<Bytes>>) {
        let mut st = GameState::new(test_cfg());
        let mut rxs = Vec::new();
        for i in 1..=n {
            let conn_id = i as u64;
            let (tx, rx) = mpsc::channel(64);
            st.add_conn(conn_id, PlayerLink::for_test(tx), [1, 2, 3, 4]);
            st.on_frame(conn_id, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes(&format!("P{i}")))));
            rxs.push(rx);
        }
        (st, rxs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::tests_support::*;
    use std::time::Duration;
    use bytes::Bytes;
    use ghost_net::PlayerLink;
    use ghost_protocol::w3gs::ids;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn a_joining_player_gets_slotinfojoin_and_is_seated() {
        let mut st = GameState::new(test_cfg());
        let (tx, mut rx) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx), [1, 2, 3, 4]);

        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))));

        assert_eq!(st.players.len(), 1);
        let p = st.players.by_conn(1).expect("seated");
        assert_eq!(p.name, "Slash");
        assert_eq!(st.slots.count_occupied(), 1);
        assert!(drain_ids(&mut rx).contains(&ids::SLOT_INFO_JOIN));
    }

    #[tokio::test]
    async fn a_second_player_with_the_same_name_is_rejected() {
        let mut st = GameState::new(test_cfg());
        let (tx1, _rx1) = mpsc::channel(64);
        let (tx2, mut rx2) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx1), [1, 1, 1, 1]);
        st.add_conn(2, PlayerLink::for_test(tx2), [2, 2, 2, 2]);

        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))));
        st.on_frame(2, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))));

        assert_eq!(st.players.len(), 1);
        assert!(drain_ids(&mut rx2).contains(&ids::REJECT_JOIN));
    }

    #[tokio::test]
    async fn joining_a_full_lobby_is_rejected() {
        let mut cfg = test_cfg();
        cfg.num_slots = 1;
        let mut st = GameState::new(cfg);
        let (tx1, _rx1) = mpsc::channel(64);
        let (tx2, mut rx2) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx1), [1, 1, 1, 1]);
        st.add_conn(2, PlayerLink::for_test(tx2), [2, 2, 2, 2]);

        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("A"))));
        st.on_frame(2, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("B"))));

        assert_eq!(st.players.len(), 1);
        assert!(drain_ids(&mut rx2).contains(&ids::REJECT_JOIN));
    }
    #[tokio::test]
    async fn leaving_frees_the_slot_and_notifies_everyone_else() {
        let mut st = GameState::new(test_cfg());
        let (tx1, _rx1) = mpsc::channel(64);
        let (tx2, mut rx2) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx1), [1, 1, 1, 1]);
        st.add_conn(2, PlayerLink::for_test(tx2), [2, 2, 2, 2]);
        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("A"))));
        st.on_frame(2, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("B"))));
        let _ = drain_ids(&mut rx2);

        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::LEAVE_GAME, Bytes::from_static(&[7, 0, 0, 0]))));
        st.reap_left_players();

        assert_eq!(st.players.len(), 1);
        assert_eq!(st.slots.count_occupied(), 1);
        assert!(drain_ids(&mut rx2).contains(&ids::PLAYER_LEAVE_OTHERS));
    }

    #[tokio::test]
    async fn a_dead_link_removes_the_player_instead_of_stalling_the_tick() {
        let mut st = GameState::new(test_cfg());
        let (tx, rx) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx), [1, 1, 1, 1]);
        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("A"))));
        drop(rx); // the writer task is gone

        st.broadcast(Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]));
        st.reap_left_players();

        assert_eq!(st.players.len(), 0);
    }

    #[tokio::test]
    async fn the_actor_shuts_down_on_command() {
        let (handle, join) = spawn_game(test_cfg());
        handle.send(GameCmd::Shutdown);
        tokio::time::timeout(Duration::from_secs(2), join)
            .await
            .expect("actor must exit promptly")
            .expect("actor must not panic");
    }
}
