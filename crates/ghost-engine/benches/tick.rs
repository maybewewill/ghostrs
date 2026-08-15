use std::time::Duration;

use bytes::Bytes;
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use ghost_engine::tick::TickScheduler;
use ghost_engine::{GameConfig, GameState, MapInfo, Player};
use ghost_net::PlayerLink;
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
        },
        virtual_host_name: "|cFF4080C0Ghost".into(),
        reconnect_wait: Duration::from_secs(180),
        custom_slots: None,
        replay_path: std::path::PathBuf::from("replays/bench.w3g"),
        relay: None,
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
