//! Generates a realistic match replay: 10-player DotA game, ~60 s of ticks with
//! real action blocks, loading/shop chats, and a final leave. Used to verify
//! that Game.dll 1.26a can load and play a replay produced by this crate.
//!
//! Usage: gen_match_replay [out.w3g]

use std::io::Write;
use std::path::PathBuf;

use ghost_spectator::{ReplayBody, W3gWriter};

const BLOCK_SIZE: usize = 8192;

/// W3GS_INCOMING_ACTION_INTERVAL: real DotA hosts tick at 100 ms.
const TICK_MS: u16 = 100;

/// Slot wire bytes for 1 host + 9 humans + open so war3 sees 10 players.
const SLOT_STATUS_OCCUPIED: u8 = 2;
const SLOT_STATUS_OPEN: u8 = 0;

fn slot(pid: u8, status: u8, team: u8, color: u8, race: u8) -> Vec<u8> {
    // [pid][download=None][status][computer=0][team][colour][race][aiType=1][handicap=100]
    vec![pid, 100, status, 0, team, color, race, 1, 100]
}

/// Build the raw action payload for one tick: `[u8 count][perAction: pid u8,
/// len u16 le, data]`.
///
/// The action bytes must be *valid* W3GS actions, not arbitrary filler. A
/// replay body is executed by the real map script, so filler decodes as garbage
/// orders — an earlier version emitted `seed + 0x10` leading bytes, and 0x10 is
/// the unit-ability order opcode, which crashed viewers at Game.dll+0x473170.
///
/// Both actions below are complete and side-effect-free:
///   0x16 ChangeSelection [mode][count u16] with count 0 — selects nothing.
///   0x61 CancelHeroRevival-class no-operand action — single opcode byte.
fn tick_actions(pid: u8, seed: u8) -> Vec<u8> {
    const ACTION_CHANGE_SELECTION: u8 = 0x16;
    const ACTION_NO_OPERAND: u8 = 0x61;

    // Alternate the two so consecutive ticks are not byte-identical, which keeps
    // the zlib block sizes representative of a real match.
    let a1: Vec<u8> = vec![ACTION_CHANGE_SELECTION, 1 + (seed & 1), 0x00, 0x00];
    let a2: Vec<u8> = vec![ACTION_NO_OPERAND];

    let mut out = Vec::with_capacity(16);
    out.push(2); // action count
    out.push(pid);
    out.extend_from_slice(&(a1.len() as u16).to_le_bytes());
    out.extend_from_slice(&a1);
    out.push(pid);
    out.extend_from_slice(&(a2.len() as u16).to_le_bytes());
    out.extend_from_slice(&a2);
    out
}

fn main() {
    let out_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("match_replay.w3g"));

    let host_pid = 1;
    let mut b = ReplayBody::new(host_pid, "iCCup");

    // Stat string built exactly like the engine's `build_replay_stat_string`:
    // flags u32, 0, width u32, height u32, crc u32, mappath\0, host\0, then the
    // Battle.net even/odd mask encoding (encode_statstring).
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

    let mut raw = Vec::new();
    raw.extend_from_slice(&0x01u32.to_le_bytes()); // map flags
    raw.push(0);
    raw.extend_from_slice(&(1920u16).to_le_bytes()); // map width
    raw.extend_from_slice(&(1080u16).to_le_bytes()); // map height
    raw.extend_from_slice(&0x1234_5678u32.to_le_bytes()); // map crc
    raw.extend_from_slice(b"Maps\\Download\\iCCup DotA 507.w3x");
    raw.push(0);
    raw.extend_from_slice(b"iCCup");
    raw.push(0);
    let stat = encode_statstring(&raw);

    b.set_game("iCCup DotA 507 [AP]", &stat, 0x01);

    // Players: host is pid 1, joiners 2..=10, one open slot.
    b.add_player(2, "Kuular.A");
    b.add_player(3, "Happy_Core");
    b.add_player(4, "MixFight75");
    b.add_player(5, "boboy72");
    b.add_player(6, "TaTeMeF");
    b.add_player(7, "MaRcY-");
    b.add_player(8, "SpartakofSoul");
    b.add_player(9, "kupa-006");
    b.add_player(10, "Izzat41k");
    b.add_player(11, "vezdeKorrupsia");

    // 10 filled slots. Teams split 5/5, colours 0..9, random races.
    let mut slots: Vec<u8> = Vec::new();
    for i in 0..10u8 {
        let pid = i + 1;
        let team = if i < 5 { 0 } else { 1 };
        let race = match i % 6 {
            0 => 1, // human
            1 => 3, // orc
            2 => 2, // night elf
            3 => 4, // undead
            4 => 1,
            _ => 2,
        };
        slots.extend_from_slice(&slot(pid, SLOT_STATUS_OCCUPIED, team, i, race));
    }
    // one open slot for pid 12
    slots.extend_from_slice(&slot(12, SLOT_STATUS_OPEN, 2, 10, 1));

    b.set_start(slots, 0xDEAD_BEEF, 0, 1).unwrap();

    // Simulate loading: everyone in, then a few leavers during load (one
    // disconnects at load -> leavegame reason 2, result 0).
    for pid in 2..=11u8 {
        b.add_leaver_loading(pid, 1, 0); // all done loading
    }

    // 600 ticks = 60 seconds of game time. Each tick: a couple of random
    // players move units. Sprinkle chats and one game-over leave.
    let mut tick = 0u32;
    while tick < 600 {
        let mut actions = Vec::new();
        // 1-3 players act per tick
        let acting = 1 + (tick % 3);
        for k in 0..acting {
            let pid = 2 + (((tick + k as u32) % 10) as u8);
            actions.extend_from_slice(&tick_actions(pid, (tick % 250) as u8 + k as u8));
        }
        b.add_timeslot(TICK_MS, &actions);

        // occasional chatter
        if tick % 100 == 0 {
            let who = 2 + ((tick % 9) as u8);
            b.add_chat(who, 0x20, 0, "gl hf");
        }
        tick += 1;
    }

    // Game over leave.
    b.add_leaver(1, 1, 0);

    let (body, len_ms) = b.finish().expect("replay body must build");
    let mut w = W3gWriter::new(26, 6059, true);
    w.set_replay_length(len_ms);
    let packed = w.pack(&body);

    let mut f = std::fs::File::create(&out_path).expect("create output");
    f.write_all(&packed).expect("write output");
    f.flush().expect("flush");

    println!("wrote {} bytes -> {}", packed.len(), out_path.display());
    println!("body {} bytes, replay ~{} ms = {:.1} s", body.len(), len_ms, len_ms as f32 / 1000.0);
    println!("blocks declared: {}", packed[44..48].iter().rev().fold(0u32, |a, &b| (a << 8) | b as u32));
    let _ = BLOCK_SIZE;
}