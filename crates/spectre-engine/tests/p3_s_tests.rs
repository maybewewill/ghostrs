use spectre_engine::stats_dota::StatsDotA;

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

#[test]
fn test_s1_dota_stats_courier_tower_rax_and_hero_parsing() {
    let mut dota = StatsDotA::new("DotA 5v5".into());
    dota.add_player(1, "Player1".into());
    dota.add_player(7, "Player2".into());

    dota.process_action(&make_dota_action("Data", "Tower010", 1));

    dota.process_action(&make_dota_action("Data", "Tower121", 1));

    dota.process_action(&make_dota_action("Data", "Courier1", 7));

    dota.process_action(&make_dota_action("Data", "Rax020", 1));

    let hero_val = u32::from_le_bytes([b'e', b'e', b'k', b'E']);
    dota.process_action(&make_dota_action("1", "9", hero_val));

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
