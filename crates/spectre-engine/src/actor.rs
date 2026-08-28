use std::time::Instant;

use spectre_net::{AnyFrame, ConnEventKind};
use spectre_protocol::frame::Frame;
use spectre_protocol::w3gs::{ids, incoming};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::handle::{GameCmd, GameHandle};
use crate::state::{GameConfig, GamePhase, GameState};

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
    let mut last_lobby_status: Option<(u32, u32, u32)> = None;

    loop {
        tokio::select! {
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
                state.publish_dotatv().await;
                sleep.as_mut().reset(state.tick.deadline().into());
            }
        }

        state.reap_left_players();

        if matches!(state.phase, GamePhase::Lobby) {
            let open = state.slots.open_slots() as u32;
            let total = state.slots.len() as u32;
            let human = state.players.human_count() as u32;
            if last_lobby_status != Some((open, total, human)) {
                last_lobby_status = Some((open, total, human));
                if let Some(tx) = &state.cfg.event_tx {
                    let _ = tx.try_send(crate::handle::GameEvent::LobbyStatus {
                        host_counter: state.cfg.host_counter,
                        slots_open: open,
                        slots_total: total,
                        human_players: human,
                    });
                }
            }
        }

        if state.finished {
            break;
        }
    }

    state.finish_dotatv().await;

    if let Some(rep) = state.replay.take() {
        let replay_path = state.cfg.replay_path.clone();
        tokio::spawn(async move {
            if let Some(parent) = replay_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            if let Err(e) =
                spectre_spectator::save_replay(replay_path.clone(), rep, 26, 6059, true).await
            {
                tracing::error!(path = ?replay_path, error = %e, "failed to save .w3g replay file");
            } else {
                tracing::info!(path = ?replay_path, "successfully saved .w3g replay file off-thread");
            }
        });
    }
}

impl GameState {
    pub fn handle_cmd(&mut self, cmd: GameCmd) {
        match cmd {
            GameCmd::NewConn {
                conn_id,
                link,
                external_ip,
            } => self.add_conn(conn_id, link, external_ip),
            GameCmd::AdoptReconnect {
                conn_id,
                pid,
                reconnect_key,
                last_packet,
                link,
                response,
            } => {
                let ok = self.handle_gps_reconnect(
                    conn_id,
                    spectre_protocol::gps::ReconnectReq {
                        pid,
                        reconnect_key,
                        last_packet,
                    },
                    link,
                );
                let _ = response.send(ok);
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
            GameCmd::AttachDotaTv(shared) => {
                tracing::info!(game = %self.cfg.name, "dotatv: live stream attached");
                self.dotatv = Some(shared);
            }
            GameCmd::ToggleFakePlayer => {
                if let Some(msg) = self.toggle_fake_player() {
                    tracing::info!(game = %self.cfg.name, "{msg}");
                }
            }
            GameCmd::Shutdown => self.finished = true,
            GameCmd::CreateVirtualHost => self.create_virtual_host(),
        }
    }

    pub fn on_frame(&mut self, conn_id: u64, frame: AnyFrame) {
        match frame {
            AnyFrame::W3gs(f) => self.on_w3gs_frame(conn_id, f),
            AnyFrame::Gps(f) => self.on_gps_frame(conn_id, f),
            AnyFrame::DotaTv(_) => {}
        }
    }

    pub fn on_w3gs_frame(&mut self, conn_id: u64, frame: Frame) {
        if frame.id != ids::OUTGOING_KEEPALIVE {
            tracing::info!(conn_id, id = format!("0x{:02X}", frame.id), len = frame.payload.len(), phase = ?self.phase, "w3gs frame");
        }
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
            ids::MAP_PART_NOT_OK => self.handle_map_part_not_ok(conn_id, &frame.payload),
            ids::DROP_REQ => self.handle_drop_request(conn_id),
            other => tracing::warn!(
                conn_id,
                id = format!("0x{other:02X}"),
                len = frame.payload.len(),
                "unknown w3gs packet id"
            ),
        }
    }

    pub fn on_gps_frame(&mut self, conn_id: u64, frame: Frame) {
        match frame.id {
            spectre_protocol::gps::ids::INIT => {
                let empty_actions = self.gproxy_empty_actions();
                let port = if self.cfg.gproxy_reconnect_port != 0 {
                    self.cfg.gproxy_reconnect_port
                } else if self.cfg.host_port != 0 {
                    self.cfg.host_port
                } else {
                    6114
                };
                if let Some(p) = self.players.by_conn_mut(conn_id) {
                    p.gproxy = true;
                    if p.gproxy_buffer.is_none() {
                        p.gproxy_buffer = Some(crate::gproxy::GProxyBuffer::new(500));
                    }
                    let _ = p.link.try_send(spectre_protocol::gps::init(
                        port as u32,
                        p.pid,
                        p.reconnect_key,
                        empty_actions,
                    ));
                    tracing::info!(game = %self.cfg.name, pid = p.pid, name = %p.name, "player is using GProxy++");
                }
            }
            spectre_protocol::gps::ids::ACK => {
                if let Some(p) = self.players.by_conn_mut(conn_id) {
                    p.gproxy = true;
                    if p.gproxy_buffer.is_none() {
                        p.gproxy_buffer = Some(crate::gproxy::GProxyBuffer::new(500));
                    }
                }
            }
            spectre_protocol::gps::ids::RECONNECT => {
                if let Ok(req) = spectre_protocol::gps::decode_reconnect(&frame.payload)
                    && let Some(idx) = self.pending.iter().position(|(id, _, _)| *id == conn_id)
                {
                    let (_, link, _) = self.pending.remove(idx);
                    self.handle_gps_reconnect(conn_id, req, link);
                }
            }
            spectre_protocol::gps::ids::FULL => {
                if let Ok((pid, key)) = spectre_protocol::gps::decode_full(&frame.payload) {
                    self.pending_full.insert(conn_id, (pid, key));
                }
            }
            _ => {}
        }
    }

    pub fn start_countdown(&mut self, by: &str) {
        if matches!(self.phase, GamePhase::Lobby) {
            tracing::info!(game = %self.cfg.name, %by, "countdown started");
            self.phase = GamePhase::Countdown {
                started_at: std::time::Instant::now(),
                total_duration: crate::state::COUNTDOWN_TOTAL,
                last_announced_step: crate::state::COUNTDOWN_STEPS + 1,
            };
        }
    }
}

#[doc(hidden)]
pub mod tests_support {
    use super::*;
    use crate::state::MapInfo;
    use bytes::{BufMut, Bytes, BytesMut};
    use spectre_net::PlayerLink;
    use std::time::Duration;

    pub fn test_cfg() -> GameConfig {
        GameConfig {
            name: "test".into(),
            owner: "slash".into(),
            host_counter: 1,
            num_slots: 12,
            latency: Duration::from_millis(100),
            sync_limit: 50,
            map: MapInfo::test_default(),
            virtual_host_name: "|cFF4080C0Spectre".into(),
            reconnect_wait: Duration::from_secs(180),
            custom_slots: None,
            replay_path: std::path::PathBuf::from("replays/test.w3g"),
            relay: None,
            max_downloaders: 3,
            max_download_speed: 100,
            allow_downloads: 1,
            autokick_ping: 400,
            lc_pings: true,
            spoof_checks: 0,
            require_spoof_checks: false,
            host_port: 6112,
            gproxy_reconnect_port: 6114,
            store: None,
            stat_string: Vec::new(),
            event_tx: None,
            lobby_time_limit: 10,
            creator_name: String::new(),
            creator_server: String::new(),
            min_score: 0.0,
            max_score: 0.0,
            matchmaking: false,
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
        b.put_slice(&[0, 0, 0, 0, 0, 0]);
        b.put_slice(&[127, 0, 0, 1]);
        b.freeze()
    }

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
            st.on_frame(
                conn_id,
                AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes(&format!("P{i}")))),
            );
            rxs.push(rx);
        }
        (st, rxs)
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::*;
    use super::*;
    use bytes::Bytes;
    use spectre_net::PlayerLink;
    use spectre_protocol::w3gs::ids;
    use std::time::Duration;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn a_joining_player_gets_slotinfojoin_and_is_seated() {
        let mut st = GameState::new(test_cfg());
        let (tx, mut rx) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx), [1, 2, 3, 4]);

        st.on_frame(
            1,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))),
        );

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

        st.on_frame(
            1,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))),
        );
        st.on_frame(
            2,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))),
        );

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

        st.on_frame(
            1,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("A"))),
        );
        st.on_frame(
            2,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("B"))),
        );

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
        st.on_frame(
            1,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("A"))),
        );
        st.on_frame(
            2,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("B"))),
        );
        let _ = drain_ids(&mut rx2);

        st.on_frame(
            1,
            AnyFrame::W3gs(Frame::new(
                ids::LEAVE_GAME,
                Bytes::from_static(&[7, 0, 0, 0]),
            )),
        );
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
        st.on_frame(
            1,
            AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("A"))),
        );
        drop(rx);

        st.broadcast(Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]));
        st.reap_left_players();

        assert_eq!(st.players.len(), 0);
    }

    #[tokio::test]
    async fn backpressure_drops_player_only_after_max_consecutive_drops() {
        let (mut st, _rxs) = tests_support::seated_game(1);
        st.begin_playing();

        let (tx, _rx) = mpsc::channel(1);
        tx.try_send(Bytes::from_static(&[1, 2, 3])).unwrap();
        st.players.by_pid_mut(1).unwrap().link = PlayerLink::for_test(tx);

        let packet = Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]);

        for _ in 0..99 {
            st.send_to(1, packet.clone());
            assert!(
                st.players.by_pid(1).unwrap().left.is_none(),
                "player must not be dropped before reaching MAX_CONSECUTIVE_DROPS"
            );
        }
        assert_eq!(st.players.by_pid(1).unwrap().consecutive_send_failures, 99);

        st.send_to(1, packet);
        assert!(
            st.players.by_pid(1).unwrap().left.is_some(),
            "player must be marked left on the 100th consecutive backpressure failure"
        );
    }

    #[tokio::test]
    async fn successful_send_resets_consecutive_send_failures() {
        let (mut st, mut rx) = {
            let (mut st, _rxs) = tests_support::seated_game(1);
            st.begin_playing();
            let (tx, rx) = mpsc::channel(1);
            st.players.by_pid_mut(1).unwrap().link = PlayerLink::for_test(tx);
            (st, rx)
        };

        let packet = Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]);

        st.send_to(1, packet.clone());

        for _ in 0..50 {
            st.send_to(1, packet.clone());
        }
        assert_eq!(st.players.by_pid(1).unwrap().consecutive_send_failures, 50);

        let _ = rx.try_recv();

        st.send_to(1, packet.clone());
        assert_eq!(st.players.by_pid(1).unwrap().consecutive_send_failures, 0);
        assert!(st.players.by_pid(1).unwrap().left.is_none());
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

    #[tokio::test]
    async fn gps_full_frame_caches_the_token() {
        let (mut st, _rxs) = tests_support::seated_game(1);
        let conn_id = st.players.by_pid(1).unwrap().conn_id;
        let frame = spectre_protocol::frame::Frame::new(
            spectre_protocol::gps::ids::FULL,
            spectre_protocol::gps::full(9, 0x1234_5678).slice(4..),
        );
        st.on_gps_frame(conn_id, frame);
        assert_eq!(st.pending_full.get(&conn_id), Some(&(9u8, 0x1234_5678u32)));
    }
}
