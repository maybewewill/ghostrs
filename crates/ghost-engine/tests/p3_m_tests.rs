use ghost_engine::map::*;
use ghost_engine::slots::SlotStatus;
use ghost_engine::state::{GameConfig, GameState, MapInfo};
use ghost_protocol::w3gs::SlotInfo;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn test_m1_map_flags_wire_decomposition() {
    // flags = MAPFLAG_TEAMSTOGETHER | MAPFLAG_FIXEDTEAMS (3)
    let flags_3 = calculate_game_flags(
        MAPSPEED_FAST,
        MAPVIS_DEFAULT,
        MAPOBS_NONE,
        MAPFLAG_TEAMSTOGETHER | MAPFLAG_FIXEDTEAMS,
    );
    // Wire flags: FAST (0x02) | DEFAULT (0x800) | TEAMSTOGETHER (0x4000) | FIXEDTEAMS (0x60000) = 0x00064802
    assert_eq!(flags_3, 0x0000_0002 | 0x0000_0800 | 0x0000_4000 | 0x0006_0000);

    // flags = MAPFLAG_RANDOMRACES (16)
    let flags_races = calculate_game_flags(
        MAPSPEED_FAST,
        MAPVIS_DEFAULT,
        MAPOBS_NONE,
        MAPFLAG_RANDOMRACES,
    );
    assert_eq!(flags_races, 0x0000_0002 | 0x0000_0800 | 0x0400_0000);

    // flags = MAPFLAG_UNITSHARE (4) | MAPFLAG_RANDOMHERO (8)
    let flags_share_hero = calculate_game_flags(
        MAPSPEED_NORMAL,
        MAPVIS_HIDETERRAIN,
        MAPOBS_ONDEFEAT,
        MAPFLAG_UNITSHARE | MAPFLAG_RANDOMHERO,
    );
    assert_eq!(
        flags_share_hero,
        0x0000_0001 | 0x0000_0100 | 0x0000_2000 | 0x0100_0000 | 0x0200_0000
    );
}

#[test]
fn test_m2_m3_game_type_four_independent_filters() {
    // Default GHost++ values:
    // maker: MAPFILTER_MAKER_USER (1) -> MAPGAMETYPE_MAKERUSER (1<<13)
    // type: MAPFILTER_TYPE_SCENARIO (2) -> MAPGAMETYPE_TYPESCENARIO (1<<16)
    // size: MAPFILTER_SIZE_LARGE (4) -> MAPGAMETYPE_SIZELARGE (1<<19)
    // obs: MAPFILTER_OBS_NONE (4) -> MAPGAMETYPE_OBSNONE (1<<22)
    let default_gt = calculate_game_type(
        MAPFILTER_MAKER_USER,
        MAPFILTER_TYPE_SCENARIO,
        MAPFILTER_SIZE_LARGE,
        MAPFILTER_OBS_NONE,
    );
    assert_eq!(
        default_gt,
        MAPGAMETYPE_MAKERUSER
            | MAPGAMETYPE_TYPESCENARIO
            | MAPGAMETYPE_SIZELARGE
            | MAPGAMETYPE_OBSNONE
    );

    // Melee game type
    let melee_gt = calculate_game_type(
        MAPFILTER_MAKER_BLIZZARD,
        MAPFILTER_TYPE_MELEE,
        MAPFILTER_SIZE_SMALL,
        MAPFILTER_OBS_FULL,
    );
    assert_eq!(
        melee_gt,
        MAPGAMETYPE_MAKERBLIZZARD
            | MAPGAMETYPE_TYPEMELEE
            | MAPGAMETYPE_SIZESMALL
            | MAPGAMETYPE_OBSFULL
    );

    // Multiple bits in filter_size (SMALL | MEDIUM) and filter_obs (FULL | ONDEATH)
    let multi_gt = calculate_game_type(
        MAPFILTER_MAKER_USER,
        MAPFILTER_TYPE_SCENARIO,
        MAPFILTER_SIZE_SMALL | MAPFILTER_SIZE_MEDIUM,
        MAPFILTER_OBS_FULL | MAPFILTER_OBS_ONDEATH,
    );
    assert_eq!(
        multi_gt,
        MAPGAMETYPE_MAKERUSER
            | MAPGAMETYPE_TYPESCENARIO
            | MAPGAMETYPE_SIZESMALL
            | MAPGAMETYPE_SIZEMEDIUM
            | MAPGAMETYPE_OBSFULL
            | MAPGAMETYPE_OBSONDEATH
    );
}

#[test]
fn test_m4_melee_slots_initialization() {
    let mut slots = vec![
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8,
            computer: 0,
            team: 0,
            colour: 0,
            race: 0x01, // Human
            computer_type: 1,
            handicap: 100,
        },
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8,
            computer: 0,
            team: 0,
            colour: 1,
            race: 0x02, // Orc
            computer_type: 1,
            handicap: 100,
        },
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8,
            computer: 0,
            team: 0,
            colour: 2,
            race: 0x04, // Undead
            computer_type: 1,
            handicap: 100,
        },
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8,
            computer: 0,
            team: 0,
            colour: 3,
            race: 0x08, // NightElf
            computer_type: 1,
            handicap: 100,
        },
    ];

    apply_melee_slot_init(&mut slots);

    for (i, slot) in slots.iter().enumerate() {
        assert_eq!(slot.team, i as u8, "slot {} should have team {}", i, i);
        assert_eq!(slot.race & 0x20, 0x20, "slot {} race should be RANDOM", i);
    }
}

#[test]
fn test_m5_forced_random_races() {
    let mut slots = vec![
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8,
            computer: 0,
            team: 0,
            colour: 0,
            race: 0x01, // Human
            computer_type: 1,
            handicap: 100,
        },
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8,
            computer: 0,
            team: 1,
            colour: 1,
            race: 0x02, // Orc
            computer_type: 1,
            handicap: 100,
        },
    ];

    apply_random_races_force(&mut slots, MAPFLAG_RANDOMRACES);
    assert_eq!(slots[0].race, 0x20);
    assert_eq!(slots[1].race, 0x20);
}

#[test]
fn test_m6_observer_slots_and_editor_version() {
    let slots = (0..10)
        .map(|i| SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8,
            computer: 0,
            team: (i / 5) as u8,
            colour: i as u8,
            race: 0x20,
            computer_type: 1,
            handicap: 100,
        })
        .collect::<Vec<_>>();

    // Modern editor version >= 6060 -> default max slots = 24
    let mut slots_24 = slots.clone();
    add_observer_slots(&mut slots_24, MAPOBS_ALLOWED, 6060, None);
    assert_eq!(slots_24.len(), 24);
    for s in &slots_24[10..24] {
        assert_eq!(s.team, 24);
        assert_eq!(s.colour, 24);
        assert_eq!(s.race, 0x20);
    }

    // Legacy editor version < 6060 -> default max slots = 12
    let mut slots_12 = slots.clone();
    add_observer_slots(&mut slots_12, MAPOBS_ALLOWED, 5000, None);
    assert_eq!(slots_12.len(), 12);
    for s in &slots_12[10..12] {
        assert_eq!(s.team, 24);
        assert_eq!(s.colour, 24);
        assert_eq!(s.race, 0x20);
    }

    // Overridden max slots = 16
    let mut slots_custom = slots.clone();
    add_observer_slots(&mut slots_custom, MAPOBS_ALLOWED, 6060, Some(16));
    assert_eq!(slots_custom.len(), 16);
}

#[test]
fn test_m7_closed_slots_and_num_players() {
    // In GHost++, closed slots are not in m_Slots and MapNumPlayers = RawMapNumPlayers - ClosedSlots
    let raw_slots = vec![
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Open as u8, // Open
            computer: 0,
            team: 0,
            colour: 0,
            race: 0x20,
            computer_type: 1,
            handicap: 100,
        },
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Closed as u8, // Closed (should be excluded)
            computer: 0,
            team: 0,
            colour: 1,
            race: 0x20,
            computer_type: 1,
            handicap: 100,
        },
        SlotInfo {
            pid: 0,
            download_status: 255,
            slot_status: SlotStatus::Occupied as u8, // Computer occupied
            computer: 1,
            team: 1,
            colour: 2,
            race: 0x20,
            computer_type: 1,
            handicap: 100,
        },
    ];

    let active_slots: Vec<SlotInfo> = raw_slots
        .into_iter()
        .filter(|s| s.slot_status != SlotStatus::Closed as u8)
        .collect();

    assert_eq!(active_slots.len(), 2);
    assert_eq!(active_slots[0].colour, 0);
    assert_eq!(active_slots[1].colour, 2);
}

#[test]
fn test_m8_map_fields_and_stats_and_hcl_dispatch() {
    let mut map = MapInfo::test_default();
    map.default_hcl = "apem".into();
    map.map_type = "dota".into();
    map.default_player_score = 1500;
    map.matchmaking_category = "dota_ladder".into();
    map.stats_w3mmd_category = "dota_stats".into();

    let cfg = GameConfig {
        name: "Casual Game".into(), // no HCL in name
        owner: "slash".into(),
        host_counter: 1,
        num_slots: 10,
        latency: Duration::from_millis(50),
        sync_limit: 50,
        map: map.clone(),
        virtual_host_name: "|cFF4080C0Ghost".into(),
        reconnect_wait: Duration::from_secs(180),
        custom_slots: None,
        replay_path: PathBuf::from("replays/test.w3g"),
        relay: None,
        max_downloaders: 3,
        max_download_speed: 100_000,
        allow_downloads: 1,
        autokick_ping: 300,
        lc_pings: false,
        spoof_checks: 1,
        require_spoof_checks: false,
        host_port: 6112,
        gproxy_reconnect_port: 0,
        store: None,
        stat_string: Vec::new(),
        event_tx: None,
        lobby_time_limit: 0,
        load_in_game: false,
        auto_save: false,
        creator_name: "slash".into(),
        creator_server: "iCCup".into(),
        min_score: 0.0,
        max_score: 0.0,
        matchmaking: false,
    };

    let st = GameState::new(cfg);
    assert_eq!(st.hcl, Some("apem".to_string()));
    assert!(st.dota.is_some());
    assert!(st.w3mmd.is_none());

    // Switch map_type to w3mmd
    let mut map_w3mmd = MapInfo::test_default();
    map_w3mmd.map_type = "w3mmd".into();
    let mut cfg_w3mmd = st.cfg.clone();
    cfg_w3mmd.map = map_w3mmd;
    let st_w3mmd = GameState::new(cfg_w3mmd);
    assert!(st_w3mmd.dota.is_none());
    assert!(st_w3mmd.w3mmd.is_some());
}

#[test]
fn test_m9_check_valid() {
    let mut map = MapInfo::test_default();
    assert!(map.check_valid().is_ok());

    // Path empty
    map.path = "".into();
    assert!(map.check_valid().is_err());

    // Path > 53 chars
    map.path = "Maps\\Download\\ThisIsAnExtremelyLongMapPathThatExceedsFiftyThreeChars.w3x".into();
    assert!(map.check_valid().is_err());

    map.path = "Maps\\Download\\test.w3x".into();

    // If validation checks dimensions
    let mut bad_dim = map.clone();
    bad_dim.width = 0;
    assert!(bad_dim.check_valid().is_err());

    // Size mismatch with data
    let mut bad_size = map.clone();
    bad_size.data = Some(Arc::new(vec![0u8; 500]));
    bad_size.size = 1000;
    assert!(bad_size.check_valid().is_err());
}
