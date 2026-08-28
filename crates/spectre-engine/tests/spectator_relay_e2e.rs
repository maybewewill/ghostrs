

use std::time::Duration;

use bytes::{BufMut, Bytes, BytesMut};
use spectre_engine::actions::MAX_ACTION_PAYLOAD;
use spectre_engine::state::{GameConfig, GamePhase, GameState, MapInfo};
use spectre_net::{AnyFrame, PlayerLink};
use spectre_protocol::dotatv::{
    self, DOTATV_HEADER, GameStartSnapshot, PlayerInfo as DotaPlayerInfo, decode_action,
    decode_hello, decode_history_end, decode_player, decode_snapshot, ids as dotatv_ids,
};
use spectre_protocol::frame::Frame;
use spectre_protocol::w3gs::{ActionBlock, ids as w3gs_ids, outgoing};
use spectre_spectator::relay::{Relay, RelayCmd, RelayConfig, RelayHandle};
use tokio::sync::mpsc;
use tokio::time::Instant;

fn test_game_cfg(relay_handle: RelayHandle) -> GameConfig {
    GameConfig {
        name: "DotA 5v5 Live Match".into(),
        owner: "HostPlayer".into(),
        host_counter: 1,
        num_slots: 10,
        latency: Duration::from_millis(100),
        sync_limit: 50,
        map: MapInfo {
            path: "Maps\\Download\\DotA v6.83d.w3x".into(),
            size: 8_388_608,
            info: 0x1122_3344,
            crc: 0x5566_7788,
            sha1: [
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
            ],
            num_players: 10,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0,
            data: None,
            layout_style: 0,
            options: 0,
            map_type: "dota".into(),
            matchmaking_category: String::new(),
            default_hcl: String::new(),
            default_player_score: 1000,
            loading_in_game: false,
            local_path: String::new(),
            max_slots: 24,
        },
        virtual_host_name: "|cFF4080C0Spectre".into(),
        reconnect_wait: Duration::from_secs(180),
        custom_slots: None,
        replay_path: std::path::PathBuf::from("replays/spectator_e2e.w3g"),
        relay: Some(relay_handle),
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

fn make_reqjoin(name: &str) -> Bytes {
    let mut b = BytesMut::new();
    b.put_u32_le(1);
    b.put_u32_le(0);
    b.put_u8(0);
    b.put_u16_le(6112);
    b.put_u32_le(0);
    b.put_slice(name.as_bytes());
    b.put_u8(0);
    b.put_slice(&[0; 6]);
    b.put_slice(&[127, 0, 0, 1]);
    b.freeze()
}

fn spawn_relay_actor(
    cfg: RelayConfig,
) -> (
    RelayHandle,
    mpsc::Sender<RelayCmd>,
    tokio::task::JoinHandle<()>,
) {
    let (tx, mut rx) = mpsc::channel(1024);
    let handle = RelayHandle::new(tx.clone());
    let tx_clone = tx.clone();

    let join = tokio::spawn(async move {
        let mut relay = Relay::new(cfg);
        let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

        loop {
            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(RelayCmd::Shutdown) | None => break,
                        Some(RelayCmd::ViewerJoined { conn_id, link }) => {
                            if relay.add_viewer(conn_id, link.clone()).is_ok() {
                                if let Ok(hello) = dotatv::encode_hello(1, "spectre") {
                                    let _ = link.try_send(hello);
                                }
                                if let Some(snap) = &relay.snapshot
                                    && let Ok(snap_bytes) = dotatv::encode_snapshot(snap)
                                {
                                    let _ = link.try_send(snap_bytes);
                                }
                                for p in &relay.players {
                                    if let Ok(p_bytes) = dotatv::encode_player(
                                        p.pid, &p.name, p.colour, p.team, p.race,
                                    ) {
                                        let _ = link.try_send(p_bytes);
                                    }
                                }
                                for action in &relay.history {
                                    if let Ok(act_bytes) = dotatv::encode_action(action) {
                                        let _ = link.try_send(act_bytes);
                                    }
                                }
                                if let Ok(end_bytes) =
                                    dotatv::encode_history_end(relay.history.len() as u32)
                                {
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
                            if let Ok(pkt) = dotatv::encode_chat(&sender, &text) {
                                relay.broadcast(&pkt);
                            }
                        }
                        Some(RelayCmd::GameBlock(block)) => {
                            let release_at = Instant::now() + relay.cfg.delay;
                            relay.delayed_blocks.push_back((release_at, block));
                            relay.release_due_blocks();
                        }
                        Some(RelayCmd::GameStart(snap)) => {
                            relay.snapshot = Some(snap);
                        }
                        Some(RelayCmd::PlayerInfo {
                            pid,
                            name,
                            colour,
                            team,
                            race,
                        }) => {
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
                            while let Some((_, block)) = relay.delayed_blocks.pop_front() {
                                if let Ok(framed) = dotatv::encode_action(&block) {
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
    });

    (handle, tx_clone, join)
}

#[tokio::test]
async fn spectator_relay_e2e_ordered_stream_snapshot_and_action_overflow() {

    let relay_cfg = RelayConfig {
        port: 0,
        delay: Duration::ZERO,
        max_viewers: 16,
        game_name: "DotA 5v5 Live Match".into(),
        history_max_mb: 64,
    };
    let (relay_handle, relay_cmd_tx, relay_join) = spawn_relay_actor(relay_cfg);

    let mut st = GameState::new(test_game_cfg(relay_handle));
    let mut player_rxs = Vec::new();
    for i in 1..=3 {
        let conn_id = i as u64;
        let (tx, rx) = mpsc::channel(64);
        st.add_conn(conn_id, PlayerLink::for_test(tx), [127, 0, 0, i as u8]);
        st.on_frame(
            conn_id,
            AnyFrame::W3gs(Frame::new(
                w3gs_ids::REQ_JOIN,
                make_reqjoin(&format!("Player_{i}")),
            )),
        );
        player_rxs.push(rx);
    }
    assert_eq!(st.players.len(), 3);
    assert_eq!(st.phase, GamePhase::Lobby);

    let mut raw_stat =
        Vec::with_capacity(64 + st.cfg.map.path.len() + st.cfg.virtual_host_name.len());
    raw_stat.extend_from_slice(&st.cfg.map.flags.to_le_bytes());
    raw_stat.push(0);
    raw_stat.extend_from_slice(&st.cfg.map.width.to_le_bytes());
    raw_stat.extend_from_slice(&st.cfg.map.height.to_le_bytes());
    raw_stat.extend_from_slice(&st.cfg.map.crc.to_le_bytes());
    raw_stat.extend_from_slice(st.cfg.map.path.as_bytes());
    raw_stat.push(0);
    raw_stat.extend_from_slice(st.cfg.virtual_host_name.as_bytes());
    raw_stat.push(0);
    raw_stat.push(0);
    raw_stat.extend_from_slice(&st.cfg.map.sha1);
    let expected_stat_string = spectre_protocol::encode_statstring(&raw_stat);

    let expected_snap = GameStartSnapshot {
        game_name: "DotA 5v5 Live Match".to_string(),
        map_path: "Maps\\Download\\DotA v6.83d.w3x".to_string(),
        map_size: 8_388_608,
        map_info_crc: 0x1122_3344,
        map_crc: 0x5566_7788,
        map_sha1: [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        ],
        stat_string: expected_stat_string.clone(),
        random_seed: st.random_seed,
        layout_style: 0,
        player_slots: 10,
        war3_version: 26,
        is_tft: true,
        slots: st.slots.as_wire().to_vec(),
    };

    st.begin_playing();
    assert_eq!(st.phase, GamePhase::Playing);

    let action1 = ActionBlock {
        pid: 1,
        data: Bytes::from(vec![0xAA; 800]),
    };
    let action2 = ActionBlock {
        pid: 2,
        data: Bytes::from(vec![0xBB; 800]),
    };
    assert!(
        action1.wire_len() + action2.wire_len() > MAX_ACTION_PAYLOAD,
        "two 800-byte actions must exceed MAX_ACTION_PAYLOAD"
    );

    st.actions.push(action1.clone());
    st.actions.push(action2.clone());

    let expected_overflow_pkt =
        outgoing::incoming_action2(&[action1]).expect("build overflow action2 packet");
    let expected_main_pkt1 =
        outgoing::incoming_action(&[action2], 100).expect("build main action packet 1");

    st.on_tick(0);

    let action3 = ActionBlock {
        pid: 3,
        data: Bytes::from(vec![0xCC; 120]),
    };
    st.actions.push(action3.clone());
    let expected_main_pkt2 =
        outgoing::incoming_action(&[action3], 100).expect("build main action packet 2");

    st.on_tick(0);

    let expected_main_pkt3 =
        outgoing::incoming_action(&[], 100).expect("build empty clock tick packet");

    st.on_tick(0);

    assert_eq!(
        expected_overflow_pkt[1],
        w3gs_ids::INCOMING_ACTION2,
        "first action packet must be INCOMING_ACTION2"
    );
    assert_eq!(expected_overflow_pkt[1], 0x48);
    assert_eq!(
        expected_main_pkt1[1],
        w3gs_ids::INCOMING_ACTION,
        "second action packet must be INCOMING_ACTION"
    );
    assert_eq!(expected_main_pkt1[1], 0x0C);
    assert_eq!(expected_main_pkt2[1], 0x0C);
    assert_eq!(expected_main_pkt3[1], 0x0C);

    tokio::time::sleep(Duration::from_millis(50)).await;

    let (viewer_tx, mut viewer_rx) = mpsc::channel(64);
    let viewer_link = PlayerLink::for_test(viewer_tx);

    relay_cmd_tx
        .send(RelayCmd::ViewerJoined {
            conn_id: 1001,
            link: viewer_link,
        })
        .await
        .expect("send ViewerJoined");

    let mut raw_frames = Vec::new();
    for _ in 0..10 {
        let frame_bytes = tokio::time::timeout(Duration::from_millis(500), viewer_rx.recv())
            .await
            .expect("timeout waiting for spectator frame")
            .expect("viewer channel closed unexpectedly");
        raw_frames.push(frame_bytes);
    }
    assert!(
        viewer_rx.try_recv().is_err(),
        "viewer must receive exactly 10 frames"
    );

    let received_ids: Vec<u8> = raw_frames.iter().map(|f| f[1]).collect();
    let expected_ordered_ids = vec![
        dotatv_ids::HELLO,
        dotatv_ids::GAME_START_SNAPSHOT,
        dotatv_ids::PLAYER,
        dotatv_ids::PLAYER,
        dotatv_ids::PLAYER,
        dotatv_ids::ACTION,
        dotatv_ids::ACTION,
        dotatv_ids::ACTION,
        dotatv_ids::ACTION,
        dotatv_ids::HISTORY_END,
    ];
    assert_eq!(received_ids, expected_ordered_ids);
    assert_eq!(
        received_ids,
        vec![0x01, 0x02, 0x03, 0x03, 0x03, 0x04, 0x04, 0x04, 0x04, 0x07]
    );

    for (idx, frame) in raw_frames.iter().enumerate() {
        assert_eq!(
            frame[0], DOTATV_HEADER,
            "frame {idx} header must be 0xFD ({DOTATV_HEADER})"
        );
        let declared_len = u16::from_le_bytes([frame[2], frame[3]]) as usize;
        assert_eq!(
            declared_len,
            frame.len(),
            "frame {idx} length header {declared_len} must match buffer size {}",
            frame.len()
        );
    }

    let (hello_version, hello_server_name) =
        decode_hello(&raw_frames[0][4..]).expect("decode hello");
    assert_eq!(hello_version, 1);
    assert_eq!(hello_server_name, "spectre");

    let decoded_snapshot = decode_snapshot(&raw_frames[1][4..]).expect("decode snapshot");
    assert_eq!(decoded_snapshot.game_name, "DotA 5v5 Live Match");
    assert_eq!(decoded_snapshot.map_path, "Maps\\Download\\DotA v6.83d.w3x");
    assert_eq!(decoded_snapshot.map_size, 8_388_608);
    assert_eq!(decoded_snapshot.map_info_crc, 0x1122_3344);
    assert_eq!(decoded_snapshot.map_crc, 0x5566_7788);
    assert_eq!(
        decoded_snapshot.map_sha1,
        [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20
        ]
    );
    assert_eq!(decoded_snapshot.stat_string, expected_stat_string);
    assert_eq!(decoded_snapshot.random_seed, st.random_seed);
    assert_eq!(decoded_snapshot.layout_style, 0);
    assert_eq!(decoded_snapshot.player_slots, 10);
    assert_eq!(decoded_snapshot.war3_version, 26);
    assert!(decoded_snapshot.is_tft);
    assert_eq!(decoded_snapshot.slots.len(), 10);
    assert_eq!(decoded_snapshot.slots, expected_snap.slots);
    assert_eq!(decoded_snapshot, expected_snap);

    let p1 = decode_player(&raw_frames[2][4..]).expect("decode player 1");
    assert_eq!(
        p1,
        DotaPlayerInfo {
            pid: 1,
            name: "Player_1".to_string(),
            colour: 0,
            team: 0,
            race: 32,
        }
    );

    let p2 = decode_player(&raw_frames[3][4..]).expect("decode player 2");
    assert_eq!(
        p2,
        DotaPlayerInfo {
            pid: 2,
            name: "Player_2".to_string(),
            colour: 1,
            team: 0,
            race: 32,
        }
    );

    let p3 = decode_player(&raw_frames[4][4..]).expect("decode player 3");
    assert_eq!(
        p3,
        DotaPlayerInfo {
            pid: 3,
            name: "Player_3".to_string(),
            colour: 2,
            team: 0,
            race: 32,
        }
    );

    let action_payload_0 = decode_action(&raw_frames[5][4..]).expect("decode action 0 (overflow)");
    assert_eq!(
        action_payload_0, expected_overflow_pkt,
        "action frame 0 must be byte-identical to pushed overflow packet"
    );
    assert_eq!(
        action_payload_0[1], 0x48,
        "action frame 0 must wrap INCOMING_ACTION2 (0x48)"
    );

    let action_payload_1 = decode_action(&raw_frames[6][4..]).expect("decode action 1 (main)");
    assert_eq!(
        action_payload_1, expected_main_pkt1,
        "action frame 1 must be byte-identical to pushed main action packet 1"
    );
    assert_eq!(
        action_payload_1[1], 0x0C,
        "action frame 1 must wrap INCOMING_ACTION (0x0C)"
    );

    let action_payload_2 = decode_action(&raw_frames[7][4..]).expect("decode action 2 (main)");
    assert_eq!(
        action_payload_2, expected_main_pkt2,
        "action frame 2 must be byte-identical to pushed main action packet 2"
    );
    assert_eq!(
        action_payload_2[1], 0x0C,
        "action frame 2 must wrap INCOMING_ACTION (0x0C)"
    );

    let action_payload_3 =
        decode_action(&raw_frames[8][4..]).expect("decode action 3 (empty clock tick)");
    assert_eq!(
        action_payload_3, expected_main_pkt3,
        "action frame 3 must be byte-identical to pushed empty clock tick packet"
    );
    assert_eq!(
        action_payload_3[1], 0x0C,
        "action frame 3 must wrap INCOMING_ACTION (0x0C)"
    );

    let history_count = decode_history_end(&raw_frames[9][4..]).expect("decode history end");
    assert_eq!(
        history_count, 4,
        "HISTORY_END packet count must equal exactly the 4 action frames pushed"
    );

    relay_cmd_tx
        .send(RelayCmd::Shutdown)
        .await
        .expect("shutdown relay");
    relay_join.await.expect("relay join");
}
