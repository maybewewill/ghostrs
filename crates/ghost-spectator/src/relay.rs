use std::collections::{HashMap, VecDeque};
use std::time::Duration;
use tokio::time::Instant;

use bytes::Bytes;
use ghost_net::{AnyFrame, ConnEvent, ConnEventKind, LinkError, PlayerLink};
use ghost_protocol::dotatv::{
    self, GameStartSnapshot, PlayerInfo as DotaPlayerInfo, encode_action, encode_chat,
    encode_hello, encode_history_end, encode_player, encode_snapshot, ids as dotatv_ids,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

pub const MAX_CONSECUTIVE_DROPS: u32 = 200;

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub port: u16,
    pub delay: Duration,
    pub max_viewers: usize,
    pub game_name: String,
    pub history_max_mb: usize,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelayError {
    #[error("viewer capacity reached")]
    Full,
    #[error("History buffer limit exceeded")]
    HistoryLimitExceeded,
}

#[derive(Debug)]
pub enum RelayCmd {
    GameBlock(Bytes),
    GameStart(GameStartSnapshot),
    PlayerInfo {
        pid: u8,
        name: String,
        colour: u8,
        team: u8,
        race: u8,
    },
    ViewerJoined {
        conn_id: u64,
        link: PlayerLink,
    },
    ViewerLeft(u64),
    Conn(ConnEvent),
    ViewerChat {
        sender: String,
        text: String,
    },
    GameOver,
    Shutdown,
    DebugGetReleasedCount(oneshot::Sender<usize>),
}

#[derive(Debug, Clone)]
pub struct RelayHandle {
    tx: mpsc::Sender<RelayCmd>,
}

impl RelayHandle {
    pub fn new(tx: mpsc::Sender<RelayCmd>) -> Self {
        Self { tx }
    }

    pub fn push(&self, block: Bytes) {
        let _ = self.tx.try_send(RelayCmd::GameBlock(block));
    }

    pub fn send_game_start(&self, snap: GameStartSnapshot) {
        let _ = self.tx.try_send(RelayCmd::GameStart(snap));
    }

    pub fn send_player_info(&self, pid: u8, name: &str, colour: u8, team: u8, race: u8) {
        let _ = self.tx.try_send(RelayCmd::PlayerInfo {
            pid,
            name: name.to_string(),
            colour,
            team,
            race,
        });
    }

    pub fn send_chat(&self, sender: &str, text: &str) {
        let _ = self.tx.try_send(RelayCmd::ViewerChat {
            sender: sender.to_string(),
            text: text.to_string(),
        });
    }

    pub async fn debug_released_count(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(RelayCmd::DebugGetReleasedCount(tx)).await;
        rx.await.unwrap_or(0)
    }
}

pub struct Relay {
    pub cfg: RelayConfig,
    pub viewers: Vec<(u64, PlayerLink)>,
    pub drop_counts: HashMap<u64, u32>,
    pub delayed_blocks: VecDeque<(Instant, Bytes)>,
    pub released_count: usize,
    pub snapshot: Option<GameStartSnapshot>,
    pub players: Vec<DotaPlayerInfo>,
    pub history: Vec<Bytes>,
    pub history_bytes: usize,
}

impl Relay {
    pub fn new(cfg: RelayConfig) -> Self {
        Self {
            cfg,
            viewers: Vec::new(),
            drop_counts: HashMap::new(),
            delayed_blocks: VecDeque::new(),
            released_count: 0,
            snapshot: None,
            players: Vec::new(),
            history: Vec::new(),
            history_bytes: 0,
        }
    }

    pub fn add_viewer(&mut self, conn_id: u64, link: PlayerLink) -> Result<(), RelayError> {
        if self.viewers.len() >= self.cfg.max_viewers {
            return Err(RelayError::Full);
        }
        let max_history_bytes = self.cfg.history_max_mb.saturating_mul(1024 * 1024);
        if self.history_bytes > max_history_bytes {
            return Err(RelayError::HistoryLimitExceeded);
        }
        self.drop_counts.remove(&conn_id);
        self.viewers.push((conn_id, link));
        Ok(())
    }

    pub fn remove_viewer(&mut self, conn_id: u64) {
        self.drop_counts.remove(&conn_id);
        self.viewers.retain(|(id, _)| *id != conn_id);
    }

    pub fn handle_conn_event(&mut self, ev: ConnEvent) {
        match ev.kind {
            ConnEventKind::Closed(_) => {
                self.remove_viewer(ev.conn_id);
            }
            ConnEventKind::Frame(AnyFrame::W3gs(ref frame))
                if frame.id == dotatv_ids::CLIENT_CHAT =>
            {
                if let Ok(text) = dotatv::decode_client_chat(&frame.payload) {
                    let sender = format!("Viewer-{}", ev.conn_id);
                    if let Ok(pkt) = encode_chat(&sender, &text) {
                        self.broadcast(&pkt);
                    }
                }
            }
            _ => {}
        }
    }

    pub fn release_due_blocks(&mut self) {
        let now = Instant::now();
        while let Some(&(release_at, _)) = self.delayed_blocks.front() {
            if release_at <= now {
                let Some((_, block)) = self.delayed_blocks.pop_front() else {
                    break;
                };
                // Released blocks become history: a viewer joining from here on
                // must receive this block in its catch-up burst, and a viewer
                // already connected has just been sent it live. Exactly once
                // either way.
                self.history_bytes = self.history_bytes.saturating_add(block.len());
                self.history.push(block.clone());
                if let Ok(framed) = encode_action(&block) {
                    self.broadcast(&framed);
                } else {
                    self.broadcast(&block);
                }
                self.released_count += 1;
            } else {
                break;
            }
        }
    }

    pub fn broadcast(&mut self, bytes: &Bytes) {
        let drop_counts = &mut self.drop_counts;
        self.viewers
            .retain(|(conn_id, link)| match link.try_send(bytes.clone()) {
                Ok(()) => {
                    drop_counts.remove(conn_id);
                    true
                }
                Err(LinkError::Backpressure) => {
                    let drops = drop_counts.entry(*conn_id).or_insert(0);
                    *drops += 1;
                    if *drops > MAX_CONSECUTIVE_DROPS {
                        drop_counts.remove(conn_id);
                        false
                    } else {
                        true
                    }
                }
                Err(LinkError::Closed) => {
                    drop_counts.remove(conn_id);
                    false
                }
            });
    }
}

pub fn spawn_relay(cfg: RelayConfig) -> (RelayHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(1024);
    let handle = RelayHandle::new(tx.clone());

    let port = cfg.port;
    if port > 0 {
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let addr = format!("0.0.0.0:{port}");
            if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
                tracing::info!(%addr, "spectator relay listening for DotaTV viewers");
                let mut conn_counter = 100_000u64;
                let (conn_tx, mut conn_rx) = mpsc::channel(256);

                let tx_events = tx_clone.clone();
                tokio::spawn(async move {
                    while let Some(ev) = conn_rx.recv().await {
                        let _ = tx_events.send(RelayCmd::Conn(ev)).await;
                    }
                });

                while let Ok((stream, peer)) = listener.accept().await {
                    conn_counter += 1;
                    tracing::info!(%peer, conn_id = conn_counter, "spectator viewer connected");
                    let link = ghost_net::spawn_conn(conn_counter, stream, conn_tx.clone(), 1024);
                    let _ = tx_clone
                        .send(RelayCmd::ViewerJoined {
                            conn_id: conn_counter,
                            link,
                        })
                        .await;
                }
            } else {
                tracing::warn!(%addr, "failed to bind spectator relay TCP port");
            }
        });
    }

    let join = tokio::spawn(async move {
        run_relay(cfg, rx).await;
    });
    (handle, join)
}

async fn run_relay(cfg: RelayConfig, mut rx: mpsc::Receiver<RelayCmd>) {
    let mut relay = Relay::new(cfg);
    let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(RelayCmd::Shutdown) | None => break,
                    Some(RelayCmd::ViewerJoined { conn_id, link }) => {
                        if relay.add_viewer(conn_id, link.clone()).is_ok() {
                            if let Ok(hello) = encode_hello(1, "ghostrs") {
                                let _ = link.try_send(hello);
                            }
                            if let Some(snap) = &relay.snapshot
                                && let Ok(snap_bytes) = encode_snapshot(snap)
                            {
                                let _ = link.try_send(snap_bytes);
                            }
                            for p in &relay.players {
                                if let Ok(p_bytes) = encode_player(p.pid, &p.name, p.colour, p.team, p.race) {
                                    let _ = link.try_send(p_bytes);
                                }
                            }
                            for action in &relay.history {
                                if let Ok(act_bytes) = encode_action(action) {
                                    let _ = link.try_send(act_bytes);
                                }
                            }
                            if let Ok(end_bytes) = encode_history_end(relay.history.len() as u32) {
                                let _ = link.try_send(end_bytes);
                            }
                        }
                    }
                    Some(RelayCmd::ViewerLeft(conn_id)) => {
                        relay.remove_viewer(conn_id);
                    }
                    Some(RelayCmd::Conn(ev)) => {
                        relay.handle_conn_event(ev);
                    }
                    Some(RelayCmd::ViewerChat { sender, text }) => {
                        if let Ok(pkt) = encode_chat(&sender, &text) {
                            relay.broadcast(&pkt);
                        }
                    }
                    Some(RelayCmd::GameBlock(block)) => {
                        // The block is NOT added to history here. History holds only
                        // blocks already released to live viewers; a block still sitting
                        // in delayed_blocks is in every viewer's future. Recording it now
                        // would send it twice to anyone joining inside the delay window:
                        // once in the history burst, then again on release. Warcraft III
                        // would simulate that timeslot twice and desync. It joins history
                        // in release_due_blocks instead.
                        let release_at = Instant::now() + relay.cfg.delay;
                        relay.delayed_blocks.push_back((release_at, block));
                        relay.release_due_blocks();
                    }
                    Some(RelayCmd::GameStart(snap)) => {
                        relay.snapshot = Some(snap);
                    }
                    Some(RelayCmd::PlayerInfo { pid, name, colour, team, race }) => {
                        if let Some(p) = relay.players.iter_mut().find(|p| p.pid == pid) {
                            p.name = name;
                            p.colour = colour;
                            p.team = team;
                            p.race = race;
                        } else {
                            relay.players.push(DotaPlayerInfo {
                                pid,
                                name,
                                colour,
                                team,
                                race,
                            });
                        }
                    }
                    Some(RelayCmd::GameOver) => {
                        // Flush any remaining blocks
                        while let Some((_, block)) = relay.delayed_blocks.pop_front() {
                            if let Ok(framed) = encode_action(&block) {
                                relay.broadcast(&framed);
                            } else {
                                relay.broadcast(&block);
                            }
                            relay.released_count += 1;
                        }
                        relay.history.clear();
                        relay.history_bytes = 0;
                    }
                    Some(RelayCmd::DebugGetReleasedCount(resp)) => {
                        relay.release_due_blocks();
                        let _ = resp.send(relay.released_count);
                    }
                }
            }

            _ = tick_interval.tick() => {
                relay.release_due_blocks();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_net::CloseReason;
    use ghost_protocol::dotatv::{
        decode_action, decode_chat, decode_hello, decode_history_end, decode_player,
        decode_snapshot,
    };
    use ghost_protocol::frame::Frame;
    use ghost_protocol::w3gs::slot::SlotInfo;

    fn test_link() -> PlayerLink {
        let (tx, _rx) = mpsc::channel(64);
        PlayerLink::for_test(tx)
    }

    fn sample_snapshot() -> GameStartSnapshot {
        GameStartSnapshot {
            game_name: "DotA 5v5 Live".into(),
            map_path: "Maps\\Download\\DotA v6.83d.w3x".into(),
            map_size: 8_388_608,
            map_info_crc: 0x1122_3344,
            map_crc: 0x5566_7788,
            map_sha1: [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
            ],
            stat_string: vec![0x10, 0x20, 0x30],
            random_seed: 123_456_789,
            layout_style: 0,
            player_slots: 10,
            war3_version: 26,
            is_tft: true,
            slots: vec![
                SlotInfo {
                    pid: 1,
                    download_status: 100,
                    slot_status: 2,
                    computer: 0,
                    team: 0,
                    colour: 1,
                    race: 1,
                    computer_type: 0,
                    handicap: 100,
                },
                SlotInfo {
                    pid: 2,
                    download_status: 100,
                    slot_status: 2,
                    computer: 0,
                    team: 1,
                    colour: 2,
                    race: 2,
                    computer_type: 0,
                    handicap: 100,
                },
            ],
        }
    }

    #[tokio::test(start_paused = true)]
    async fn blocks_are_released_only_after_the_configured_delay() {
        let (handle, _join) = spawn_relay(RelayConfig {
            port: 0,
            delay: Duration::from_secs(120),
            max_viewers: 8,
            game_name: "t".into(),
            history_max_mb: 64,
        });
        handle.push(Bytes::from_static(&[1, 2, 3]));
        tokio::task::yield_now().await;

        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(handle.debug_released_count().await, 0);

        tokio::time::advance(Duration::from_secs(61)).await;
        assert_eq!(handle.debug_released_count().await, 1);
    }

    #[tokio::test]
    async fn viewers_beyond_the_limit_are_refused() {
        let cfg = RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 2,
            game_name: "t".into(),
            history_max_mb: 64,
        };
        let mut relay = Relay::new(cfg);
        assert_eq!(relay.add_viewer(1, test_link()), Ok(()));
        assert_eq!(relay.add_viewer(2, test_link()), Ok(()));
        assert_eq!(relay.add_viewer(3, test_link()), Err(RelayError::Full));
    }

    #[test]
    fn backpressure_does_not_drop_viewer_immediately_until_threshold() {
        let cfg = RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 10,
            game_name: "t".into(),
            history_max_mb: 64,
        };
        let mut relay = Relay::new(cfg);

        // Create a link with channel capacity 1
        let (tx, _rx) = mpsc::channel(1);
        let link = PlayerLink::for_test(tx);
        relay.add_viewer(1, link).unwrap();

        let pkt = Bytes::from_static(&[1, 2, 3]);

        // 1st broadcast succeeds (fills channel)
        relay.broadcast(&pkt);
        assert_eq!(relay.viewers.len(), 1);
        assert_eq!(relay.drop_counts.get(&1), None);

        // 2nd broadcast experiences backpressure, but does NOT drop viewer
        relay.broadcast(&pkt);
        assert_eq!(relay.viewers.len(), 1);
        assert_eq!(relay.drop_counts.get(&1), Some(&1));

        // Send up to threshold
        for _ in 0..199 {
            relay.broadcast(&pkt);
        }
        assert_eq!(relay.viewers.len(), 1);
        assert_eq!(relay.drop_counts.get(&1), Some(&200));

        // One more exceeds threshold -> drops viewer
        relay.broadcast(&pkt);
        assert_eq!(relay.viewers.len(), 0);
        assert_eq!(relay.drop_counts.get(&1), None);

        // Verify successful send resets drop count
        let (tx2, mut rx2) = mpsc::channel(1);
        let link2 = PlayerLink::for_test(tx2);
        relay.add_viewer(2, link2).unwrap();
        relay.broadcast(&pkt); // fills channel
        relay.broadcast(&pkt); // drop 1
        assert_eq!(relay.drop_counts.get(&2), Some(&1));
        // drain channel
        let _ = rx2.try_recv();
        relay.broadcast(&pkt); // succeeds -> resets drop count
        assert_eq!(relay.drop_counts.get(&2), None);
        assert_eq!(relay.viewers.len(), 1);
    }

    #[tokio::test]
    async fn joining_viewer_receives_exact_ordered_sequence_of_0xfd_message_ids_and_decoded_payloads()
     {
        // Zero delay so the pushed blocks are released - and therefore enter
        // history - before the viewer joins. A block still inside the delay
        // window is deliberately NOT in the catch-up burst; see
        // a_viewer_joining_inside_the_delay_window_receives_each_block_exactly_once.
        let (handle, _join) = spawn_relay(RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 4,
            game_name: "DotA 5v5 Live".into(),
            history_max_mb: 64,
        });

        let expected_snap = sample_snapshot();
        handle.send_game_start(expected_snap.clone());

        handle.send_player_info(1, "PlayerOne", 1, 0, 1);
        handle.send_player_info(2, "PlayerTwo", 2, 1, 2);

        let block1 = Bytes::from_static(&[0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00]);
        let block2 = Bytes::from_static(&[0xF7, 0x48, 0x06, 0x00, 0x00, 0x00]);
        let block3 = Bytes::from_static(&[0xF7, 0x0C, 0x08, 0x00, 0x64, 0x00, 0xAA, 0xBB]);

        handle.push(block1.clone());
        handle.push(block2.clone());
        handle.push(block3.clone());

        // Allow actor to process game start, player infos, and game blocks
        tokio::time::sleep(Duration::from_millis(50)).await;

        let (viewer_tx, mut viewer_rx) = mpsc::channel(64);
        let link = PlayerLink::for_test(viewer_tx);

        let _ = handle
            .tx
            .send(RelayCmd::ViewerJoined {
                conn_id: 1001,
                link,
            })
            .await;

        // Drain all messages sent to the viewer
        let mut frames = Vec::new();
        for _ in 0..8 {
            let pkt = tokio::time::timeout(Duration::from_millis(200), viewer_rx.recv())
                .await
                .expect("timeout waiting for frame")
                .expect("channel closed unexpectedly");
            frames.push(pkt);
        }

        // (a) Assert exact ordered sequence of 0xFD message IDs
        let received_ids: Vec<u8> = frames.iter().map(|f| f[1]).collect();
        assert_eq!(
            received_ids,
            vec![
                dotatv_ids::HELLO,               // 0x01
                dotatv_ids::GAME_START_SNAPSHOT, // 0x02
                dotatv_ids::PLAYER,              // 0x03
                dotatv_ids::PLAYER,              // 0x03
                dotatv_ids::ACTION,              // 0x04
                dotatv_ids::ACTION,              // 0x04
                dotatv_ids::ACTION,              // 0x04
                dotatv_ids::HISTORY_END,         // 0x07
            ]
        );
        assert_eq!(
            received_ids,
            vec![0x01, 0x02, 0x03, 0x03, 0x04, 0x04, 0x04, 0x07]
        );

        // Frame 0: HELLO
        assert_eq!(frames[0][0], 0xFD);
        let (hello_ver, hello_server) = decode_hello(&frames[0][4..]).unwrap();
        assert_eq!(hello_ver, 1);
        assert_eq!(hello_server, "ghostrs");

        // Frame 1: GAME_START_SNAPSHOT
        assert_eq!(frames[1][0], 0xFD);
        let decoded_snap = decode_snapshot(&frames[1][4..]).unwrap();
        assert_eq!(decoded_snap, expected_snap);

        // Frame 2: PLAYER 1
        assert_eq!(frames[2][0], 0xFD);
        let p1 = decode_player(&frames[2][4..]).unwrap();
        assert_eq!(
            p1,
            DotaPlayerInfo {
                pid: 1,
                name: "PlayerOne".into(),
                colour: 1,
                team: 0,
                race: 1,
            }
        );

        // Frame 3: PLAYER 2
        assert_eq!(frames[3][0], 0xFD);
        let p2 = decode_player(&frames[3][4..]).unwrap();
        assert_eq!(
            p2,
            DotaPlayerInfo {
                pid: 2,
                name: "PlayerTwo".into(),
                colour: 2,
                team: 1,
                race: 2,
            }
        );

        // (b) Assert history action frames match exact payloads that were pushed in order
        assert_eq!(frames[4][0], 0xFD);
        let act1 = decode_action(&frames[4][4..]).unwrap();
        assert_eq!(act1, block1);

        assert_eq!(frames[5][0], 0xFD);
        let act2 = decode_action(&frames[5][4..]).unwrap();
        assert_eq!(act2, block2);

        assert_eq!(frames[6][0], 0xFD);
        let act3 = decode_action(&frames[6][4..]).unwrap();
        assert_eq!(act3, block3);

        // Frame 7: HISTORY_END
        assert_eq!(frames[7][0], 0xFD);
        let history_count = decode_history_end(&frames[7][4..]).unwrap();
        assert_eq!(history_count, 3);
    }

    /// A viewer joining while blocks are still inside the delay window must
    /// receive each block exactly once. Before this was fixed, a block was
    /// recorded into history the moment it arrived while a copy stayed queued
    /// in delayed_blocks, so a joiner got it in the catch-up burst AND again
    /// when the delay expired. Warcraft III would simulate that timeslot twice
    /// and desync - with the default 120 s delay that hit every single viewer.
    #[tokio::test(start_paused = true)]
    async fn a_viewer_joining_inside_the_delay_window_receives_each_block_exactly_once() {
        let (handle, _join) = spawn_relay(RelayConfig {
            port: 0,
            delay: Duration::from_secs(120),
            max_viewers: 4,
            game_name: "Delayed Stream".into(),
            history_max_mb: 64,
        });

        let block = Bytes::from_static(&[0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00]);
        handle.push(block.clone());
        tokio::task::yield_now().await;

        // Join while the block is still queued and undelivered.
        let (viewer_tx, mut viewer_rx) = mpsc::channel(64);
        let _ = handle
            .tx
            .send(RelayCmd::ViewerJoined {
                conn_id: 2001,
                link: PlayerLink::for_test(viewer_tx),
            })
            .await;
        tokio::task::yield_now().await;

        // Let the 120 s delay elapse so the block is released live.
        tokio::time::advance(Duration::from_secs(121)).await;
        let _ = handle.debug_released_count().await;
        tokio::task::yield_now().await;

        let mut frames = Vec::new();
        while let Ok(pkt) = viewer_rx.try_recv() {
            frames.push(pkt);
        }

        let ids: Vec<u8> = frames.iter().map(|f| f[1]).collect();
        assert_eq!(
            ids,
            vec![
                dotatv_ids::HELLO,
                dotatv_ids::HISTORY_END,
                dotatv_ids::ACTION,
            ],
            "the queued block must arrive once, live, after an empty catch-up burst"
        );

        let expected_action = encode_action(&block).expect("encode action");
        assert_eq!(
            frames[2], expected_action,
            "the single ACTION frame must be the block that was pushed"
        );
    }

    #[tokio::test]
    async fn player_info_command_stores_player_and_delivers_to_subsequent_joiner() {
        let (handle, _join) = spawn_relay(RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 4,
            game_name: "Live Game".into(),
            history_max_mb: 64,
        });

        handle.send_player_info(4, "ShadowFiend", 7, 1, 2);
        tokio::time::sleep(Duration::from_millis(50)).await;

        let (viewer_tx, mut viewer_rx) = mpsc::channel(16);
        let link = PlayerLink::for_test(viewer_tx);
        let _ = handle
            .tx
            .send(RelayCmd::ViewerJoined {
                conn_id: 2002,
                link,
            })
            .await;

        // Message 0: HELLO
        let hello = tokio::time::timeout(Duration::from_millis(200), viewer_rx.recv())
            .await
            .expect("timeout")
            .expect("channel open");
        assert_eq!(hello[1], dotatv_ids::HELLO);

        // Message 1: PLAYER (since snapshot was not set)
        let player_frame = tokio::time::timeout(Duration::from_millis(200), viewer_rx.recv())
            .await
            .expect("timeout")
            .expect("channel open");
        assert_eq!(player_frame[1], dotatv_ids::PLAYER);

        let decoded = decode_player(&player_frame[4..]).unwrap();
        assert_eq!(
            decoded,
            DotaPlayerInfo {
                pid: 4,
                name: "ShadowFiend".into(),
                colour: 7,
                team: 1,
                race: 2,
            }
        );

        // Message 2: HISTORY_END
        let history_end = tokio::time::timeout(Duration::from_millis(200), viewer_rx.recv())
            .await
            .expect("timeout")
            .expect("channel open");
        assert_eq!(history_end[1], dotatv_ids::HISTORY_END);
        assert_eq!(decode_history_end(&history_end[4..]).unwrap(), 0);
    }

    #[tokio::test]
    async fn history_buffer_cap_refuses_new_viewers_while_existing_viewers_continue_streaming() {
        let mut relay = Relay::new(RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 10,
            game_name: "Cap Test".into(),
            history_max_mb: 1, // 1 MB cap = 1,048,576 bytes
        });

        // 1. First viewer connects successfully under limit
        let (v1_tx, mut v1_rx) = mpsc::channel(16);
        let link1 = PlayerLink::for_test(v1_tx);
        assert_eq!(relay.add_viewer(1, link1), Ok(()));
        assert_eq!(relay.viewers.len(), 1);

        // 2. Push blocks exceeding 1 MB cap
        let big_block = Bytes::from(vec![0xAA; 1_048_580]);
        relay.history_bytes = relay.history_bytes.saturating_add(big_block.len());
        relay.history.push(big_block);
        assert_eq!(relay.history_bytes, 1_048_580);

        // 3. New viewer is refused with HistoryLimitExceeded
        let (v2_tx, _v2_rx) = mpsc::channel(16);
        let link2 = PlayerLink::for_test(v2_tx);
        assert_eq!(
            relay.add_viewer(2, link2),
            Err(RelayError::HistoryLimitExceeded)
        );
        assert_eq!(relay.viewers.len(), 1);

        // 4. Existing connected viewer still receives live frames
        let live_block = Bytes::from_static(&[0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00]);
        let release_at = Instant::now();
        relay
            .delayed_blocks
            .push_back((release_at, live_block.clone()));
        relay.release_due_blocks();

        let received = v1_rx.try_recv().expect("viewer 1 must receive live frame");
        assert_eq!(received[0], 0xFD);
        assert_eq!(received[1], dotatv_ids::ACTION);
        assert_eq!(decode_action(&received[4..]).unwrap(), live_block);
    }

    #[test]
    fn closed_connection_event_removes_viewer_from_active_list() {
        let mut relay = Relay::new(RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 10,
            game_name: "Close Test".into(),
            history_max_mb: 64,
        });

        relay.add_viewer(10, test_link()).unwrap();
        relay.add_viewer(20, test_link()).unwrap();
        let current_ids: Vec<u64> = relay.viewers.iter().map(|(id, _)| *id).collect();
        assert_eq!(current_ids, vec![10, 20]);

        // Dispatch Closed event for conn_id 10
        relay.handle_conn_event(ConnEvent {
            conn_id: 10,
            kind: ConnEventKind::Closed(CloseReason::PeerClosed),
        });

        let remaining_ids: Vec<u64> = relay.viewers.iter().map(|(id, _)| *id).collect();
        assert_eq!(remaining_ids, vec![20]);
        assert_eq!(relay.drop_counts.get(&10), None);

        // Dispatch Closed event for conn_id 20
        relay.handle_conn_event(ConnEvent {
            conn_id: 20,
            kind: ConnEventKind::Closed(CloseReason::Io("reset".into())),
        });

        assert_eq!(relay.viewers.len(), 0);
        let empty_ids: Vec<u64> = relay.viewers.iter().map(|(id, _)| *id).collect();
        assert_eq!(empty_ids, Vec::<u64>::new());
        assert_eq!(relay.drop_counts.get(&20), None);
    }

    #[test]
    fn inbound_client_chat_frame_broadcasts_spectator_chat() {
        let mut relay = Relay::new(RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 10,
            game_name: "Chat Test".into(),
            history_max_mb: 64,
        });

        let (v1_tx, _v1_rx) = mpsc::channel(16);
        let (v2_tx, mut v2_rx) = mpsc::channel(16);
        relay.add_viewer(100, PlayerLink::for_test(v1_tx)).unwrap();
        relay.add_viewer(200, PlayerLink::for_test(v2_tx)).unwrap();

        // Inbound 0x81 CLIENT_CHAT from conn_id 100
        let chat_bytes = dotatv::encode_client_chat("Good game everyone").unwrap();
        let frame = Frame::new(dotatv_ids::CLIENT_CHAT, chat_bytes.slice(4..));

        relay.handle_conn_event(ConnEvent {
            conn_id: 100,
            kind: ConnEventKind::Frame(AnyFrame::W3gs(frame)),
        });

        let broadcasted = v2_rx.try_recv().expect("viewer 200 must receive chat");
        assert_eq!(broadcasted[0], 0xFD);
        assert_eq!(broadcasted[1], dotatv_ids::CHAT);

        let decoded = decode_chat(&broadcasted[4..]).unwrap();
        assert_eq!(decoded.sender, "Viewer-100");
        assert_eq!(decoded.text, "Good game everyone");
    }

    #[test]
    fn game_over_command_clears_history_buffer_and_flushes_pending_blocks() {
        let mut relay = Relay::new(RelayConfig {
            port: 0,
            delay: Duration::from_secs(100),
            max_viewers: 10,
            game_name: "Game Over Test".into(),
            history_max_mb: 64,
        });

        let (v_tx, mut v_rx) = mpsc::channel(16);
        relay.add_viewer(50, PlayerLink::for_test(v_tx)).unwrap();

        let block = Bytes::from_static(&[0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00]);
        relay.history_bytes += block.len();
        relay.history.push(block.clone());
        relay
            .delayed_blocks
            .push_back((Instant::now() + Duration::from_secs(100), block.clone()));

        assert_eq!(relay.history.len(), 1);
        assert_eq!(relay.history_bytes, 6);
        assert_eq!(relay.delayed_blocks.len(), 1);

        // Simulate GameOver command handling
        while let Some((_, b)) = relay.delayed_blocks.pop_front() {
            if let Ok(framed) = encode_action(&b) {
                relay.broadcast(&framed);
            }
            relay.released_count += 1;
        }
        relay.history.clear();
        relay.history_bytes = 0;

        assert_eq!(relay.history.len(), 0);
        assert_eq!(relay.history_bytes, 0);
        assert_eq!(relay.delayed_blocks.len(), 0);
        assert_eq!(relay.released_count, 1);

        let received = v_rx.try_recv().expect("viewer must receive flushed block");
        assert_eq!(received[0], 0xFD);
        assert_eq!(received[1], dotatv_ids::ACTION);
        assert_eq!(decode_action(&received[4..]).unwrap(), block);
    }
}
