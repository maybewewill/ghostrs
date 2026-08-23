//! Generates a full-history DotA .w3g whose body is the prologue plus N
//! clocked but ACTION-FREE timeslots (record 0x1F, 100 ms increment, zero
//! command bytes). Loading it exercises the seek-to-live path: the injected
//! client should drain the whole body behind the loading screen and reveal the
//! 3D world already at match-time N*100 ms, with no visible fast-forward.
//!
//! Action-free is deliberate: real action data crashes the 1.26a parser behind
//! the loading screen (that is the separate, harder problem). This isolates the
//! SEEK mechanism and the clock advance from the action-parse crash.
//!
//! Usage: gen_seek_test [out.w3g] [ticks]   (default 9000 ticks = 15 min)

use std::io::Write;
use std::path::PathBuf;

use ghost_spectator::{ReplayBody, W3gWriter};

const TICK_MS: u16 = 100;

fn slot(pid: u8, status: u8, team: u8, color: u8, race: u8) -> Vec<u8> {
    vec![pid, 100, status, 0, team, color, race, 1, 100]
}

fn encode_statstring(raw: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(raw.len() + raw.len() / 7 + 1);
    let mut mask = 1u8;
    for i in 0..raw.len() {
        let byte = raw[i];
        if byte % 2 == 0 {
            result.push(byte.wrapping_add(1));
        } else {
            result.push(byte);
            mask |= 1 << ((i % 7) + 1);
        }
        if i % 7 == 6 || i == raw.len() - 1 {
            let insert_pos = result.len() - 1 - (i % 7);
            result.insert(insert_pos, mask);
            mask = 1;
        }
    }
    result
}

fn main() {
    let out_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("seek_test.w3g"));
    let ticks: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(9000);

    let mut b = ReplayBody::new(1, "iCCup");

    let mut raw = Vec::new();
    raw.extend_from_slice(&0x01u32.to_le_bytes()); // map flags
    raw.push(0);
    raw.extend_from_slice(&(1920u16).to_le_bytes());
    raw.extend_from_slice(&(1080u16).to_le_bytes());
    raw.extend_from_slice(&0x1234_5678u32.to_le_bytes());
    raw.extend_from_slice(b"Maps\\Download\\iCCup DotA 507.w3x");
    raw.push(0);
    raw.extend_from_slice(b"iCCup");
    raw.push(0);
    let stat = encode_statstring(&raw);
    b.set_game("iCCup DotA 507 [AP]", &stat, 0x01);

    for pid in 2..=11u8 {
        b.add_player(pid, &format!("Bot_{pid}"));
    }
    let mut slots: Vec<u8> = Vec::new();
    for i in 0..10u8 {
        let pid = i + 1;
        let team = if i < 5 { 0 } else { 1 };
        slots.extend_from_slice(&slot(pid, 2, team, i, 1));
    }
    slots.extend_from_slice(&slot(12, 0, 2, 10, 1));
    b.set_start(slots, 0xDEAD_BEEF, 0, 1).unwrap();

    for pid in 2..=11u8 {
        b.add_leaver_loading(pid, 1, 0);
    }

    // The whole point: N clocked, action-free ticks. The clock advances
    // TICK_MS each, so the reveal must land at ticks*TICK_MS ms.
    for _ in 0..ticks {
        b.add_timeslot(TICK_MS, &[]);
    }

    let (body, len_ms) = b.finish().expect("replay body must build");
    let mut w = W3gWriter::new(26, 6059, true);
    w.set_replay_length(len_ms);
    let packed = w.pack(&body);

    let mut f = std::fs::File::create(&out_path).expect("create output");
    f.write_all(&packed).expect("write output");
    f.flush().expect("flush");

    let mm = (len_ms / 1000) / 60;
    let ss = (len_ms / 1000) % 60;
    println!("wrote {} bytes -> {}", packed.len(), out_path.display());
    println!(
        "body {} bytes, {} ticks, replay {} ms = {:02}:{:02}",
        body.len(),
        ticks,
        len_ms,
        mm,
        ss
    );
}
