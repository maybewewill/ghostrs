use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{BufMut, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use ghost_protocol::frame::Frame;
use ghost_protocol::w3gs::{W3gsCodec, ids};
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

/// Pulls the host's map size out of a W3GS_MAPCHECK payload, skipping the
/// leading u32 and the NUL-terminated map path. Returns `None` if the payload
/// is truncated rather than guessing a size.
fn map_size_from_mapcheck(payload: &[u8]) -> Option<u32> {
    let after_unknown = payload.get(4..)?;
    let nul = after_unknown.iter().position(|&b| b == 0)?;
    let size = after_unknown.get(nul + 1..nul + 5)?;
    Some(u32::from_le_bytes([size[0], size[1], size[2], size[3]]))
}

fn mapsize_bytes(size: u32) -> Bytes {
    let mut b = BytesMut::new();
    b.put_slice(&[0, 0, 0, 0]); // unknown 4 bytes
    b.put_u8(1); // size_flag = 1 (have map)
    b.put_u32_le(size); // full map size
    Frame::new(ids::MAP_SIZE, b.freeze())
        .encode_with(0xF7)
        .unwrap()
}

fn gameloaded_bytes() -> Bytes {
    Frame::new(ids::GAME_LOADED_SELF, Bytes::new())
        .encode_with(0xF7)
        .unwrap()
}

/// One well-formed, side-effect-free action.
///
/// This must be a *valid* W3GS action, not filler. The host relays action bytes
/// verbatim into the replay body, so whatever goes on the wire here is what a
/// spectating Warcraft III client will execute. An earlier version sent
/// `[0x10; 20]`, and 0x10 is the unit-ability order opcode — twenty of them in a
/// row decodes as a garbage order batch, and the DotA map's script layer then
/// resolves handles for it and crashes the viewer at Game.dll+0x473170.
///
/// 0x16 is ChangeSelection: `[0x16][mode][count u16][unit ids...]`. With
/// `count = 0` there is nothing to select, so the engine parses a complete,
/// legal action and does nothing with it — exactly what a throughput harness
/// wants.
fn action_bytes() -> Bytes {
    const ACTION_CHANGE_SELECTION: u8 = 0x16;
    const SELECT_MODE_ADD: u8 = 0x01;

    let mut action = BytesMut::with_capacity(4);
    action.put_u8(ACTION_CHANGE_SELECTION);
    action.put_u8(SELECT_MODE_ADD);
    action.put_u16_le(0); // no units

    let mut b = BytesMut::new();
    b.put_u32_le(0x1234_5678); // CRC
    b.put_slice(&action);
    Frame::new(ids::OUTGOING_ACTION, b.freeze())
        .encode_with(0xF7)
        .unwrap()
}
fn chat_bytes(from_pid: u8, msg: &str) -> Bytes {
    let mut b = BytesMut::new();
    b.put_u8(1); // 1 recipient
    b.put_u8(255); // to host
    b.put_u8(from_pid);
    b.put_u8(0x20); // in-game chat message flag
    b.put_slice(&[0, 0, 0, 0]); // chat scope extra
    b.put_slice(msg.as_bytes());
    b.put_u8(0); // NUL terminator
    Frame::new(ids::CHAT_TO_HOST, b.freeze())
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
    let mut in_game = false;
    let mut incoming_action_count: u32 = 0;
    let mut last_random_sent = Instant::now();
    // The server-assigned PID arrives in the SLOT_INFO_JOIN packet; the
    // name-derived fallback is wrong because connection order ≠ slot order.
    let mut pid: u8 = 0;
    let mut is_slot0 = false;
    while start.elapsed() < duration {
        tokio::select! {
            _ = action_interval.tick() => {
                if in_game {
                    let _ = framed_write.send(action_bytes()).await;
                }
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
                        // SLOT_INFO_JOIN payload:
                        //   u16 block_len, u8 num_slots, [9B per slot...],
                        //   u32 random_seed, u8 layout, u8 player_slots,
                        //   u8 pid, ...
                        let p = &frame.payload;
                        if p.len() >= 3 {
                            let num_slots = p[2] as usize;
                            let pid_offset = 3 + num_slots * 9 + 6; // past slots + seed + layout + player_slots
                            if p.len() > pid_offset {
                                pid = p[pid_offset];
                                // Check if we're in slot 0 (Blue / Player 1 — the DotA mode picker)
                                // Slot 0's pid field is at offset 3 (first byte of first slot)
                                if num_slots > 0 && p.len() >= 12 {
                                    is_slot0 = p[3] == pid;
                                }
                                tracing::info!(player = %player_name, pid, is_slot0, "seated (pid from server)");
                            }
                        }
                    }
                    ids::MAP_CHECK => {
                        // Report back the size the host just advertised, so the host
                        // treats us as already having the map. Replying with a made-up
                        // size makes the host start a real map download — on the iCCup
                        // DotA map that is a 17 MB stream of MAP_PART packets per client,
                        // which drowns out the action ticks this harness exists to
                        // measure.
                        //
                        // W3GS_MAPCHECK payload (outgoing.rs:229-235):
                        //   u32 unknown, cstring path, u32 size, u32 info, u32 crc, 20B sha1
                        let size = map_size_from_mapcheck(&frame.payload).unwrap_or(1000);
                        let _ = framed_write.send(mapsize_bytes(size)).await;
                    }
                    ids::COUNTDOWN_START => {
                        tracing::debug!(player = %player_name, "countdown started");
                    }
                    ids::COUNTDOWN_END => {
                        tracing::debug!(player = %player_name, "loading started");
                        // Simulate loading time then signal loaded
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        let _ = framed_write.send(gameloaded_bytes()).await;
                        in_game = true;
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

                        incoming_action_count += 1;
                        // Give the heavy DotA init triggers 5 seconds (50 ticks) before typing.
                        // In GHost++, PID 1 is virtual host, PID 2 is Blue (first human player).
                        if incoming_action_count == 50 && pid == 2 {
                            let _ = framed_write.send(chat_bytes(pid, "-ap")).await;
                        }
                        if incoming_action_count == 100 {
                            let _ = framed_write.send(chat_bytes(pid, "-random")).await;
                            last_random_sent = Instant::now();
                        }

                        // Send -random message every 30 seconds from Player 1 (Blue)
                        if incoming_action_count > 100 && pid == 2 && last_random_sent.elapsed() >= Duration::from_secs(30) {
                            let _ = framed_write.send(chat_bytes(pid, "-random")).await;
                            last_random_sent = Instant::now();
                        }
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
    println!(
        "Total ticks received across all clients : {}",
        m.total_actions
    );
    println!(
        "Dropped clients                        : {}",
        m.dropped_clients
    );

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapsize_packet_structure() {
        let b = mapsize_bytes(54321);
        assert_eq!(b[0], 0xF7);
        assert_eq!(b[1], ids::MAP_SIZE);
        assert_eq!(b.len(), 4 + 9); // header(4) + unknown(4) + flag(1) + size(4)
        assert_eq!(b[8], 1); // size_flag
        assert_eq!(u32::from_le_bytes([b[9], b[10], b[11], b[12]]), 54321);
    }

    #[test]
    fn gameloaded_packet_structure() {
        let b = gameloaded_bytes();
        assert_eq!(b[0], 0xF7);
        assert_eq!(b[1], ids::GAME_LOADED_SELF);
        assert_eq!(b.len(), 4); // header(4) with empty payload
    }

    #[test]
    fn keepalive_packet_structure() {
        let b = keepalive_bytes(42);
        assert_eq!(b[0], 0xF7);
        assert_eq!(b[1], ids::OUTGOING_KEEPALIVE);
        assert_eq!(b.len(), 4 + 5);
        assert_eq!(b[4], 0);
        assert_eq!(u32::from_le_bytes([b[5], b[6], b[7], b[8]]), 42);
    }
}
