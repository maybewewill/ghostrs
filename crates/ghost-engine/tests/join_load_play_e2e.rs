use bytes::{BufMut, Bytes, BytesMut};
use ghost_engine::handle::GameCmd;
use ghost_engine::state::{COUNTDOWN_TOTAL, GameConfig, GamePhase, GameState, MapInfo};
use ghost_net::PlayerLink;
use ghost_protocol::w3gs::ids;
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
        virtual_host_name: "|cFF4080C0Ghost".into(),
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

    // 1. Join 3 players
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
    assert_eq!(st.players.len(), 4); // 3 humans + 1 virtual host
    assert_eq!(st.phase, GamePhase::Lobby);

    // 2. All 3 players send MAP_SIZE
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

    // 3. Start game countdown
    st.handle_cmd(GameCmd::Start {
        by: "HostPlayer".into(),
    });
    assert!(matches!(st.phase, GamePhase::Countdown { .. }));

    // Drain countdown chat announcements
    for rx in &mut rxs {
        let _ = drain_all_ids(rx);
    }

    // Fast-forward countdown duration past total (2.5s)
    if let GamePhase::Countdown {
        ref mut started_at, ..
    } = st.phase
    {
        *started_at = std::time::Instant::now() - COUNTDOWN_TOTAL - Duration::from_millis(100);
    }
    st.on_tick(0);
    assert_eq!(st.phase, GamePhase::Loading);
    assert_eq!(st.virtual_host_pid, 255); // Virtual host removed on loading

    // Verify COUNTDOWN_START and COUNTDOWN_END received
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

    // 4. All 3 players send GAME_LOADED_SELF
    for i in 1..=3 {
        st.handle_loaded(i as u64);
    }
    assert_eq!(st.phase, GamePhase::Playing);

    // 5. Verify action ticks are broadcast to all players
    st.on_tick(0);
    for (i, rx) in rxs.iter_mut().enumerate() {
        let received = drain_all_ids(rx);
        assert!(
            received.contains(&ids::INCOMING_ACTION),
            "player {} missing INCOMING_ACTION",
            i + 1
        );
    }

    // 6. Action propagation test: Player 1 sends an OUTGOING_ACTION
    let mut act_payload = BytesMut::new();
    act_payload.put_u32_le(0xABCD_1234); // CRC
    act_payload.put_slice(&[0x01, 0x02, 0x03, 0x04]); // sample action data
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

    // Start countdown
    st.handle_cmd(GameCmd::Start {
        by: "Player_1".into(),
    });
    assert!(matches!(st.phase, GamePhase::Countdown { .. }));

    // Drain pending packets from rx2
    let _ = drain_all_ids(&mut rx2);

    // Player 1 leaves during countdown
    st.handle_leave(1, 13);
    st.reap_left_players();

    // Must revert to Lobby phase
    assert_eq!(st.phase, GamePhase::Lobby);
    assert_eq!(st.players.len(), 1);

    // Player 2 must receive chat notification and updated slot table
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

    // Begin loading
    st.begin_loading();
    assert_eq!(st.phase, GamePhase::Loading);

    // Player 1 loads
    st.handle_loaded(1);
    assert_eq!(st.phase, GamePhase::Loading);

    // Player 2 disconnects during loading
    st.handle_leave(2, 13);
    st.reap_left_players();
    // Still waiting on Player 3
    assert_eq!(st.phase, GamePhase::Loading);

    // Fast-forward loading timer past 60s timeout for Player 3
    st.started_loading_at = Some(std::time::Instant::now() - Duration::from_secs(65));
    st.on_tick(0);

    // Player 3 timed out and dropped, leaving only Player 1 who is loaded -> game starts!
    assert_eq!(st.phase, GamePhase::Playing);
    assert_eq!(st.players.len(), 1);

    // Action ticks flow to Player 1
    st.on_tick(0);
    let received = drain_all_ids(&mut rx1);
    assert!(received.contains(&ids::INCOMING_ACTION));
}
