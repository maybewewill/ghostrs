use bytes::{BufMut, Bytes, BytesMut};
use spectre_engine::actor::tests_support::{drain_ids, reqjoin_bytes, seated_game, test_cfg};
use spectre_engine::chat::ChatCommand;
use spectre_engine::mapxfer::Download;
use spectre_engine::state::{GamePhase, GameState};
use spectre_net::{AnyFrame, PlayerLink};
use spectre_protocol::frame::Frame;
use spectre_protocol::w3gs::ids;
use spectre_store::Store;
use std::time::Duration;
use tokio::sync::mpsc;

fn make_chat_to_host(from_pid: u8, msg: &str) -> Bytes {
    let mut b = BytesMut::new();
    b.put_u8(1);
    b.put_u8(0);
    b.put_u8(from_pid);
    b.put_u8(0x10);
    b.put_slice(msg.as_bytes());
    b.put_u8(0);
    b.freeze()
}

#[test]
fn parity_c1_start_and_abort_restricted_to_owner() {
    let (mut st, mut rxs) = seated_game(2);
    st.cfg.owner = "RootAdmin".into();
    st.created_at = std::time::Instant::now() - Duration::from_secs(10);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }

    let chat_p1 = make_chat_to_host(1, "!start");
    st.handle_chat_to_host(1, &chat_p1);
    assert_eq!(st.phase, GamePhase::Lobby, "Non-owner cannot start game");

    let (tx_owner, _rx_owner) = mpsc::channel(64);
    st.add_conn(99, PlayerLink::for_test(tx_owner), [127, 0, 0, 1]);
    st.on_frame(
        99,
        AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("RootAdmin"))),
    );

    for p in st.players.iter_mut() {
        p.record_ping(50);
        p.record_ping(50);
        p.record_ping(50);
    }

    let chat_owner = make_chat_to_host(3, "!start");
    st.handle_chat_to_host(99, &chat_owner);
    assert!(
        matches!(st.phase, GamePhase::Countdown { .. }),
        "Owner can start game"
    );
}

#[test]
fn parity_c2_start_without_force_checks_downloads_and_leaver_cooldown() {
    let (mut st, _rxs) = seated_game(2);
    st.cfg.owner = "slash".into();
    st.cfg.map.size = 50_000;
    st.cfg.map.data = Some(std::sync::Arc::new(vec![0; 50_000]));

    let mut d = Download::new(2);
    d.sent_upto = 10_000;
    st.downloads.push(d);

    st.run_command(1, "slash", ChatCommand::Start { force: false });
    assert_eq!(
        st.phase,
        GamePhase::Lobby,
        "Start without force must reject while player downloads"
    );

    st.run_command(1, "slash", ChatCommand::Start { force: true });
    assert!(
        matches!(st.phase, GamePhase::Countdown { .. }),
        "Start with force must bypass download check"
    );

    st.run_command(1, "slash", ChatCommand::Abort);
    assert_eq!(st.phase, GamePhase::Lobby);

    st.downloads.clear();
    st.last_player_left_time = Some(std::time::Instant::now());

    st.run_command(1, "slash", ChatCommand::Start { force: false });
    assert_eq!(
        st.phase,
        GamePhase::Lobby,
        "Start without force must reject if a player left < 2s ago"
    );

    st.run_command(1, "slash", ChatCommand::Start { force: true });
    assert!(
        matches!(st.phase, GamePhase::Countdown { .. }),
        "Start with force must bypass leaver cooldown"
    );
}

#[test]
fn parity_c3_autokick_on_high_ping() {
    let (mut st, _rxs) = seated_game(2);
    st.cfg.autokick_ping = 300;
    st.created_at = std::time::Instant::now() - Duration::from_secs(10);

    for _ in 0..3 {
        let now = st.created_at.elapsed().as_millis() as u32;
        let mut p = BytesMut::new();

        p.put_u32_le(now.saturating_sub(1000));
        st.handle_pong(2, &p.freeze());
    }

    let p2 = st.players.by_pid(2).unwrap();
    assert!(
        p2.left.is_some(),
        "Player 2 must be autokicked for high ping"
    );
    assert!(p2.left.as_ref().unwrap().contains("autokicked"));
}

#[test]
fn parity_c5_reserved_slots_enforced_on_join() {
    let mut st = GameState::new(test_cfg());

    st.holds.insert(0, "VIPPlayer".into());

    let (tx1, _rx1) = mpsc::channel(64);
    st.add_conn(1, PlayerLink::for_test(tx1), [127, 0, 0, 1]);
    st.on_frame(
        1,
        AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("RegularJoe"))),
    );

    let p1 = st
        .players
        .by_name_partial("RegularJoe")
        .expect("RegularJoe seated");
    let p1_sid = st.slots.sid_of_pid(p1.pid).unwrap();
    assert_ne!(p1_sid, 0, "RegularJoe must not take reserved Slot 0");
    assert_eq!(p1_sid, 1, "RegularJoe gets Slot 1");

    let (tx2, _rx2) = mpsc::channel(64);
    st.add_conn(2, PlayerLink::for_test(tx2), [127, 0, 0, 1]);
    st.on_frame(
        2,
        AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("VIPPlayer"))),
    );

    let p2 = st
        .players
        .by_name_partial("VIPPlayer")
        .expect("VIPPlayer seated");
    let p2_sid = st.slots.sid_of_pid(p2.pid).unwrap();
    assert_eq!(p2_sid, 0, "VIPPlayer must be seated in reserved Slot 0");
    assert!(p2.reserved, "VIPPlayer must have reserved flag set");
}

#[test]
fn parity_c8_in_game_commands_openall_closeall_votekick_lock() {
    let (mut st, _rxs) = seated_game(2);
    st.cfg.owner = "slash".into();

    st.run_command(1, "slash", ChatCommand::CloseAll);
    assert_eq!(st.slots.count_open(), 0);

    st.run_command(1, "slash", ChatCommand::OpenAll);
    assert_eq!(st.slots.count_open(), (st.slots.len() - 2) as u32);

    st.run_command(1, "slash", ChatCommand::Lock);
    assert!(st.locked);

    st.run_command(1, "slash", ChatCommand::Unlock);
    assert!(!st.locked);

    st.hcl = Some("-ap".into());
    st.run_command(1, "slash", ChatCommand::ClearHcl);
    assert!(st.hcl.is_none());

    st.run_command(1, "slash", ChatCommand::VoteKick("P2".into()));
    assert_eq!(st.votekick_target, Some(2));
    assert_eq!(st.votekick_votes, vec![1]);

    st.run_command(2, "P2", ChatCommand::Yes);
    assert!(
        st.players.by_pid(2).unwrap().left.is_some(),
        "Player 2 kicked via votekick"
    );
}

#[test]
fn test_p2_1_kick_marks_player_left_without_sending_0x1c() {
    let kick_id = spectre_protocol::w3gs::ids::HOST_KICK_PLAYER;

    let (mut st, mut rxs) = seated_game(2);
    st.cfg.owner = "slash".into();
    st.run_command(1, "slash", ChatCommand::Kick("P2".into()));
    assert!(
        !drain_ids(&mut rxs[1]).contains(&kick_id),
        "!kick must not send 0x1C"
    );
    assert!(
        st.players.by_pid(2).unwrap().left.is_some(),
        "!kick must mark player left"
    );

    let (mut st2, mut rxs2) = seated_game(2);
    st2.cfg.owner = "slash".into();
    st2.run_command(
        1,
        "slash",
        ChatCommand::Ban {
            name: "P2".into(),
            reason: "bad".into(),
        },
    );
    assert!(
        !drain_ids(&mut rxs2[1]).contains(&kick_id),
        "!ban must not send 0x1C"
    );
    assert!(
        st2.players.by_pid(2).unwrap().left.is_some(),
        "!ban must mark player left"
    );

    let (mut st3, mut rxs3) = seated_game(2);
    st3.run_command(1, "slash", ChatCommand::VoteKick("P2".into()));
    st3.run_command(2, "P2", ChatCommand::Yes);
    assert!(
        !drain_ids(&mut rxs3[1]).contains(&kick_id),
        "votekick must not send 0x1C"
    );

    let (mut st4, mut rxs4) = seated_game(2);
    st4.lagging = true;
    st4.players.by_pid_mut(2).unwrap().lagging = true;
    st4.handle_drop_request(1);
    assert!(
        !drain_ids(&mut rxs4[1]).contains(&kick_id),
        "drop must not send 0x1C"
    );
    assert!(
        st4.players.by_pid(2).unwrap().left.is_some(),
        "drop must mark lagger left"
    );
}

#[test]
fn test_p2_3_lobby_timeout_without_reserved_player() {
    let (mut st, _rxs) = seated_game(2);
    st.cfg.lobby_time_limit = 10;
    st.autostart_players = None;

    st.last_reserved_seen = std::time::Instant::now() - std::time::Duration::from_secs(11 * 60);

    st.on_tick(0);
    assert_eq!(
        st.phase,
        GamePhase::Over,
        "Lobby must transition to GamePhase::Over when time limit expires without reserved player"
    );

    let (mut st2, _rxs2) = seated_game(2);
    st2.cfg.lobby_time_limit = 10;
    st2.autostart_players = None;
    st2.players.by_pid_mut(1).unwrap().reserved = true;
    st2.last_reserved_seen = std::time::Instant::now() - std::time::Duration::from_secs(11 * 60);
    st2.on_tick(0);
    assert_eq!(
        st2.phase,
        GamePhase::Lobby,
        "Reserved player resets last_reserved_seen, so lobby stays active"
    );

    let (mut st3, _rxs3) = seated_game(2);
    st3.cfg.lobby_time_limit = 10;
    st3.autostart_players = Some(5);
    st3.last_reserved_seen = std::time::Instant::now() - std::time::Duration::from_secs(11 * 60);
    st3.on_tick(0);
    assert_eq!(
        st3.phase,
        GamePhase::Lobby,
        "When autostart_players > 0, lobby time limit is not enforced"
    );
}

#[tokio::test]
async fn parity_d1_game_and_download_logging_in_store() {
    let (store, _join) = Store::open_in_memory().expect("open test store");
    let mut cfg = test_cfg();
    cfg.store = Some(store.clone());

    let mut st = GameState::new(cfg);
    let (tx1, _rx1) = mpsc::channel(64);
    st.add_conn(1, PlayerLink::for_test(tx1), [192, 168, 1, 50]);
    st.on_frame(
        1,
        AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Alice"))),
    );

    st.cfg.map.size = 1000;
    st.cfg.map.data = Some(std::sync::Arc::new(vec![0; 1000]));
    let mut d = Download::new(1);
    d.acked_upto = 1000;
    st.downloads.push(d);
    st.pump_downloads();

    if let Some(dota) = st.dota.as_mut() {
        dota.winner = 1;
        dota.duration_min = 35;
        dota.duration_sec = 20;
        let mut p = spectre_engine::stats_dota::DotAPlayerStats::new(1);
        p.name = "Alice".into();
        p.kills = 12;
        p.deaths = 2;
        p.assists = 8;
        p.creep_kills = 220;
        p.creep_denies = 15;
        dota.players.insert(1, p);
    }

    st.begin_playing();
    st.players.by_pid_mut(1).unwrap().left = Some("game over".into());
    st.reap_left_players();
    st.on_tick(0);

    assert_eq!(st.phase, GamePhase::Over);
    assert!(st.finished);

    tokio::time::sleep(Duration::from_millis(50)).await;
    let stats = store
        .get_dota_stats("Alice")
        .await
        .expect("DotA stats found in store");
    assert_eq!(stats.games, 1);
    assert_eq!(stats.kills, 12);
    assert_eq!(stats.deaths, 2);
    assert_eq!(stats.assists, 8);
    assert_eq!(stats.creep_kills, 220);
    assert_eq!(stats.creep_denies, 15);
}

#[test]
fn test_p2_7_allow_downloads_modes() {
    let (mut st0, _rxs0) = seated_game(2);
    st0.cfg.allow_downloads = 0;
    st0.cfg.map.size = 100_000;
    st0.cfg.map.data = Some(std::sync::Arc::new(vec![0; 100_000]));
    let mut report0 = BytesMut::new();
    report0.put_u32_le(0);
    report0.put_u8(1);
    report0.put_u32_le(0);
    st0.handle_map_size(2, &report0.freeze());
    assert!(
        st0.players.by_pid(2).is_none(),
        "Player without map must be kicked and reaped in mode 0"
    );

    let (mut st2, _rxs2) = seated_game(2);
    st2.cfg.allow_downloads = 2;
    st2.cfg.map.size = 100_000;
    st2.cfg.map.data = Some(std::sync::Arc::new(vec![0; 100_000]));
    let mut report2 = BytesMut::new();
    report2.put_u32_le(0);
    report2.put_u8(1);
    report2.put_u32_le(0);
    let report2_bytes = report2.freeze();
    st2.handle_map_size(2, &report2_bytes);
    assert_eq!(
        st2.downloads.len(),
        0,
        "Download should not start without permission"
    );

    st2.run_command(1, "slash", ChatCommand::Download("P2".into()));
    assert!(st2.players.by_pid(2).unwrap().download_allowed);
    st2.handle_map_size(2, &report2_bytes);
    assert_eq!(
        st2.downloads.len(),
        1,
        "Download should start after permission granted"
    );
}

#[test]
fn test_p2_7_mute_lobby_and_announcements() {
    let (mut st, mut rxs) = seated_game(2);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }

    st.run_command(1, "slash", ChatCommand::MuteLobby(Some(true)));
    assert!(st.mute_lobby);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }

    let chat = make_chat_to_host(2, "Hello everyone");
    st.handle_chat_to_host(2, &chat);
    assert!(
        rxs[0].try_recv().is_err(),
        "Chat should not be broadcast while lobby is muted"
    );

    st.run_command(1, "slash", ChatCommand::MuteLobby(Some(false)));
    assert!(!st.mute_lobby);

    st.run_command(1, "slash", ChatCommand::Announce("1 Hello Periodic".into()));
    assert_eq!(st.announce_interval, Duration::from_secs(1));
    assert_eq!(st.announce_message.as_deref(), Some("Hello Periodic"));
}

#[test]
fn test_p2_8_in_game_and_lobby_commands() {
    let (mut st, mut rxs) = seated_game(2);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }

    st.run_command(1, "slash", ChatCommand::DbStatus);

    st.run_command(1, "slash", ChatCommand::FakePlayer);
    assert!(
        st.fake_player_pid.is_some(),
        "Fake player should be seated in lobby"
    );
    st.run_command(1, "slash", ChatCommand::FakePlayer);
    assert!(
        st.fake_player_pid.is_none(),
        "Fake player should be removed"
    );

    st.run_command(1, "slash", ChatCommand::FakePlayer);
    st.begin_playing();
    st.run_command(1, "slash", ChatCommand::FpPause);
    assert_eq!(
        st.actions.last().unwrap().data,
        bytes::Bytes::from_static(&[0x01])
    );
    st.run_command(1, "slash", ChatCommand::FpResume);
    assert_eq!(
        st.actions.last().unwrap().data,
        bytes::Bytes::from_static(&[0x02])
    );

    st.run_command(1, "slash", ChatCommand::From);

    st.run_command(1, "slash", ChatCommand::Messages(Some(false)));
    assert!(!st.local_admin_messages);
    st.run_command(1, "slash", ChatCommand::Messages(Some(true)));
    assert!(st.local_admin_messages);

    st.run_command(
        1,
        "slash",
        ChatCommand::SendLan {
            ip: "192.168.1.100".into(),
            port: Some(6112),
        },
    );

    st.phase = GamePhase::Lobby;
    st.run_command(1, "slash", ChatCommand::Pub("New Public Game".into()));
    assert_eq!(st.cfg.name, "New Public Game");
    assert!(st.refresh_rehosted);

    st.refresh_rehosted = false;
    st.run_command(1, "slash", ChatCommand::Priv("New Private Game".into()));
    assert_eq!(st.cfg.name, "New Private Game");
    assert!(st.refresh_rehosted);

    st.run_command(1, "slash", ChatCommand::Stats("P2".into()));
    let last1 = st.players.by_pid(1).unwrap().stats_sent_time;
    assert!(last1.is_some());
    st.run_command(1, "slash", ChatCommand::Stats("P2".into()));
    let last2 = st.players.by_pid(1).unwrap().stats_sent_time;
    assert_eq!(last1, last2, "Second !stats within 5s must be throttled");

    let _ = drain_ids(&mut rxs[0]);
    st.run_command(1, "slash", ChatCommand::Unknown("foobar".into()));
    let reply = rxs[0]
        .try_recv()
        .expect("Caller must receive unknown command reply");
    assert!(reply.windows(7).any(|w| w == b"Unknown"));
}
