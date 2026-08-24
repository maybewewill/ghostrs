use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::{BufMut, Bytes, BytesMut};
use futures_util::{SinkExt, StreamExt};
use ghost_protocol::frame::Frame;
use ghost_protocol::w3gs::{W3gsCodec, ids};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
    let mut game_start_time: Option<Instant> = None;
    let mut ap_sent = false;
    let mut initial_random_sent = false;
    let mut last_random_sent = Instant::now();
    let mut pid: u8 = 0;
    let mut is_blue = false;
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
                                // Check if we are seated in the Blue slot (color == 1 or first sentinel slot)
                                for s in 0..num_slots {
                                    let soff = 3 + s * 9;
                                    if p.len() >= soff + 9 && p[soff] == pid && p[soff + 5] == 1 {
                                        is_blue = true;
                                    }
                                }
                                if pid == 2 { is_blue = true; }
                                tracing::info!(player = %player_name, pid, is_blue, "seated (pid from server)");
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
                        let _ = framed_write.send(gameloaded_bytes()).await;
                        in_game = true;
                        game_start_time = Some(Instant::now());
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
                        if let Some(gst) = game_start_time {
                            let elapsed = gst.elapsed();
                            let is_mode_picker = is_blue || player_name.ends_with("_P0") || pid == 2;
                            // Send -ap after 10 seconds minimum from Blue player
                            if elapsed >= Duration::from_secs(10) && !ap_sent && is_mode_picker {
                                let _ = framed_write.send(chat_bytes(pid, "-ap")).await;
                                ap_sent = true;
                            }
                            // Send initial -random after 15 seconds (after -ap mode active)
                            if elapsed >= Duration::from_secs(15) && !initial_random_sent {
                                let _ = framed_write.send(chat_bytes(pid, "-random")).await;
                                initial_random_sent = true;
                                last_random_sent = Instant::now();
                            }
                            // Send -random every 30 seconds thereafter from Blue player
                            if initial_random_sent && is_mode_picker && last_random_sent.elapsed() >= Duration::from_secs(30) {
                                let _ = framed_write.send(chat_bytes(pid, "-random")).await;
                                last_random_sent = Instant::now();
                            }
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

// ---------------------------------------------------------------------------
// DotaTV viewer: bootstrap fetch + live chunk stream, the exact wire protocol
// the injected WC3 client speaks (DTV1 greeting, mode byte, chunk frames).
// ---------------------------------------------------------------------------

struct TvMetrics {
    viewers: u64,
    failed_viewers: u64,
    frames: u64,
    stream_bytes: u64,
    gaps_ms: Vec<f64>,
    bootstrap_ms: Vec<f64>,
}

async fn read_n(sock: &mut TcpStream, n: usize) -> std::io::Result<Vec<u8>> {
    let mut out = vec![0u8; n];
    sock.read_exact(&mut out).await?;
    Ok(out)
}

async fn run_viewer(tv_addr: String, delay: Duration, duration: Duration, m: Arc<Mutex<TvMetrics>>) {
    tokio::time::sleep(delay).await;

    // 1. Bootstrap: mode 0 + start index 0 -> u32 resume index, u32 file len, file.
    let t0 = Instant::now();
    let mut sock = match TcpStream::connect(&tv_addr).await {
        Ok(s) => s,
        Err(_) => {
            m.lock().await.failed_viewers += 1;
            return;
        }
    };
    if read_n(&mut sock, 4).await.unwrap_or_default() != b"DTV1" {
        m.lock().await.failed_viewers += 1;
        return;
    }
    let _ = sock.write_all(&[0u8, 0, 0, 0, 0]).await; // MODE_BOOTSTRAP, index 0
    let Ok(idx_bytes) = read_n(&mut sock, 4).await else {
        m.lock().await.failed_viewers += 1;
        return;
    };
    let start_index = u32::from_le_bytes(idx_bytes.try_into().unwrap());
    let Ok(len_bytes) = read_n(&mut sock, 4).await else {
        m.lock().await.failed_viewers += 1;
        return;
    };
    let file_len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
    let Ok(file) = read_n(&mut sock, file_len).await else {
        m.lock().await.failed_viewers += 1;
        return;
    };
    let boot_ms = t0.elapsed().as_secs_f64() * 1000.0;
    drop(sock);

    // Validate every block inflates to exactly 8192 (what the WC3 loader requires).
    let n_blocks = u32::from_le_bytes(file[44..48].try_into().unwrap()) as usize;
    let mut off = 68usize;
    for _ in 0..n_blocks {
        if off + 8 > file.len() {
            m.lock().await.failed_viewers += 1;
            return;
        }
        let comp_len = u16::from_le_bytes(file[off..off + 2].try_into().unwrap()) as usize;
        off += 8 + comp_len;
    }
    if off != file.len() {
        m.lock().await.failed_viewers += 1;
        return;
    }

    // 2. Live stream from the resume index.
    let mut sock = match TcpStream::connect(&tv_addr).await {
        Ok(s) => s,
        Err(_) => {
            m.lock().await.failed_viewers += 1;
            return;
        }
    };
    if read_n(&mut sock, 4).await.unwrap_or_default() != b"DTV1" {
        m.lock().await.failed_viewers += 1;
        return;
    }
    let mut req = vec![1u8]; // MODE_STREAM
    req.extend_from_slice(&start_index.to_le_bytes());
    let _ = sock.write_all(&req).await;

    let deadline = Instant::now() + duration;
    let mut last_frame = Instant::now();
    let mut local_gaps = Vec::new();
    let mut frames = 0u64;
    let mut bytes = 0u64;
    loop {
        let gap = tokio::time::timeout(Duration::from_secs(10), async {
            // Wire frame: u16 compressedSize, u16 validBytes, u32 crc32, u8 data[] (LE).
            // The header is 8 bytes, not 4 — the u32 CRC must be consumed or every
            // following frame desyncs and the metrics measure garbage.
            let hdr = read_n(&mut sock, 8).await.ok()?;
            let comp_len = u16::from_le_bytes(hdr[..2].try_into().unwrap()) as usize;
            let _valid_bytes = u16::from_le_bytes(hdr[2..4].try_into().unwrap());
            let _crc = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
            let payload = read_n(&mut sock, comp_len).await.ok()?;
            Some(payload)
        }).await;
        match gap {
            Ok(Some(payload)) => {
                let now = Instant::now();
                local_gaps.push(now.duration_since(last_frame).as_secs_f64() * 1000.0);
                last_frame = now;
                frames += 1;
                bytes += (8 + payload.len()) as u64;
                if now >= deadline { break; }
            }
            Ok(None) | Err(_) => break, // timeout or clean close
        }
    }

    let mut m = m.lock().await;
    m.viewers += 1;
    m.frames += frames;
    m.stream_bytes += bytes;
    m.gaps_ms.extend(local_gaps);
    m.bootstrap_ms.push(boot_ms);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let mut num_games = 1usize;
    let mut players_per_game = 10usize;
    let mut duration_secs = 10u64;
    let mut addr = "127.0.0.1:6112".to_string();
    let mut tv_addr = "127.0.0.1:6116".to_string();
    let mut viewers = 0usize;
    let mut viewer_delay_secs = 0u64;

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
            "--tv-addr" => {
                tv_addr = args[i + 1].clone();
                i += 2;
            }
            "--viewers" => {
                viewers = args[i + 1].parse()?;
                i += 2;
            }
            "--viewer-delay" => {
                viewer_delay_secs = args[i + 1].parse()?;
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

    let tv_metrics = Arc::new(Mutex::new(TvMetrics {
        viewers: 0,
        failed_viewers: 0,
        frames: 0,
        stream_bytes: 0,
        gaps_ms: Vec::new(),
        bootstrap_ms: Vec::new(),
    }));
    for v in 0..viewers {
        let a = tv_addr.clone();
        let m = tv_metrics.clone();
        let delay = Duration::from_secs(viewer_delay_secs + v as u64);
        let d = Duration::from_secs(duration_secs);
        tasks.push(tokio::spawn(async move {
            run_viewer(a, delay, d, m).await;
        }));
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
    if viewers > 0 {
        let mut tv = tv_metrics.lock().await;
        println!("============================================================");
        println!("                   DOTATV VIEWER REPORT                     ");
        println!("============================================================");
        println!("Viewers attached : {}", tv.viewers);
        println!("Failed viewers   : {}", tv.failed_viewers);
        println!("Total frames     : {}", tv.frames);
        println!("Stream bytes     : {}", tv.stream_bytes);
        if !tv.bootstrap_ms.is_empty() {
            let avg = tv.bootstrap_ms.iter().sum::<f64>() / tv.bootstrap_ms.len() as f64;
            println!("Bootstrap ms avg : {:.1}", avg);
        }
        if !tv.gaps_ms.is_empty() {
            tv.gaps_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let n = tv.gaps_ms.len();
            println!(
                "Frame gap p50 / p95 / max : {:.1} / {:.1} / {:.1} ms",
                tv.gaps_ms[n * 50 / 100],
                tv.gaps_ms[n * 95 / 100],
                tv.gaps_ms[n - 1]
            );
        }
        println!("============================================================");
    }
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
