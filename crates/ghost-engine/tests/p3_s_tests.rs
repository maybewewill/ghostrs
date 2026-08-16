use ghost_engine::stats_dota::StatsDotA;
use ghost_engine::stats_w3mmd::StatsW3MMD;

fn make_dota_action(data: &str, key: &str, value: u32) -> Vec<u8> {
    let mut pkt = Vec::new();
    pkt.extend_from_slice(&[0x6b, b'd', b'r', b'.', b'x', 0x00]);
    pkt.extend_from_slice(data.as_bytes());
    pkt.push(0x00);
    pkt.extend_from_slice(key.as_bytes());
    pkt.push(0x00);
    pkt.extend_from_slice(&value.to_le_bytes());
    pkt
}

fn make_w3mmd_action(mission_key: &str, key: &str, value: u32) -> Vec<u8> {
    let mut pkt = Vec::new();
    pkt.extend_from_slice(b"kMMD.Dat\0");
    pkt.extend_from_slice(mission_key.as_bytes());
    pkt.push(0x00);
    pkt.extend_from_slice(key.as_bytes());
    pkt.push(0x00);
    pkt.extend_from_slice(&value.to_le_bytes());
    pkt
}

#[test]
fn test_s1_dota_stats_courier_tower_rax_and_hero_parsing() {
    let mut dota = StatsDotA::new("DotA 5v5".into());
    dota.add_player(1, "Player1".into());
    dota.add_player(7, "Player2".into());

    // In-game actions:
    // Player 1 destroys Sentinel top level 1 tower (Alliance 0, Level 1, Side 0)
    dota.process_action(&make_dota_action("Data", "Tower010", 1));
    // Player 1 destroys Scourge mid level 2 tower (Alliance 1, Level 2, Side 1)
    dota.process_action(&make_dota_action("Data", "Tower121", 1));
    // Player 7 kills courier
    dota.process_action(&make_dota_action("Data", "Courier1", 7));
    // Player 1 destroys Sentinel bottom melee rax (Alliance 0, Side 2, Type 0)
    dota.process_action(&make_dota_action("Data", "Rax020", 1));

    // End-game hero code for Player 1: "Ekee" (Keeper of the Light)
    let hero_val = u32::from_le_bytes([b'e', b'e', b'k', b'E']);
    dota.process_action(&make_dota_action("1", "9", hero_val));

    // End-game hero code for Player 7: "Obla" (Bloodseeker)
    let hero_val_7 = u32::from_le_bytes([b'a', b'l', b'b', b'O']);
    dota.process_action(&make_dota_action("7", "9", hero_val_7));

    let p1 = dota.players.get(&1).expect("player 1 exists");
    assert_eq!(p1.tower_kills, 2);
    assert_eq!(p1.rax_kills, 1);
    assert_eq!(p1.courier_kills, 0);
    assert_eq!(p1.hero, "Ekee");

    let p7 = dota.players.get(&7).expect("player 7 exists");
    assert_eq!(p7.courier_kills, 1);
    assert_eq!(p7.hero, "Obla");
}

#[test]
fn test_s2_w3mmd_binary_packet_parsing_and_operations() {
    let mut mmd = StatsW3MMD::new("Custom Map".into(), "ladder".into());

    // 1. init version & pid
    mmd.process_action(&make_w3mmd_action("val:0", "init version 1 1", 0));
    mmd.process_action(&make_w3mmd_action("val:1", "init pid 0 Alice", 0));
    mmd.process_action(&make_w3mmd_action("val:2", "init pid 1 Bob", 0));

    assert_eq!(mmd.pid_to_name.get(&0), Some(&"Alice".to_string()));
    assert_eq!(mmd.pid_to_name.get(&1), Some(&"Bob".to_string()));

    // 2. DefVarP definitions: int, real, string
    mmd.process_action(&make_w3mmd_action("val:3", "DefVarP kills int none none", 0));
    mmd.process_action(&make_w3mmd_action("val:4", "DefVarP ratio real none none", 0));
    mmd.process_action(&make_w3mmd_action("val:5", "DefVarP hero string none none", 0));

    // 3. VarP operations (=, +=, -=)
    // Assignment =
    mmd.process_action(&make_w3mmd_action("val:6", "VarP 0 kills = 10", 0));
    assert_eq!(mmd.var_ints.get(&(0, "kills".into())), Some(&10));

    // Addition +=
    mmd.process_action(&make_w3mmd_action("val:7", "VarP 0 kills += 5", 0));
    assert_eq!(mmd.var_ints.get(&(0, "kills".into())), Some(&15));

    // Subtraction -=
    mmd.process_action(&make_w3mmd_action("val:8", "VarP 0 kills -= 3", 0));
    assert_eq!(mmd.var_ints.get(&(0, "kills".into())), Some(&12));

    // Real operations
    mmd.process_action(&make_w3mmd_action("val:9", "VarP 0 ratio = 2.5", 0));
    mmd.process_action(&make_w3mmd_action("val:10", "VarP 0 ratio += 1.5", 0));
    assert!((mmd.var_reals.get(&(0, "ratio".into())).unwrap() - 4.0).abs() < 1e-5);

    // String operations
    mmd.process_action(&make_w3mmd_action("val:11", "VarP 0 hero = Paladin", 0));
    assert_eq!(mmd.var_strings.get(&(0, "hero".into())), Some(&"Paladin".to_string()));

    // 4. Flags
    mmd.process_action(&make_w3mmd_action("val:12", "FlagP 0 winner", 0));
    mmd.process_action(&make_w3mmd_action("val:13", "FlagP 1 leaver", 0));
    assert_eq!(mmd.flags.get(&0), Some(&"winner".to_string()));
    assert_eq!(mmd.flags_leaver.get(&1), Some(&true));

    // 5. Escaping in keys: "Token\ with\ space"
    mmd.process_action(&make_w3mmd_action("val:14", "DefVarP special\\ var int none none", 0));
    mmd.process_action(&make_w3mmd_action("val:15", "VarP 0 special\\ var = 42", 0));
    assert_eq!(mmd.var_ints.get(&(0, "special var".into())), Some(&42));

    // 6. Check ID message
    mmd.process_action(&make_w3mmd_action("chk:0", "check 0", 0));
    assert_eq!(mmd.next_check_id, 1);
}
