use std::time::Duration;

use bytes::Bytes;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use spectre_engine::tick::TickScheduler;
use spectre_engine::{GameConfig, GameState, MapInfo, Player};
use spectre_net::PlayerLink;
use tokio::sync::mpsc;

fn seated_state(n: usize) -> GameState {
    let cfg = GameConfig {
        name: "test".into(),
        owner: "slash".into(),
        host_counter: 1,
        num_slots: 12,
        latency: Duration::from_millis(100),
        sync_limit: 50,
        map: MapInfo {
            path: "Maps\\test.w3x".into(),
            size: 1000,
            info: 1,
            crc: 0x1234_5678,
            sha1: [0; 20],
            num_players: 12,
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
        replay_path: std::path::PathBuf::from("replays/bench.w3g"),
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
    };
    let mut st = GameState::new(cfg);
    for i in 1..=n {
        let (tx, _rx) = mpsc::channel(64);
        let p = Player::new(i as u8, format!("P{i}"), i as u64, PlayerLink::for_test(tx));
        st.players.insert(p);
    }
    st
}

fn bench_tick_scheduler(c: &mut Criterion) {
    c.bench_function("tick_scheduler_advance", |b| {
        let mut sched = TickScheduler::new(Duration::from_millis(100));
        let mut now = sched.deadline();
        b.iter(|| {
            now += Duration::from_millis(100);
            black_box(sched.advance(now));
        })
    });
}

fn bench_broadcast_10_players(c: &mut Criterion) {
    let mut st = seated_state(10);
    let packet = Bytes::from_static(&[0xF7, 0x0C, 0x0A, 0x00, 0x64, 0x00, 0x11, 0x22, 0x01, 0x00]);

    c.bench_function("broadcast_10_players", |b| {
        b.iter(|| {
            st.broadcast(black_box(packet.clone()));
        })
    });
}

criterion_group!(benches, bench_tick_scheduler, bench_broadcast_10_players);
criterion_main!(benches);
