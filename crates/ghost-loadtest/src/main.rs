use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{BufMut, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use ghost_protocol::frame::Frame;
use ghost_protocol::w3gs::{ids, W3gsCodec};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

struct Metrics {
    intervals_ms: Vec<f64>,
    total_actions: u64,
    dropped_clients: u64,
}

fn reqjoin_bytes(name: &str) -> Bytes {
    let mut b = BytesMut::new();
    b.put_u32_le(1); // host counter
    b.put_u32_le(0); // entry key
    b.put_u8(0);
    b.put_u16_le(6112);
    b.put_u32_le(0);
    b.put_slice(name.as_bytes());
    b.put_u8(0);
    b.put_slice(&[0, 0, 0, 0, 0, 0]); // 6 bytes unknown/sockaddr prefix
    b.put_slice(&[127, 0, 0, 1]);
    b.freeze()
}

fn keepalive_bytes(checksum: u32) -> Bytes {
    let mut b = BytesMut::new();
    b.put_u8(0);
    b.put_u32_le(checksum);
    Frame::new(ids::OUTGOING_KEEPALIVE, b.freeze())
        .encode_with(0xF7)
        .unwrap()
}

fn action_bytes() -> Bytes {
    let mut b = BytesMut::new();
    b.put_u32_le(0x1234_5678); // CRC
    b.put_slice(&[0x10; 20]);   // 20 bytes action payload
    Frame::new(ids::OUTGOING_ACTION, b.freeze())
        .encode_with(0xF7)
        .unwrap()
}

async fn run_client(
    addr: String,
    player_name: String,
    duration: Duration,
    metrics: Arc<Mutex<Metrics>>,
) {
    let stream = match TcpStream::connect(&addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(player = %player_name, error = %e, "connect failed");
            metrics.lock().await.dropped_clients += 1;
            return;
        }
    };

    let (read_half, write_half) = stream.into_split();
    let mut framed_read = tokio_util::codec::FramedRead::new(read_half, W3gsCodec::default());
    let mut framed_write = tokio_util::codec::FramedWrite::new(write_half, W3gsCodec::default());

    // Send REQ_JOIN
    let req = Frame::new(ids::REQ_JOIN, reqjoin_bytes(&player_name))
        .encode_with(0xF7)
        .unwrap();
    if framed_write.send(req).await.is_err() {
        metrics.lock().await.dropped_clients += 1;
        return;
    }

    let start = Instant::now();
    let mut last_action_tick: Option<Instant> = None;
    let mut checksum: u32 = 0;
    let mut local_intervals = Vec::new();
    let mut action_interval = tokio::time::interval(Duration::from_secs(1));

    while start.elapsed() < duration {
        tokio::select! {
            _ = action_interval.tick() => {
                let _ = framed_write.send(action_bytes()).await;
            }

            frame = framed_read.next() => {
                let frame = match frame {
                    Some(Ok(f)) => f,
                    _ => {
                        metrics.lock().await.dropped_clients += 1;
                        break;
                    }
                };

                match frame.id {
                    ids::SLOT_INFO_JOIN => {
                        // Confirmed seated
                    }
                    ids::INCOMING_ACTION => {
                        let now = Instant::now();
                        if let Some(prev) = last_action_tick {
                            let interval_ms = now.duration_since(prev).as_secs_f64() * 1000.0;
                            local_intervals.push(interval_ms);
                        }
                        last_action_tick = Some(now);

                        checksum = checksum.wrapping_add(1);
                        let _ = framed_write.send(keepalive_bytes(checksum)).await;
                    }
                    ids::PING_FROM_HOST => {
                        let pong = Frame::new(ids::PONG_TO_HOST, frame.payload)
                            .encode_with(0xF7)
                            .unwrap();
                        let _ = framed_write.send(pong).await;
                    }
                    _ => {}
                }
            }
        }
    }

    let mut m = metrics.lock().await;
    m.total_actions += local_intervals.len() as u64;
    m.intervals_ms.extend(local_intervals);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let mut num_games = 1usize;
    let mut players_per_game = 10usize;
    let mut duration_secs = 10u64;
    let mut addr = "127.0.0.1:6112".to_string();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--games" => {
                num_games = args[i + 1].parse()?;
                i += 2;
            }
            "--players-per-game" => {
                players_per_game = args[i + 1].parse()?;
                i += 2;
            }
            "--duration" => {
                duration_secs = args[i + 1].parse()?;
                i += 2;
            }
            "--addr" => {
                addr = args[i + 1].clone();
                i += 2;
            }
            _ => i += 1,
        }
    }

    let total_players = num_games * players_per_game;
    println!(
        "Starting load test: {num_games} game(s) x {players_per_game} players = {total_players} clients for {duration_secs}s against {addr}"
    );

    let metrics = Arc::new(Mutex::new(Metrics {
        intervals_ms: Vec::new(),
        total_actions: 0,
        dropped_clients: 0,
    }));

    let mut tasks = Vec::new();
    for g in 0..num_games {
        for p in 0..players_per_game {
            let name = format!("Bot_G{g}_P{p}");
            let a = addr.clone();
            let m = metrics.clone();
            let d = Duration::from_secs(duration_secs);
            tasks.push(tokio::spawn(async move {
                run_client(a, name, d, m).await;
            }));
        }
    }

    for task in tasks {
        let _ = task.await;
    }

    let mut m = metrics.lock().await;
    m.intervals_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());

    println!("============================================================");
    println!("                   LOAD TEST REPORT                         ");
    println!("============================================================");
    println!("Total ticks received across all clients : {}", m.total_actions);
    println!("Dropped clients                        : {}", m.dropped_clients);

    if !m.intervals_ms.is_empty() {
        let n = m.intervals_ms.len();
        let min = m.intervals_ms[0];
        let p50 = m.intervals_ms[n * 50 / 100];
        let p95 = m.intervals_ms[n * 95 / 100];
        let p99 = m.intervals_ms[n * 99 / 100];
        let max = m.intervals_ms[n - 1];

        println!("Tick interval Min : {:.2} ms", min);
        println!("Tick interval p50 : {:.2} ms", p50);
        println!("Tick interval p95 : {:.2} ms", p95);
        println!("Tick interval p99 : {:.2} ms", p99);
        println!("Tick interval Max : {:.2} ms", max);
    }
    println!("============================================================");

    Ok(())
}
