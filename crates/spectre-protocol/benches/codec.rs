use bytes::{Bytes, BytesMut};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use spectre_protocol::w3gs::{ActionBlock, W3gsCodec, outgoing::incoming_action};
use tokio_util::codec::Decoder;

fn bench_incoming_action(c: &mut Criterion) {
    let mut group = c.benchmark_group("incoming_action");

    group.bench_function("0_actions", |b| {
        b.iter(|| incoming_action(black_box(&[]), black_box(100)).unwrap())
    });

    let actions_10: Vec<ActionBlock> = (0..10)
        .map(|i| ActionBlock {
            pid: i as u8 + 1,
            data: Bytes::from_static(&[0x10, 0x20, 0x30, 0x40]),
        })
        .collect();

    group.bench_function("10_actions", |b| {
        b.iter(|| incoming_action(black_box(&actions_10), black_box(100)).unwrap())
    });

    let actions_100: Vec<ActionBlock> = (0..100)
        .map(|i| ActionBlock {
            pid: (i % 12) as u8 + 1,
            data: Bytes::from_static(&[0x10, 0x20, 0x30, 0x40]),
        })
        .collect();

    group.bench_function("100_actions", |b| {
        b.iter(|| incoming_action(black_box(&actions_100), black_box(100)).unwrap())
    });

    group.finish();
}

fn bench_w3gs_decode(c: &mut Criterion) {
    let actions = vec![
        ActionBlock {
            pid: 1,
            data: Bytes::from_static(&[0x10, 0x20]),
        },
        ActionBlock {
            pid: 2,
            data: Bytes::from_static(&[0x30, 0x40]),
        },
    ];
    let packet = incoming_action(&actions, 100).unwrap();

    let mut buffer = BytesMut::new();
    for _ in 0..1000 {
        buffer.extend_from_slice(&packet);
    }
    let frozen = buffer.freeze();

    c.bench_function("w3gs_decode_1000_frames", |b| {
        b.iter(|| {
            let mut buf = BytesMut::from(&frozen[..]);
            let mut codec = W3gsCodec::default();
            let mut count = 0;
            while let Ok(Some(_frame)) = codec.decode(black_box(&mut buf)) {
                count += 1;
            }
            assert_eq!(count, 1000);
        })
    });
}

criterion_group!(benches, bench_incoming_action, bench_w3gs_decode);
criterion_main!(benches);
