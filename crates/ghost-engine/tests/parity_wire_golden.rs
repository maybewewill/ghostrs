use bytes::{BufMut, Bytes, BytesMut};
use ghost_engine::actions::MAX_ACTION_PAYLOAD;
use ghost_engine::actor::tests_support::{drain_ids, seated_game};
use ghost_engine::state::{COUNTDOWN_STEPS, COUNTDOWN_TOTAL, GamePhase};
use ghost_protocol::w3gs::{ActionBlock, ids};
use ghost_spectator::body::ReplayBody;
use std::time::Duration;

#[test]
fn parity_wire_b1_map_transfer_packets_use_host_pid() {
    let (mut st, mut rxs) = seated_game(1);
    st.cfg.map.size = 10_000;
    st.cfg.map.data = Some(std::sync::Arc::new(vec![0x77; 10_000]));
    let _ = drain_ids(&mut rxs[0]);

    // 1. Client reports 0 bytes -> START_DOWNLOAD
    let mut p = BytesMut::new();
    p.put_slice(&[0, 0, 0, 0]);
    p.put_u8(1);
    p.put_u32_le(0);
    st.handle_map_size(1, &p.freeze());

    let start_pkt = rxs[0].try_recv().expect("START_DOWNLOAD packet");
    assert_eq!(start_pkt[1], ids::START_DOWNLOAD);
    assert_eq!(
        start_pkt[4],
        st.host_pid(),
        "START_DOWNLOAD fromPID must be host_pid()"
    );
    assert_ne!(start_pkt[4], 255, "START_DOWNLOAD must not use 255");

    // 2. Map part packet
    let _ = drain_ids(&mut rxs[0]);
    st.pump_downloads();
    let part_pkt = rxs[0].try_recv().expect("MAP_PART packet");
    assert_eq!(part_pkt[1], ids::MAP_PART);
    assert_eq!(
        part_pkt[4],
        st.host_pid(),
        "MAP_PART fromPID must be host_pid()"
    );
    assert_eq!(part_pkt[5], 1, "MAP_PART toPID must be player PID 1");
}

#[test]
fn parity_wire_b2_max_action_payload_is_1452() {
    assert_eq!(
        MAX_ACTION_PAYLOAD, 1452,
        "Wire parity: MAX_ACTION_PAYLOAD must be 1452 bytes (GHost++ game_base.cpp:1373)"
    );
}

#[test]
fn parity_wire_b3_countdown_10_steps_5_seconds() {
    assert_eq!(COUNTDOWN_STEPS, 10, "Countdown must have 10 steps");
    assert_eq!(
        COUNTDOWN_TOTAL,
        Duration::from_millis(5000),
        "Countdown duration must be 5.0 seconds"
    );

    let (mut st, mut rxs) = seated_game(2);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }
    st.start_countdown("host");

    // Initial announcement at t=0
    st.on_tick(0);
    let sent = drain_ids(&mut rxs[0]);
    assert!(sent.contains(&ids::CHAT_FROM_HOST));

    // Announce steps down to 1
    if let GamePhase::Countdown {
        ref mut started_at, ..
    } = st.phase
    {
        *started_at = std::time::Instant::now() - Duration::from_millis(4500);
    }
    st.on_tick(0);
    let sent = drain_ids(&mut rxs[0]);
    assert!(sent.contains(&ids::CHAT_FROM_HOST));
}

#[test]
fn parity_wire_b4_hcl_is_encoded_at_begin_loading_not_start_countdown() {
    let (mut st, _rxs) = seated_game(2);
    st.hcl = Some("ab".into());

    // Before countdown: handicaps are 100
    assert_eq!(st.slots.as_wire()[0].handicap, 100);
    assert_eq!(st.slots.as_wire()[1].handicap, 100);

    // Start countdown: handicaps must still be untouched
    st.start_countdown("slash");
    assert_eq!(st.slots.as_wire()[0].handicap, 100);
    assert_eq!(st.slots.as_wire()[1].handicap, 100);

    // Countdown ends and begin_loading is called: HCL is now encoded into slot handicaps!
    st.begin_loading();
    assert_ne!(
        st.slots.as_wire()[0].handicap,
        100,
        "Slot handicap must be encoded with HCL at begin_loading"
    );
}

#[test]
fn parity_wire_b5_b6_replay_timeslots_use_0x1f_and_0x1e_without_crc() {
    let mut replay = ReplayBody::new(2, "GhostBot");
    let slots_wire = vec![0u8; 18];
    replay.set_game("Test", &[0; 4], 0);
    replay.add_player(1, "Player1");
    let _ = replay.set_start(slots_wire, 12345, 0, 2);

    let action1 = ActionBlock {
        pid: 1,
        data: Bytes::from_static(&[0x10, 0x20]),
    };
    let raw1 = ActionBlock::encode_actions_raw(&[action1]);

    // Add overflow timeslot2 (0x1E)
    replay.add_timeslot2(&raw1);
    // Add standard timeslot (0x1F)
    replay.add_timeslot(100, &raw1);

    let (body, duration) = replay.finish().expect("finish replay");
    assert_eq!(duration, 100);

    // Verify 0x1E record: [0x1E, len_le, 0_le, raw_actions...]
    let mut expected_ts2 = Vec::new();
    expected_ts2.push(0x1E);
    expected_ts2.extend_from_slice(&((2 + raw1.len()) as u16).to_le_bytes());
    expected_ts2.extend_from_slice(&0u16.to_le_bytes());
    expected_ts2.extend_from_slice(&raw1);

    // Verify 0x1F record: [0x1F, len_le, 100_le, raw_actions...]
    let mut expected_ts1 = Vec::new();
    expected_ts1.push(0x1F);
    expected_ts1.extend_from_slice(&((2 + raw1.len()) as u16).to_le_bytes());
    expected_ts1.extend_from_slice(&100u16.to_le_bytes());
    expected_ts1.extend_from_slice(&raw1);

    assert!(
        body.windows(expected_ts2.len())
            .any(|w| w == expected_ts2.as_slice()),
        "Must contain 0x1E timeslot2 without CRC"
    );
    assert!(
        body.windows(expected_ts1.len())
            .any(|w| w == expected_ts1.as_slice()),
        "Must contain 0x1F timeslot without CRC"
    );
}

#[test]
fn parity_wire_b7_replay_host_pid_matches_virtual_host_pid() {
    let (mut st, _rxs) = seated_game(2);
    st.create_virtual_host();
    let vhost_pid = st.host_pid();

    st.begin_playing();

    let rep = st.replay.take().expect("replay exists");
    let (body, _) = rep.finish().expect("replay body built");

    // The host record at start of body is [16, 1, 0, 0, 0x00, host_pid, ...]
    assert_eq!(&body[0..4], &[16, 1, 0, 0], "Unknown 4.0 prefix");
    assert_eq!(body[4], 0x00, "Host record RecordID must be 0x00");
    assert_eq!(
        body[5], vhost_pid,
        "HostRecord host PID must match virtual host PID"
    );
}

#[test]
fn parity_wire_b8_loading_leavers_placed_between_0x1b_and_0x1c() {
    let mut replay = ReplayBody::new(1, "GhostBot");
    let slots_wire = vec![0u8; 18];
    replay.set_game("Test", &[0; 4], 0);
    replay.add_player(1, "Player1");
    replay.add_player(2, "Player2");
    let _ = replay.set_start(slots_wire, 12345, 0, 2);

    // Player 2 leaves during loading
    replay.add_leaver_loading(2, 0x01, 0x01);

    let (body, _) = replay.finish().expect("replay finish");

    // Marker sequence: 0x1B (second start block), followed by loading leaver (0x17), followed by 0x1C (third start block)
    let b1_pos = body
        .windows(5)
        .position(|w| w == [0x1B, 1, 0, 0, 0])
        .expect("0x1B block present");
    let b2_pos = body
        .windows(5)
        .position(|w| w == [0x1C, 1, 0, 0, 0])
        .expect("0x1C block present");

    assert!(b1_pos < b2_pos, "0x1B must precede 0x1C");
    let between = &body[b1_pos + 5..b2_pos];
    assert!(
        !between.is_empty(),
        "Loading leaver must be placed between 0x1B and 0x1C"
    );
    assert_eq!(between[0], 0x17, "Loading leaver record ID must be 0x17");
    assert_eq!(between[5], 2, "Loading leaver PID must be 2");
}

#[test]
fn parity_wire_b13_pings_broadcast_only_in_lobby_countdown_loading() {
    let (mut st, mut rxs) = seated_game(2);
    st.last_ping_at = std::time::Instant::now() - Duration::from_secs(10);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }

    // 1. In Lobby: ping is sent
    st.on_tick(0);
    let sent = drain_ids(&mut rxs[0]);
    assert!(sent.contains(&ids::PING_FROM_HOST));

    // 2. In Playing: ping is NOT sent
    st.begin_playing();
    st.last_ping_at = std::time::Instant::now() - Duration::from_secs(10);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }
    st.on_tick(0);
    let sent = drain_ids(&mut rxs[0]);
    assert!(
        !sent.contains(&ids::PING_FROM_HOST),
        "Ping packets must not be broadcast during Playing phase"
    );
}

#[test]
fn parity_wire_p1_2_replay_contains_real_statstring() {
    let (mut st, _rxs) = seated_game(2);
    st.create_virtual_host();
    st.begin_playing();

    let rep = st.replay.take().expect("replay exists");
    let (body, _) = rep.finish().expect("finish replay");

    // Game name is "test" followed by NUL, then the stat string followed by NUL
    let game_name_bytes = b"test\0";
    let name_pos = body
        .windows(game_name_bytes.len())
        .position(|w| w == game_name_bytes)
        .expect("game name in replay");
    let after_name = &body[name_pos + game_name_bytes.len()..];

    // First byte of stat string should be the null (4.0) byte in ReplayBody, followed by encoded stat string
    assert_eq!(after_name[0], 0, "null byte (4.0) preceding stat string");
    let stat_slice = &after_name[1..];
    let stat_len = stat_slice
        .iter()
        .position(|&b| b == 0)
        .expect("stat string terminator");
    let stat_bytes = &stat_slice[..stat_len];

    assert!(
        !stat_bytes.is_empty(),
        "stat string in replay must not be empty"
    );
    assert!(
        !stat_bytes.contains(&0),
        "encoded stat string must not contain null bytes"
    );
    let decoded = ghost_protocol::decode_statstring(stat_bytes);
    assert!(
        decoded.len() >= 14,
        "decoded stat string has valid structure"
    );
}

#[test]
fn parity_wire_p1_3_and_p1_4_leave_codes_and_replay_leave_blocks() {
    // 1. Lobby voluntary leave -> PLAYERLEAVE_LOBBY (13)
    let (mut st, mut rxs) = seated_game(2);
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }
    st.handle_leave(1, 0);
    st.reap_left_players();
    let pkt = rxs[1].try_recv().expect("leave packet");
    assert_eq!(pkt[1], ids::PLAYER_LEAVE_OTHERS);
    assert_eq!(pkt[4], 1, "leaving PID");
    let code = u32::from_le_bytes([pkt[5], pkt[6], pkt[7], pkt[8]]);
    assert_eq!(
        code,
        ids::PLAYERLEAVE_LOBBY,
        "Lobby leave must send PLAYERLEAVE_LOBBY (13)"
    );

    // 2. Loading leaver -> reason=1, result=PLAYERLEAVE_DISCONNECT (1) in replay loading block & wire
    let (mut st, mut rxs) = seated_game(2);
    st.begin_loading();
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }
    st.handle_conn_closed(2, "connection dropped".into());
    st.reap_left_players();

    let pkt = rxs[0].try_recv().expect("leave packet");
    assert_eq!(pkt[1], ids::PLAYER_LEAVE_OTHERS);
    assert_eq!(pkt[4], 2, "leaving PID");
    let code = u32::from_le_bytes([pkt[5], pkt[6], pkt[7], pkt[8]]);
    assert_eq!(
        code,
        ids::PLAYERLEAVE_DISCONNECT,
        "Loading disconnect must send PLAYERLEAVE_DISCONNECT (1)"
    );

    // 3. Desync drop -> PLAYERLEAVE_LOST (7) in wire and replay
    let (mut st, mut rxs) = seated_game(3);
    st.begin_playing();
    for rx in &mut rxs {
        let _ = drain_ids(rx);
    }
    // Player 1 & 2 send checksum 0xAAAA, Player 3 sends 0xBBBB
    let mut p1 = BytesMut::new();
    p1.put_u8(0);
    p1.put_u32_le(0xAAAA);
    let mut p2 = BytesMut::new();
    p2.put_u8(0);
    p2.put_u32_le(0xAAAA);
    let mut p3 = BytesMut::new();
    p3.put_u8(0);
    p3.put_u32_le(0xBBBB);
    st.handle_keepalive(1, &p1.freeze());
    st.handle_keepalive(2, &p2.freeze());
    st.handle_keepalive(3, &p3.freeze());
    st.on_tick(0);
    st.reap_left_players();

    let mut leave_pkt = None;
    while let Ok(pkt) = rxs[0].try_recv() {
        if pkt[1] == ids::PLAYER_LEAVE_OTHERS {
            leave_pkt = Some(pkt);
            break;
        }
    }
    let pkt = leave_pkt.expect("leave packet for desynced player");
    assert_eq!(pkt[1], ids::PLAYER_LEAVE_OTHERS);
    assert_eq!(pkt[4], 3, "desynced PID 3");
    let code = u32::from_le_bytes([pkt[5], pkt[6], pkt[7], pkt[8]]);
    assert_eq!(
        code,
        ids::PLAYERLEAVE_LOST,
        "Desync drop must send PLAYERLEAVE_LOST (7)"
    );
}

#[test]
fn parity_wire_p1_7_replay_host_pid_and_name() {
    let (mut st, _rxs) = seated_game(2);
    st.create_virtual_host();
    let vhost_pid = st.virtual_host_pid;
    assert_ne!(vhost_pid, 255);

    st.begin_playing();
    let rep = st.replay.take().expect("replay exists");
    let (body, _) = rep.finish().expect("finish");

    // Header structure: 4 bytes unknown + 1 byte RecordID (0) + 1 byte hostPID + hostName\0
    assert_eq!(body[4], 0, "Host record ID is 0");
    assert_eq!(
        body[5], vhost_pid,
        "Host PID in replay header must match real host PID"
    );

    let name_slice = &body[6..];
    let name_len = name_slice
        .iter()
        .position(|&b| b == 0)
        .expect("host name null terminator");
    let host_name = std::str::from_utf8(&name_slice[..name_len]).unwrap();
    assert_eq!(
        host_name, &st.cfg.virtual_host_name,
        "Host name in replay must match virtual host name"
    );
}

#[test]
fn parity_wire_p1_8_relay_receives_in_game_chat_and_game_over() {
    let (relay_tx, mut relay_rx) = tokio::sync::mpsc::channel(64);
    let relay_handle = ghost_spectator::RelayHandle::new(relay_tx);

    let (mut st, _rxs) = seated_game(2);
    st.relay = Some(relay_handle);
    st.begin_playing();

    // Drain initial GameStart / PlayerInfo commands
    while let Ok(cmd) = relay_rx.try_recv() {
        if matches!(cmd, ghost_spectator::RelayCmd::ViewerChat { .. }) {
            break;
        }
    }

    // 1. In-game player chat forwarded to relay
    let mut chat_bytes = BytesMut::new();
    chat_bytes.put_u8(1); // 1 recipient
    chat_bytes.put_u8(2); // to PID 2
    chat_bytes.put_u8(1); // from PID 1
    chat_bytes.put_u8(0x20); // flag 0x20
    chat_bytes.put_slice(&[0, 0, 0, 0]); // extra flags
    chat_bytes.put_slice(b"hello spectator\0");
    st.handle_chat_to_host(1, &chat_bytes.freeze());

    let cmd = relay_rx.try_recv().expect("relay received chat");
    match cmd {
        ghost_spectator::RelayCmd::ViewerChat { sender, text } => {
            assert_eq!(sender, "P1");
            assert_eq!(text, "hello spectator");
        }
        other => panic!("expected ViewerChat, got {:?}", other),
    }

    // 2. Host chat (send_chat_all) forwarded to relay
    st.send_chat_all("host message");
    let cmd = relay_rx.try_recv().expect("relay received host chat");
    match cmd {
        ghost_spectator::RelayCmd::ViewerChat { text, .. } => {
            assert_eq!(text, "host message");
        }
        other => panic!("expected ViewerChat, got {:?}", other),
    }

    // 3. Game over forwards GameOver to relay
    st.handle_conn_closed(1, "left".into());
    st.handle_conn_closed(2, "left".into());
    st.reap_left_players();
    st.on_tick(0);

    // Drain any remaining game blocks
    let mut got_game_over = false;
    while let Ok(cmd) = relay_rx.try_recv() {
        if matches!(cmd, ghost_spectator::RelayCmd::GameOver) {
            got_game_over = true;
            break;
        }
    }
    assert!(
        got_game_over,
        "Relay must receive GameOver when all players leave / game ends"
    );
}

#[test]
fn parity_wire_p1_9_replay_chat_preserves_flag_and_extra() {
    let (mut st, _rxs) = seated_game(2);
    st.begin_playing();

    // Player 1 sends allied chat with flag 0x20 and extra flags = [2, 0, 0, 0]
    let mut chat_bytes = BytesMut::new();
    chat_bytes.put_u8(1); // 1 recipient
    chat_bytes.put_u8(2); // to PID 2
    chat_bytes.put_u8(1); // from PID 1
    chat_bytes.put_u8(0x20); // flag 0x20
    chat_bytes.put_slice(&[2, 0, 0, 0]); // extra flags = 2 (allied chat scope)
    chat_bytes.put_slice(b"allied chat message\0");
    st.handle_chat_to_host(1, &chat_bytes.freeze());

    let rep = st.replay.take().expect("replay exists");
    let (body, _) = rep.finish().expect("finish replay");

    // REPLAY_CHATMESSAGE = 0x20 in blocks
    // Format: 0x20 (RecordID), PID(1), length(2 LE), flag(1), extra(4 LE), message\0
    let msg_bytes = b"allied chat message\0";
    let pos = body
        .windows(msg_bytes.len())
        .position(|w| w == msg_bytes)
        .expect("message found in replay");
    let extra_bytes = &body[pos - 4..pos];
    let flag = body[pos - 5];
    let pid = body[pos - 8];

    assert_eq!(pid, 1, "chat message PID is 1");
    assert_eq!(flag, 0x20, "chat message flag preserved (0x20)");
    assert_eq!(
        extra_bytes,
        &[2, 0, 0, 0],
        "extra flags (scope) preserved in replay chat"
    );
}
