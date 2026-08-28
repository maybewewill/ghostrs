use std::path::Path;

use spectre_protocol::dotatv::{
    GameStartSnapshot, decode_action, decode_hello, decode_history_end, decode_player,
    decode_snapshot, encode_action, encode_hello, encode_history_end, encode_player,
    encode_snapshot,
};
use spectre_protocol::w3gs::SlotInfo;

#[test]
fn generate_and_verify_golden_fixtures() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fixtures_dir = Path::new(manifest_dir).join("tests/fixtures/dotatv");
    std::fs::create_dir_all(&fixtures_dir).expect("failed to create fixtures directory");

    // 1. hello.bin
    let hello_bytes = encode_hello(1, "spectre").expect("encode hello");
    let expected_hello: &[u8] = &[
        0xFD, 0x01, 0x0E, 0x00, 0x01, 0x00, 0x73, 0x70, 0x65, 0x63, 0x74, 0x72, 0x65, 0x00,
    ];
    assert_eq!(&hello_bytes[..], expected_hello);
    let hello_path = fixtures_dir.join("hello.bin");
    std::fs::write(&hello_path, &hello_bytes).expect("write hello.bin");
    let read_hello = std::fs::read(&hello_path).expect("read hello.bin");
    assert_eq!(read_hello, expected_hello);
    let (ver, srv) = decode_hello(&read_hello[4..]).expect("decode hello");
    assert_eq!(ver, 1);
    assert_eq!(srv, "spectre");

    // 2. snapshot.bin
    let snap = GameStartSnapshot {
        game_name: "DotA Live".to_string(),
        map_path: "Maps\\Download\\DotA v6.83d.w3x".to_string(),
        map_size: 8_388_608,
        map_info_crc: 0x1122_3344,
        map_crc: 0x5566_7788,
        map_sha1: [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20,
        ],
        stat_string: vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
        random_seed: 1337,
        layout_style: 0,
        player_slots: 10,
        war3_version: 26,
        is_tft: true,
        slots: vec![
            SlotInfo {
                pid: 0,
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
                pid: 1,
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
    };
    let snapshot_bytes = encode_snapshot(&snap).expect("encode snapshot");
    let expected_snapshot: &[u8] = &[
        0xFD, 0x02, 112, 0x00, // Header: FD 02, total length 112 (4 header + 108 payload)
        0x44, 0x6F, 0x74, 0x41, 0x20, 0x4C, 0x69, 0x76, 0x65, 0x00, // "DotA Live\0" (10)
        0x4D, 0x61, 0x70, 0x73, 0x5C, 0x44, 0x6F, 0x77, 0x6E, 0x6C, 0x6F, 0x61, 0x64, 0x5C, 0x44,
        0x6F, 0x74, 0x41, 0x20, 0x76, 0x36, 0x2E, 0x38, 0x33, 0x64, 0x2E, 0x77, 0x33, 0x78,
        0x00, // "Maps\Download\DotA v6.83d.w3x\0" (31)
        0x00, 0x00, 0x80, 0x00, // map_size = 8388608 (4)
        0x44, 0x33, 0x22, 0x11, // map_info_crc (4)
        0x88, 0x77, 0x66, 0x55, // map_crc (4)
        1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19,
        20, // map_sha1 (20)
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x00, // stat_string\0 (9)
        0x39, 0x05, 0x00, 0x00, // random_seed = 1337 (4)
        0x00, // layout_style (1)
        0x0A, // player_slots (1)
        0x1A, // war3_version = 26 (1)
        0x01, // is_tft = 1 (1)
        0x02, // num_slots = 2 (1)
        0x00, 0x64, 0x02, 0x00, 0x00, 0x01, 0x01, 0x00, 0x64, // slot 0 (9)
        0x01, 0x64, 0x02, 0x00, 0x01, 0x02, 0x02, 0x00, 0x64, // slot 1 (9)
    ];
    assert_eq!(&snapshot_bytes[..], expected_snapshot);
    let snapshot_path = fixtures_dir.join("snapshot.bin");
    std::fs::write(&snapshot_path, &snapshot_bytes).expect("write snapshot.bin");
    let read_snapshot = std::fs::read(&snapshot_path).expect("read snapshot.bin");
    assert_eq!(read_snapshot, expected_snapshot);
    let decoded_snap = decode_snapshot(&read_snapshot[4..]).expect("decode snapshot");
    assert_eq!(decoded_snap, snap);

    // 3. player.bin
    let player_bytes = encode_player(1, "Player1", 1, 0, 1).expect("encode player");
    let expected_player: &[u8] = &[
        0xFD, 0x03, 0x10, 0x00, 0x01, 0x50, 0x6C, 0x61, 0x79, 0x65, 0x72, 0x31, 0x00, 0x01, 0x00,
        0x01,
    ];
    assert_eq!(&player_bytes[..], expected_player);
    let player_path = fixtures_dir.join("player.bin");
    std::fs::write(&player_path, &player_bytes).expect("write player.bin");
    let read_player = std::fs::read(&player_path).expect("read player.bin");
    assert_eq!(read_player, expected_player);
    let decoded_player = decode_player(&read_player[4..]).expect("decode player");
    assert_eq!(decoded_player.pid, 1);
    assert_eq!(decoded_player.name, "Player1");
    assert_eq!(decoded_player.colour, 1);
    assert_eq!(decoded_player.team, 0);
    assert_eq!(decoded_player.race, 1);

    // 4. action.bin
    let raw_action = [0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00];
    let action_bytes = encode_action(&raw_action).expect("encode action");
    let expected_action: &[u8] = &[0xFD, 0x04, 0x0A, 0x00, 0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00];
    assert_eq!(&action_bytes[..], expected_action);
    let action_path = fixtures_dir.join("action.bin");
    std::fs::write(&action_path, &action_bytes).expect("write action.bin");
    let read_action = std::fs::read(&action_path).expect("read action.bin");
    assert_eq!(read_action, expected_action);
    let decoded_act = decode_action(&read_action[4..]).expect("decode action");
    assert_eq!(&decoded_act[..], &raw_action);

    // 5. history_end.bin
    let history_end_bytes = encode_history_end(1000).expect("encode history_end");
    let expected_history_end: &[u8] = &[0xFD, 0x07, 0x08, 0x00, 0xE8, 0x03, 0x00, 0x00];
    assert_eq!(&history_end_bytes[..], expected_history_end);
    let history_end_path = fixtures_dir.join("history_end.bin");
    std::fs::write(&history_end_path, &history_end_bytes).expect("write history_end.bin");
    let read_history_end = std::fs::read(&history_end_path).expect("read history_end.bin");
    assert_eq!(read_history_end, expected_history_end);
    let decoded_count = decode_history_end(&read_history_end[4..]).expect("decode history_end");
    assert_eq!(decoded_count, 1000);
}
