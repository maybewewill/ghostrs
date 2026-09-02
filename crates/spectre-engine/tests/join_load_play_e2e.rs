use bytes::{BufMut, Bytes, BytesMut};
use spectre_engine::handle::GameCmd;
use spectre_engine::state::{COUNTDOWN_TOTAL, GameConfig, GamePhase, GameState, MapInfo};
use spectre_net::PlayerLink;
use spectre_protocol::w3gs::ids;
use std::time::Duration;
use tokio::sync::mpsc;

fn test_game_cfg() -> GameConfig {
    GameConfig {
        name: "E2E Test Match".into(),
        owner: "HostPlayer".into(),
        host_counter: 1,
        num_slots: 10,
        latency: Duration::from_millis(50),
        sync_limit: 50,
        map: MapInfo::test_default(),
        virtual_host_name: "|cFF4080C0Spectre".into(),
        reconnect_wait: Duration::from_secs(180),
        custom_slots: None,
        replay_path: std::path::PathBuf::from("replays/e2e.w3g"),
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
        hcl_from_game_name: true,
        votekick_allowed: true,
        votekick_percentage: 100,
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

fn drain_all_ids(rx: &mut mpsc::Receiver<Bytes>) -> Vec<u8> {
    let mut ids = Vec::new();
    while let Ok(b) = rx.try_recv() {
        if b.len() >= 2 {
            ids.push(b[1]);
        }
    }
    ids
}

#[tokio::test]
async fn full_join_mapcheck_countdown_load_play_lifecycle() {
    let mut st = GameState::new(test_game_cfg());
    st.create_virtual_host();

    let mut rxs = Vec::new();
    for i in 1..=3 {
        let (tx, mut rx) = mpsc::channel(64);
        let conn_id = i as u64;
        st.add_conn(conn_id, PlayerLink::for_test(tx), [127, 0, 0, i as u8]);
        st.handle_req_join(conn_id, &make_reqjoin(&format!("Player_{i}")));

        let received = drain_all_ids(&mut rx);
        assert!(
            received.contains(&ids::SLOT_INFO_JOIN),
            "player {i} must receive SLOT_INFO_JOIN"
        );
        assert!(
            received.contains(&ids::MAP_CHECK),
            "player {i} must receive MAP_CHECK"
        );
        rxs.push(rx);
    }
    assert_eq!(st.players.len(), 4);
    assert_eq!(st.phase, GamePhase::Lobby);

    for i in 1..=3 {
        let conn_id = i as u64;
        let mut p = BytesMut::new();
        p.put_slice(&[0; 4]);
        p.put_u8(1);
        p.put_u32_le(st.cfg.map.size);
        st.handle_map_size(conn_id, &p.freeze());
    }
    assert!(
        st.players
            .iter()
            .filter(|p| !p.virtual_host)
            .all(|p| p.download_status == 100)
    );

    st.handle_cmd(GameCmd::Start {
        by: "HostPlayer".into(),
    });
    assert!(matches!(st.phase, GamePhase::Countdown { .. }));

    for rx in &mut rxs {
        let _ = drain_all_ids(rx);
    }

    if let GamePhase::Countdown {
        ref mut started_at, ..
    } = st.phase
    {
        *started_at = std::time::Instant::now() - COUNTDOWN_TOTAL - Duration::from_millis(100);
    }
    st.on_tick(0);
    assert_eq!(st.phase, GamePhase::Loading);
    assert_eq!(st.virtual_host_pid, 255);

    for (i, rx) in rxs.iter_mut().enumerate() {
        let received = drain_all_ids(rx);
        assert!(
            received.contains(&ids::COUNTDOWN_START),
            "player {} missing COUNTDOWN_START",
            i + 1
        );
        assert!(
            received.contains(&ids::COUNTDOWN_END),
            "player {} missing COUNTDOWN_END",
            i + 1
        );
    }

    for i in 1..=3 {
        st.handle_loaded(i as u64);
    }
    assert_eq!(st.phase, GamePhase::Playing);

    st.on_tick(0);
    for (i, rx) in rxs.iter_mut().enumerate() {
        let received = drain_all_ids(rx);
        assert!(
            received.contains(&ids::INCOMING_ACTION),
            "player {} missing INCOMING_ACTION",
            i + 1
        );
    }

    let mut act_payload = BytesMut::new();
    act_payload.put_u32_le(0xABCD_1234);
    act_payload.put_slice(&[0x01, 0x02, 0x03, 0x04]);
    st.handle_action(1, &act_payload.freeze());

    st.on_tick(0);
    for (i, rx) in rxs.iter_mut().enumerate() {
        let received = drain_all_ids(rx);
        assert!(
            received.contains(&ids::INCOMING_ACTION),
            "player {} missing action tick",
            i + 1
        );
    }
    assert_eq!(st.sync_counter, 2);
}

#[tokio::test]
async fn countdown_aborted_by_leaver_lifecycle() {
    let mut st = GameState::new(test_game_cfg());
    let (tx1, _rx1) = mpsc::channel(64);
    let (tx2, mut rx2) = mpsc::channel(64);

    st.add_conn(1, PlayerLink::for_test(tx1), [127, 0, 0, 1]);
    st.handle_req_join(1, &make_reqjoin("Player_1"));
    st.add_conn(2, PlayerLink::for_test(tx2), [127, 0, 0, 2]);
    st.handle_req_join(2, &make_reqjoin("Player_2"));

    st.handle_cmd(GameCmd::Start {
        by: "Player_1".into(),
    });
    assert!(matches!(st.phase, GamePhase::Countdown { .. }));

    let _ = drain_all_ids(&mut rx2);

    st.handle_leave(1, 13);
    st.reap_left_players();

    assert_eq!(st.phase, GamePhase::Lobby);
    assert_eq!(st.players.len(), 1);

    let received = drain_all_ids(&mut rx2);
    assert!(received.contains(&ids::CHAT_FROM_HOST));
    assert!(received.contains(&ids::SLOT_INFO));
}

#[tokio::test]
async fn loading_timeout_and_leaver_recovery_lifecycle() {
    let mut st = GameState::new(test_game_cfg());
    let (tx1, mut rx1) = mpsc::channel(64);
    let (tx2, _rx2) = mpsc::channel(64);
    let (tx3, _rx3) = mpsc::channel(64);

    st.add_conn(1, PlayerLink::for_test(tx1), [127, 0, 0, 1]);
    st.handle_req_join(1, &make_reqjoin("Player_1"));
    st.add_conn(2, PlayerLink::for_test(tx2), [127, 0, 0, 2]);
    st.handle_req_join(2, &make_reqjoin("Player_2"));
    st.add_conn(3, PlayerLink::for_test(tx3), [127, 0, 0, 3]);
    st.handle_req_join(3, &make_reqjoin("Player_3"));

    st.begin_loading();
    assert_eq!(st.phase, GamePhase::Loading);

    st.handle_loaded(1);
    assert_eq!(st.phase, GamePhase::Loading);

    st.handle_leave(2, 13);
    st.reap_left_players();

    assert_eq!(st.phase, GamePhase::Loading);

    st.started_loading_at = Some(std::time::Instant::now() - Duration::from_secs(65));
    st.on_tick(0);

    assert_eq!(st.phase, GamePhase::Playing);
    assert_eq!(st.players.len(), 1);

    st.on_tick(0);
    let received = drain_all_ids(&mut rx1);
    assert!(received.contains(&ids::INCOMING_ACTION));
}
