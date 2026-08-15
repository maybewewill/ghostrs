# ghostrs ↔ GHost++ Parity & DotaTV Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the behavioural gap between the existing `ghostrs` workspace and the original GHost++ C++ bot, fix the four defects that make real Warcraft III 1.26a clients misbehave, and turn the spectator relay into a working DotaTV stream with a matching client protocol.

**Architecture:** The workspace already runs the correct shape — one tokio actor task owns each `GameState`, packets are pre-encoded once and shared as `Bytes`, the tick is deadline-scheduled. This plan does not change that shape. It adds the missing *content*: a virtual host player, a valid `.w3g` container, the DotaTV framed protocol (`0xFD`) on both the Rust relay and the C++ client, ~76 missing bot commands, real BNCS NLS logon, and the two places where blocking work still sits on the actor thread.

**Tech Stack:** Rust 2024 edition, tokio 1.45, `tokio_util::codec`, `bytes`, `rusqlite` (WAL), `flate2`, `crc32fast`, `tracing`, `criterion`, `proptest`. C++ side: `dotatv_client` (MSVC, winsock2, minhook).

---

## Global Constraints

- **Wire format is law.** Every W3GS/BNCS/GPS byte must match GHost++ exactly. Where this plan cites a C++ line number, that file is the authority: `C:\Users\slash\iccwc3_work\ref\ghostpp\ghost\`.
- **Never block the actor thread.** No `std::fs`, no `zlib`, no `rusqlite` call inside `GameState::on_tick` or `handle_cmd`. Offload via `tokio::task::spawn_blocking` or a dedicated writer task.
- **Never `await` a socket from the tick.** All player sends go through `PlayerLink::try_send`; backpressure marks the player as left.
- **No `unsafe`.** `ghost-protocol` has `#![forbid(unsafe_code)]`; keep it.
- **TDD.** Every task writes the failing test first, watches it fail, then implements.
- **Commit per task.** Working tree must build (`cargo check --workspace`) and pass (`cargo test --workspace`) at every commit.
- Target Warcraft III version: **1.26a (version 26, build 6059)**, TFT (`W3XP`).
- Rust toolchain: 1.96.1 or newer.

---

## Current State (verified 2026-08-15)

`cargo test --workspace` passes. 10 crates, ~7 400 lines of Rust:

| Crate | Lines | Status |
|---|---|---|
| `ghost-protocol` | ~950 | W3GS/BNCS/GPS codecs, 37 W3GS ids, statstring, slots |
| `ghost-engine` | ~2 200 | actor, tick, lobby, slots, players, chat, actions, lagcheck, gproxy, mapxfer, map (MPQ), hcl, stats |
| `ghost-net` | ~300 | listener, dual-codec conn, UDP broadcast |
| `ghost-bnet` | ~540 | BNCS client state machine, advert, auth helpers |
| `ghost-store` | ~560 | WAL SQLite + blocking writer task |
| `ghost-spectator` | ~300 | delayed relay, replay writer |
| `ghostrs` (bin) | ~940 | typed config, supervisor |
| `ghost-loadtest` | ~190 | synthetic load harness |
| `ghost-legacy-attic` | ~820 | unwired ports: `packed.rs`, `savegame.rs`, `stats_dota.rs`, `stats_w3mmd.rs` |

### Gap analysis vs GHost++

**Defects that break real clients** (evidence-backed, each becomes a regression test):

1. **No virtual host player.** `cfg.virtual_host_name` is read from config (`crates/ghostrs/src/config.rs:153`), threaded into `GameConfig` (`crates/ghostrs/src/supervisor.rs:453`), stored in `GameState` (`crates/ghost-engine/src/state.rs:60`) — and never used. GHost++ creates it at `game_base.cpp:4702` and sends `W3GS_PLAYERINFO` for it (`:4713`), then sends all bot chat *from that PID* (`:1268`). `ghostrs` sends chat from PID 255 (`crates/ghost-engine/src/state.rs:202`) with no matching PLAYERINFO — clients have no sender to attribute it to, and the lobby shows one fewer occupant than GHost++.
2. **Every produced `.w3g` replay is invalid.** `crates/ghost-spectator/src/replay.rs` writes a 68-byte header but never writes flags (must be `32768`, `packed.cpp:341` / `replay.cpp:139`), never writes replay length (offset 60), and never computes the header CRC32 at offset 64 (`packed.cpp:346-358`). The tail block is not padded to 8192 (`packed.cpp:286`), and the block CRC uses `crc32fast::hash(compressed)` instead of the required folded pair (`packed.cpp:377-382`). Additionally the whole replay *body* prefix (`CReplay::BuildReplay`, `replay.cpp:135-212`: host PlayerRecord, GameName, StatString, PlayerList, GameStartRecord, three start blocks) is absent, so even a byte-correct container decompresses to garbage.
3. **DotaTV is disconnected end to end.** `crates/ghost-spectator/src/relay.rs:157` sends binary W3GS `CHAT_FROM_HOST` to viewers, but `dotatv_client/src/NetClient.cpp:53` sends newline-terminated text and hands one raw `recv()` buffer to `PacketCallback` with no framing. Viewer→relay traffic is dropped on the floor (`relay.rs:121-123` spawns a task that discards every `ConnEvent`), and viewers are attached with `ghost_net::spawn_conn`, whose `DualCodec` only accepts `0xF7`/`0xF8` — a `0xFD` byte would be resynced away. No game blocks are ever pushed by the engine.
4. **GProxy buffers start too late.** `crates/ghost-engine/src/actor.rs:118` allocates `gproxy_buffer` only when a `GPS_ACK` arrives. Every packet sent between join and that first ACK is unrecoverable, so an early disconnect cannot be replayed.

**Missing feature surface:**

| Area | GHost++ | ghostrs | Gap |
|---|---|---|---|
| In-game `!` commands | 62 (`game.cpp:396-1782`) | 35 (`chat.rs:59-110`) | 31 missing (Task 10-11) |
| BNET whisper/channel commands | 45 (`bnet.cpp:1191-2103`) | 0 | all (Task 12) |
| Admin game | `game_admin.cpp`, 1 088 lines | — | **explicitly dropped** (see Assumptions) |
| BNCS logon | NLS/SRP via bncsutil | old-logon only, SRP proof unimplemented | Task 14 |
| CD-key / version hash | bncsutil `checkRevision` | `SHA1(ct+st+key)` placeholder | Task 15 |
| DB tables | bans, admins, games, gameplayers, dotagames, dotaplayers, downloads, w3mmd, scores | 7 of 9 | `downloads`, `scores` (Task 13) |
| Savegame `!load` | `savegame.cpp` | in attic, unwired | out of scope |
| MySQL backend | `ghostdbmysql.cpp` | — | out of scope (SQLite chosen) |
| Localisation | `language.cpp`, 1 305 lines | `lang.rs`, 34 lines | out of scope |

---

## File Structure

**Created:**
- `crates/ghost-protocol/src/dtv/mod.rs` — DotaTV `0xFD` message ids, encoders, decoders.
- `crates/ghost-spectator/src/conn.rs` — `spawn_dtv_conn`, a `HeaderCodec<0xFD>` connection task for viewers.
- `crates/ghost-spectator/src/w3g.rs` — the `.w3g` container writer (header CRC, 8192 padding, block CRC).
- `crates/ghost-spectator/src/body.rs` — the replay body prefix (`BuildReplay` equivalent).
- `crates/ghost-engine/src/commands/` — `lobby_cmds.rs`, `game_cmds.rs`, `vote.rs`, `comp.rs`.
- `crates/ghost-bnet/src/commands.rs` — the BNET whisper/channel command router.
- `crates/ghost-bnet/src/nls.rs` — SRP-6a (NLS) client.
- `crates/ghost-bnet/src/cdkey.rs` — CD-key decode + version hash.
- `crates/ghost-store/src/queries.rs` — `!stats` / `!statsdota` / `downloads` queries.

**Modified:**
- `crates/ghost-engine/src/state.rs` — virtual host fields, chat sender PID.
- `crates/ghost-engine/src/lobby.rs` — create/delete virtual host, allocate GProxy buffer at join.
- `crates/ghost-engine/src/chat.rs` — extend `ChatCommand` and the parser.
- `crates/ghost-engine/src/actor.rs` — command dispatch, relay pushes.
- `crates/ghost-spectator/src/relay.rs` — DTV protocol, inbound viewer frames, bounded queue.
- `crates/ghostrs/src/supervisor.rs` — wire relay + replay + BNET commands.
- `dotatv_client/include/NetClient.hpp`, `dotatv_client/src/NetClient.cpp` — framing.
- `dotatv_client/src/DotaTV.cpp` — DTV message handling.

---

### Task 1: Virtual Host Player

**Files:**
- Modify: `crates/ghost-engine/src/state.rs:76-105` (fields), `:191-206` (`send_chat_all`)
- Modify: `crates/ghost-engine/src/lobby.rs:12-110`
- Test: `crates/ghost-engine/src/lobby.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `PlayerTable::next_free_pid() -> Option<u8>`, `outgoing::player_info`, `outgoing::player_leave_others`.
- Produces: `GameState::virtual_host_pid: u8` (255 = none), `GameState::create_virtual_host()`, `GameState::delete_virtual_host()`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn the_virtual_host_is_announced_to_a_joining_player() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(0);
    st.create_virtual_host();
    assert_ne!(st.virtual_host_pid, 255, "a virtual host PID must be allocated");

    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    st.add_conn(7, ghost_net::PlayerLink::for_test(tx), [127, 0, 0, 1]);
    st.handle_req_join(7, &crate::actor::tests_support::reqjoin_bytes("alice"));

    let ids = crate::actor::tests_support::drain_ids(&mut rx);
    let vh = st.virtual_host_pid;
    assert!(
        ids.contains(&ghost_protocol::w3gs::ids::PLAYER_INFO),
        "joiner must be told about the virtual host, got {ids:?}"
    );
    assert_eq!(st.players.by_pid(vh).map(|p| p.name.as_str()), Some(st.cfg.virtual_host_name.as_str()));
}

#[tokio::test]
async fn bot_chat_is_sent_from_the_virtual_host_pid() {
    let (mut st, mut rxs) = crate::actor::tests_support::seated_game(1);
    st.create_virtual_host();
    st.send_chat_all("hello");
    let pkt = rxs[0].try_recv().expect("chat packet");
    // [0xF7, 0x0F, len_lo, len_hi, from_pid, ...]
    assert_eq!(pkt[1], ghost_protocol::w3gs::ids::CHAT_FROM_HOST);
    assert_eq!(pkt[4], st.virtual_host_pid, "sender must be the virtual host, not 255");
}

#[tokio::test]
async fn the_virtual_host_makes_way_for_the_last_real_player() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(0);
    st.create_virtual_host();
    let vh = st.virtual_host_pid;
    // Fill every slot but one; the virtual host must step aside for the last seat.
    for i in 0..(st.cfg.num_slots - 1) {
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        st.add_conn(100 + i as u64, ghost_net::PlayerLink::for_test(tx), [0; 4]);
        st.handle_req_join(100 + i as u64, &crate::actor::tests_support::reqjoin_bytes(&format!("p{i}")));
    }
    assert_eq!(st.virtual_host_pid, 255, "virtual host must be deleted");
    assert!(st.players.by_pid(vh).is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghost-engine virtual_host`
Expected: FAIL — `no method named create_virtual_host`, `no field virtual_host_pid`.

- [ ] **Step 3: Add the fields**

In `crates/ghost-engine/src/state.rs`, inside `pub struct GameState`:

```rust
    /// PID of the fake "bot" player shown in the lobby, or 255 when absent.
    /// Mirrors GHost++ `m_VirtualHostPID` (game_base.cpp:4702).
    pub virtual_host_pid: u8,
```

In `GameState::new`, initialise `virtual_host_pid: 255,`.

- [ ] **Step 4: Implement create/delete**

Append to `impl GameState` in `crates/ghost-engine/src/state.rs`:

```rust
    /// Seats a socket-less player so clients have a sender to attribute bot chat
    /// to, and so the lobby count matches GHost++. No-op when one already exists.
    pub fn create_virtual_host(&mut self) {
        if self.virtual_host_pid != 255 {
            return;
        }
        let Some(pid) = self.players.next_free_pid() else {
            return;
        };
        // The link is a dead channel: nothing is ever read from it, and every
        // try_send fails with Closed, which reap_left_players must not act on.
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        std::mem::forget(rx); // keep the channel open so try_send reports Backpressure, not Closed
        let mut p = crate::players::Player::new(pid, self.cfg.virtual_host_name.clone(), u64::MAX, PlayerLink::for_test(tx));
        p.virtual_host = true;
        p.loaded = true;
        self.virtual_host_pid = pid;

        let ip = [0u8; 4];
        if let Ok(b) = outgoing::player_info(pid, &self.cfg.virtual_host_name, ip, ip) {
            self.broadcast(b);
        }
        self.players.insert(p);
    }

    /// Removes the virtual host, freeing its PID for a real player.
    pub fn delete_virtual_host(&mut self) {
        if self.virtual_host_pid == 255 {
            return;
        }
        let pid = self.virtual_host_pid;
        self.virtual_host_pid = 255;
        self.players.remove_pid(pid);
        // PLAYERLEAVE_LOBBY == 13, matching game_base.cpp:4721.
        self.broadcast(outgoing::player_leave_others(pid, 13));
    }
```

Add `pub virtual_host: bool,` to `Player` in `crates/ghost-engine/src/players.rs` (default `false` in `Player::new`).

- [ ] **Step 5: Route chat through the virtual host**

Replace the `from_pid` argument in `send_chat_all` (`crates/ghost-engine/src/state.rs:202`):

```rust
        let from = if self.virtual_host_pid != 255 { self.virtual_host_pid } else { 255 };
        let pids: Vec<u8> = self.players.iter().filter(|p| !p.virtual_host).map(|p| p.pid).collect();
        if pids.is_empty() {
            return;
        }
        match outgoing::chat_from_host(from, &pids, flag, extra, message) {
```

- [ ] **Step 6: Exclude the virtual host from broadcast and reaping**

In `GameState::broadcast` (`state.rs:156`), skip it — it has no socket:

```rust
        for p in self.players.iter_mut() {
            if p.left.is_some() || p.virtual_host {
                continue;
            }
```

In `reap_left_players` (`state.rs:211`), skip it in the `filter_map` the same way.

- [ ] **Step 7: Free the seat for the last real player**

At the end of `handle_req_join` in `crates/ghost-engine/src/lobby.rs` (after `send_all_slot_info()`), add:

```rust
        // GHost++ deletes the virtual host once only one seat is left, so the
        // lobby can still fill completely (game_base.cpp:2052).
        let real_players = self.players.iter().filter(|p| !p.virtual_host).count();
        if real_players >= self.cfg.num_slots - 1 {
            self.delete_virtual_host();
        }
```

And in `GameState::begin_loading` (`crates/ghost-engine/src/actions.rs:54`), call `self.delete_virtual_host();` before broadcasting `countdown_start` — GHost++ does this at `game_base.cpp:3389`.

- [ ] **Step 8: Create it when the game is created**

In `crates/ghostrs/src/supervisor.rs`, immediately after `spawn_game(game_cfg)` returns (`:459`), send a new command. Add to `GameCmd` in `crates/ghost-engine/src/handle.rs`:

```rust
    CreateVirtualHost,
```

and to `handle_cmd` in `crates/ghost-engine/src/actor.rs`:

```rust
            GameCmd::CreateVirtualHost => self.create_virtual_host(),
```

then in the supervisor: `handle.send(GameCmd::CreateVirtualHost);`

- [ ] **Step 9: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected: PASS, including the three new tests.

- [ ] **Step 10: Commit**

```bash
git add crates/ghost-engine/src/state.rs crates/ghost-engine/src/lobby.rs crates/ghost-engine/src/players.rs crates/ghost-engine/src/actor.rs crates/ghost-engine/src/actions.rs crates/ghost-engine/src/handle.rs crates/ghostrs/src/supervisor.rs
git commit -m "fix(engine): add the virtual host player and send bot chat from its PID"
```

---

### Task 2: Valid .w3g Container

**Files:**
- Create: `crates/ghost-spectator/src/w3g.rs`
- Modify: `crates/ghost-spectator/src/lib.rs`
- Test: `crates/ghost-spectator/src/w3g.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `W3gWriter::new(war3_version: u32, build: u16, tft: bool) -> Self`, `W3gWriter::set_replay_length(&mut self, ms: u32)`, `W3gWriter::pack(&self, decompressed: &[u8]) -> Vec<u8>`.
- Consumed by Task 3.

This replaces the streaming `ReplayWriter`. GHost++ builds the whole decompressed body in memory then packs it once (`packed.cpp:275-394`); a 60-minute DotA replay is ~2-4 MB decompressed, so doing the same is correct and far simpler than streaming.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn read_u32(d: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
    }

    #[test]
    fn the_header_carries_the_flags_length_and_a_self_consistent_crc() {
        let mut w = W3gWriter::new(26, 6059, true);
        w.set_replay_length(123_456);
        let out = w.pack(&vec![0xABu8; 100]);

        assert_eq!(&out[..28], b"Warcraft III recorded game\x1A\0");
        assert_eq!(read_u32(&out, 28), 68, "header size");
        assert_eq!(read_u32(&out, 36), 1, "header version");
        assert_eq!(&out[48..52], b"PX3W", "W3XP, little-endian on the wire");
        assert_eq!(read_u32(&out, 52), 26, "war3 version");
        assert_eq!(u16::from_le_bytes([out[56], out[57]]), 6059, "build");
        assert_eq!(u16::from_le_bytes([out[58], out[59]]), 32768, "flags must be 32768");
        assert_eq!(read_u32(&out, 60), 123_456, "replay length ms");

        // The stored CRC must equal CRC32 of the header with its CRC field zeroed.
        let stored = read_u32(&out, 64);
        let mut probe = out[..68].to_vec();
        probe[64..68].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(stored, crc32fast::hash(&probe), "header CRC mismatch");
        assert_ne!(stored, 0);
    }

    #[test]
    fn every_block_decompresses_to_exactly_8192_bytes() {
        let w = W3gWriter::new(26, 6059, true);
        // 3 blocks worth, with a deliberately ragged tail.
        let body = vec![0x5Au8; 8192 * 2 + 7];
        let out = w.pack(&body);

        let n_blocks = read_u32(&out, 44) as usize;
        assert_eq!(n_blocks, 3);
        assert_eq!(read_u32(&out, 40) as usize, 8192 * 3, "decompressed size must be padded");

        let mut pos = 68;
        for i in 0..n_blocks {
            let c_len = u16::from_le_bytes([out[pos], out[pos + 1]]) as usize;
            let u_len = u16::from_le_bytes([out[pos + 2], out[pos + 3]]) as usize;
            assert_eq!(u_len, 8192, "block {i} uncompressed size");
            let comp = &out[pos + 8..pos + 8 + c_len];
            let mut dec = Vec::new();
            flate2::read::ZlibDecoder::new(comp)
                .read_to_end(&mut dec)
                .expect("block must be valid zlib");
            assert_eq!(dec.len(), 8192, "block {i} must inflate to 8192 bytes");
            pos += 8 + c_len;
        }
        assert_eq!(pos, out.len(), "compressed size accounting");
        assert_eq!(read_u32(&out, 32) as usize, out.len(), "field 32 is the whole file size");
    }

    #[test]
    fn the_block_crc_folds_the_header_and_data_checksums() {
        let w = W3gWriter::new(26, 6059, true);
        let out = w.pack(&vec![1u8; 10]);
        let c_len = u16::from_le_bytes([out[68], out[69]]) as usize;

        let mut bh = out[68..76].to_vec();
        bh[4..8].copy_from_slice(&0u32.to_le_bytes());
        let crc1 = { let c = crc32fast::hash(&bh); c ^ (c >> 16) };
        let crc2 = { let c = crc32fast::hash(&out[76..76 + c_len]); c ^ (c >> 16) };
        let expected = (crc1 & 0xFFFF) | (crc2 << 16);

        assert_eq!(u32::from_le_bytes([out[72], out[73], out[74], out[75]]), expected);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghost-spectator w3g`
Expected: FAIL — `unresolved module w3g`.

- [ ] **Step 3: Implement the writer**

Create `crates/ghost-spectator/src/w3g.rs`:

```rust
//! `.w3g` replay container. Byte-for-byte equivalent to GHost++ `CPacked::Compress`
//! (ref/ghostpp/ghost/packed.cpp:275-394).
use std::io::{Read, Write};

use flate2::Compression;
use flate2::write::ZlibEncoder;

const HEADER_SIZE: u32 = 68;
const BLOCK_SIZE: usize = 8192;
/// GHost++ hardcodes this; the client validates it (replay.cpp:234).
const FLAGS_MULTIPLAYER: u16 = 32768;

pub struct W3gWriter {
    war3_version: u32,
    build: u16,
    tft: bool,
    replay_length_ms: u32,
}

impl W3gWriter {
    pub fn new(war3_version: u32, build: u16, tft: bool) -> Self {
        Self { war3_version, build, tft, replay_length_ms: 0 }
    }

    pub fn set_replay_length(&mut self, ms: u32) {
        self.replay_length_ms = ms;
    }

    /// Packs an already-built replay body into a complete `.w3g` file.
    pub fn pack(&self, decompressed: &[u8]) -> Vec<u8> {
        // Every block must inflate to exactly BLOCK_SIZE, so the tail is padded.
        let mut padded = decompressed.to_vec();
        let pad = BLOCK_SIZE - (padded.len() % BLOCK_SIZE);
        padded.resize(padded.len() + pad, 0);

        let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(padded.len() / BLOCK_SIZE);
        for chunk in padded.chunks_exact(BLOCK_SIZE) {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(chunk).expect("zlib encode into Vec cannot fail");
            blocks.push(enc.finish().expect("zlib finish into Vec cannot fail"));
        }

        let compressed_total: usize = blocks.iter().map(|b| b.len()).sum();
        let file_size = HEADER_SIZE as usize + compressed_total + blocks.len() * 8;

        let mut header = Vec::with_capacity(HEADER_SIZE as usize);
        header.extend_from_slice(b"Warcraft III recorded game\x1A\0");
        header.extend_from_slice(&HEADER_SIZE.to_le_bytes());
        header.extend_from_slice(&(file_size as u32).to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes()); // header version
        header.extend_from_slice(&(padded.len() as u32).to_le_bytes());
        header.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        // "W3XP"/"WAR3" stored reversed on the wire (packed.cpp:326-336).
        header.extend_from_slice(if self.tft { b"PX3W" } else { b"3RAW" });
        header.extend_from_slice(&self.war3_version.to_le_bytes());
        header.extend_from_slice(&self.build.to_le_bytes());
        header.extend_from_slice(&FLAGS_MULTIPLAYER.to_le_bytes());
        header.extend_from_slice(&self.replay_length_ms.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
        debug_assert_eq!(header.len(), HEADER_SIZE as usize);

        let crc = crc32fast::hash(&header);
        header[64..68].copy_from_slice(&crc.to_le_bytes());

        let mut out = Vec::with_capacity(file_size);
        out.extend_from_slice(&header);
        for block in &blocks {
            let mut bh = Vec::with_capacity(8);
            bh.extend_from_slice(&(block.len() as u16).to_le_bytes());
            bh.extend_from_slice(&(BLOCK_SIZE as u16).to_le_bytes());
            bh.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder

            // Folded 16+16 checksum, packed.cpp:377-382.
            let crc1 = { let c = crc32fast::hash(&bh); c ^ (c >> 16) };
            let crc2 = { let c = crc32fast::hash(block); c ^ (c >> 16) };
            let block_crc = (crc1 & 0xFFFF) | (crc2 << 16);
            bh[4..8].copy_from_slice(&block_crc.to_le_bytes());

            out.extend_from_slice(&bh);
            out.extend_from_slice(block);
        }
        debug_assert_eq!(out.len(), file_size);
        out
    }
}
```

Add `use std::io::Read;` to the test module and `pub mod w3g;` + `pub use w3g::W3gWriter;` to `crates/ghost-spectator/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghost-spectator w3g`
Expected: PASS (3 tests).

- [ ] **Step 5: Delete the broken writer**

Delete `crates/ghost-spectator/src/replay.rs` and its `pub mod replay;` line. Delete the `replay_header_is_written_and_the_block_count_updated` test in `crates/ghost-spectator/src/relay.rs:237-249` and the `use crate::replay::ReplayWriter;` on `:198`.

- [ ] **Step 6: Verify the workspace still builds**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-spectator/src/w3g.rs crates/ghost-spectator/src/lib.rs crates/ghost-spectator/src/relay.rs
git rm crates/ghost-spectator/src/replay.rs
git commit -m "fix(spectator): write a valid w3g container with header CRC, padding and folded block CRCs"
```

---

### Task 3: Replay Body and Off-Thread Saving

**Files:**
- Create: `crates/ghost-spectator/src/body.rs`
- Modify: `crates/ghost-spectator/src/lib.rs`, `crates/ghost-engine/src/state.rs`, `crates/ghost-engine/src/actions.rs`
- Test: `crates/ghost-spectator/src/body.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `W3gWriter` (Task 2), `GameState::virtual_host_pid` (Task 1).
- Produces: `ReplayBody::new(host_pid: u8, host_name: &str) -> Self`, `ReplayBody::add_player(&mut self, pid: u8, name: &str)`, `ReplayBody::set_start(&mut self, slots: Vec<u8>, random_seed: u32, select_mode: u8, start_spots: u8)`, `ReplayBody::add_timeslot(&mut self, time_increment: u16, actions: &[u8])`, `ReplayBody::add_chat(&mut self, pid: u8, flag: u8, extra: u32, message: &str)`, `ReplayBody::add_leaver(&mut self, pid: u8, reason: u32, result: u32)`, `ReplayBody::finish(self) -> (Vec<u8>, u32)` returning body bytes and replay length in ms.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_opens_with_the_host_record_and_the_three_start_blocks() {
        let mut b = ReplayBody::new(1, "iCCup");
        b.add_player(2, "alice");
        b.set_start(vec![0u8; 9 * 2], 0xDEADBEEF, 0, 2);
        let (body, len_ms) = b.finish();

        assert_eq!(&body[0..4], &[16, 1, 0, 0], "unknown 4.0");
        assert_eq!(body[4], 0, "host RecordID");
        assert_eq!(body[5], 1, "host PID");
        assert_eq!(&body[6..12], b"iCCup\0", "host name, null terminated");
        assert_eq!(len_ms, 0);

        // RecordID 25 introduces the GameStartRecord, then 0x1A/0x1B/0x1C.
        let start = body.windows(1).position(|w| w[0] == 25).expect("GameStartRecord");
        assert_eq!(body[start + 1..start + 3], (7u16 + 2 * 9).to_le_bytes());
        let tail = &body[body.len() - 15..];
        assert_eq!(tail, &[0x1A, 1, 0, 0, 0, 0x1B, 1, 0, 0, 0, 0x1C, 1, 0, 0, 0]);
    }

    #[test]
    fn timeslots_accumulate_the_replay_length() {
        let mut b = ReplayBody::new(1, "h");
        b.set_start(vec![0u8; 9], 1, 0, 1);
        b.add_timeslot(100, &[0xAA]);
        b.add_timeslot(150, &[0xBB]);
        let (_body, len_ms) = b.finish();
        assert_eq!(len_ms, 250, "replay length is the sum of time increments");
    }

    #[test]
    fn a_timeslot_block_is_length_prefixed_after_its_first_four_bytes() {
        let mut b = ReplayBody::new(1, "h");
        b.set_start(vec![0u8; 9], 1, 0, 1);
        b.add_timeslot(100, &[0xAA, 0xBB]);
        let (body, _) = b.finish();

        // Locate the 0x1E block: [0x1E][u16 len][u16 time][actions...]
        let at = body.windows(5)
            .position(|w| w[0] == 0x1E && u16::from_le_bytes([w[3], w[4]]) == 100)
            .expect("timeslot block");
        let len = u16::from_le_bytes([body[at + 1], body[at + 2]]) as usize;
        assert_eq!(len, 2 + 2, "length counts time increment plus actions");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghost-spectator body`
Expected: FAIL — `unresolved module body`.

- [ ] **Step 3: Implement the body builder**

Create `crates/ghost-spectator/src/body.rs`:

```rust
//! The decompressed replay body. Mirrors GHost++ `CReplay::BuildReplay`
//! (ref/ghostpp/ghost/replay.cpp:135-212) and `CReplay::AddTimeSlot`/`AddChatMessage`.

const REPLAY_FIRSTSTARTBLOCK: u8 = 0x1A;
const REPLAY_SECONDSTARTBLOCK: u8 = 0x1B;
const REPLAY_THIRDSTARTBLOCK: u8 = 0x1C;
const REPLAY_TIMESLOTBLOCK: u8 = 0x1E;
const REPLAY_CHATMESSAGE: u8 = 0x20;
const REPLAY_LEAVEGAME: u8 = 0x17;
/// GHost++ hardcodes this language id (replay.cpp:143).
const LANGUAGE_ID: u32 = 0x0012_F8B0;

pub struct ReplayBody {
    host_pid: u8,
    host_name: String,
    players: Vec<(u8, String)>,
    slots: Vec<u8>,
    random_seed: u32,
    select_mode: u8,
    start_spots: u8,
    num_slots: usize,
    game_name: String,
    stat_string: Vec<u8>,
    map_game_type: u32,
    blocks: Vec<u8>,
    replay_length_ms: u32,
}

fn put_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

impl ReplayBody {
    pub fn new(host_pid: u8, host_name: &str) -> Self {
        Self {
            host_pid,
            host_name: host_name.to_string(),
            players: Vec::new(),
            slots: Vec::new(),
            random_seed: 0,
            select_mode: 0,
            start_spots: 0,
            num_slots: 0,
            game_name: String::new(),
            stat_string: Vec::new(),
            map_game_type: 0,
            blocks: Vec::new(),
            replay_length_ms: 0,
        }
    }

    pub fn set_game(&mut self, game_name: &str, stat_string: &[u8], map_game_type: u32) {
        self.game_name = game_name.to_string();
        self.stat_string = stat_string.to_vec();
        self.map_game_type = map_game_type;
    }

    pub fn add_player(&mut self, pid: u8, name: &str) {
        if pid != self.host_pid {
            self.players.push((pid, name.to_string()));
        }
    }

    /// `slots` is the raw 9-bytes-per-slot wire form used by W3GS_SLOTINFO.
    pub fn set_start(&mut self, slots: Vec<u8>, random_seed: u32, select_mode: u8, start_spots: u8) {
        self.num_slots = slots.len() / 9;
        self.slots = slots;
        self.random_seed = random_seed;
        self.select_mode = select_mode;
        self.start_spots = start_spots;
    }

    /// One 100 ms action packet. `actions` is the payload of W3GS_INCOMING_ACTION
    /// *after* the send-interval field, i.e. the CRC and action blocks.
    pub fn add_timeslot(&mut self, time_increment: u16, actions: &[u8]) {
        self.replay_length_ms += time_increment as u32;
        self.blocks.push(REPLAY_TIMESLOTBLOCK);
        let len = 2 + actions.len();
        self.blocks.extend_from_slice(&(len as u16).to_le_bytes());
        self.blocks.extend_from_slice(&time_increment.to_le_bytes());
        self.blocks.extend_from_slice(actions);
    }

    pub fn add_chat(&mut self, pid: u8, flag: u8, extra: u32, message: &str) {
        self.blocks.push(REPLAY_CHATMESSAGE);
        self.blocks.push(pid);
        // length covers flag + extra + message + terminator
        let len = 1 + 4 + message.len() + 1;
        self.blocks.extend_from_slice(&(len as u16).to_le_bytes());
        self.blocks.push(flag);
        self.blocks.extend_from_slice(&extra.to_le_bytes());
        put_cstr(&mut self.blocks, message);
    }

    pub fn add_leaver(&mut self, pid: u8, reason: u32, result: u32) {
        self.blocks.push(REPLAY_LEAVEGAME);
        self.blocks.extend_from_slice(&reason.to_le_bytes());
        self.blocks.push(pid);
        self.blocks.extend_from_slice(&result.to_le_bytes());
        self.blocks.extend_from_slice(&1u32.to_le_bytes());
    }

    /// Returns the decompressed body and the total replay length in ms.
    pub fn finish(self) -> (Vec<u8>, u32) {
        let mut r = Vec::with_capacity(512 + self.blocks.len());
        r.extend_from_slice(&[16, 1, 0, 0]); // unknown (4.0)
        r.push(0); // host RecordID
        r.push(self.host_pid);
        put_cstr(&mut r, &self.host_name);
        r.push(1); // host AdditionalSize
        r.push(0); // host AdditionalData
        put_cstr(&mut r, &self.game_name);
        r.push(0); // null (4.0)
        r.extend_from_slice(&self.stat_string);
        r.extend_from_slice(&(self.num_slots as u32).to_le_bytes());
        r.extend_from_slice(&self.map_game_type.to_le_bytes());
        r.extend_from_slice(&LANGUAGE_ID.to_le_bytes());

        for (pid, name) in &self.players {
            r.push(22); // player RecordID
            r.push(*pid);
            put_cstr(&mut r, name);
            r.push(1);
            r.push(0);
            r.extend_from_slice(&0u32.to_le_bytes());
        }

        r.push(25); // GameStartRecord
        r.extend_from_slice(&((7 + self.num_slots * 9) as u16).to_le_bytes());
        r.push(self.num_slots as u8);
        r.extend_from_slice(&self.slots);
        r.extend_from_slice(&self.random_seed.to_le_bytes());
        r.push(self.select_mode);
        r.push(self.start_spots);

        r.push(REPLAY_FIRSTSTARTBLOCK);
        r.extend_from_slice(&1u32.to_le_bytes());
        r.push(REPLAY_SECONDSTARTBLOCK);
        r.extend_from_slice(&1u32.to_le_bytes());
        r.push(REPLAY_THIRDSTARTBLOCK);
        r.extend_from_slice(&1u32.to_le_bytes());

        r.extend_from_slice(&self.blocks);
        let len = self.replay_length_ms;
        (r, len)
    }
}
```

Note: the three start blocks must appear *before* the accumulated blocks. The test asserts them at the tail only because that test never calls `add_timeslot`; keep the ordering above and adjust the first test's tail assertion to slice the 15 bytes that precede `self.blocks` when blocks are empty — with no blocks they are the last 15 bytes, so the assertion holds as written.

Add `pub mod body;` + `pub use body::ReplayBody;` to `crates/ghost-spectator/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghost-spectator body`
Expected: PASS (3 tests).

- [ ] **Step 5: Write the failing integration test for off-thread saving**

Add to `crates/ghost-spectator/src/lib.rs`:

```rust
#[cfg(test)]
mod save_tests {
    use super::*;

    #[tokio::test]
    async fn saving_a_replay_does_not_run_on_the_caller_thread() {
        let dir = std::env::temp_dir().join("ghostrs-w3g-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live.w3g");

        let mut b = ReplayBody::new(1, "host");
        b.set_game("test game", &[0u8; 4], 0);
        b.set_start(vec![0u8; 9], 42, 0, 1);
        b.add_timeslot(100, &[0xAA]);

        save_replay(path.clone(), b, 26, 6059, true).await.unwrap();

        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b"Warcraft III recorded game\x1A\0"));
        assert_eq!(u32::from_le_bytes([data[32], data[33], data[34], data[35]]) as usize, data.len());
        assert_eq!(u32::from_le_bytes([data[60], data[61], data[62], data[63]]), 100);
    }
}
```

- [ ] **Step 6: Run it to verify it fails**

Run: `cargo test -p ghost-spectator save_tests`
Expected: FAIL — `cannot find function save_replay`.

- [ ] **Step 7: Implement the off-thread save**

Add to `crates/ghost-spectator/src/lib.rs`:

```rust
/// Packs and writes a replay on a blocking thread. zlib on a 4 MB body takes
/// tens of milliseconds — far more than one 100 ms tick can spare.
pub async fn save_replay(
    path: std::path::PathBuf,
    body: ReplayBody,
    war3_version: u32,
    build: u16,
    tft: bool,
) -> std::io::Result<()> {
    tokio::task::spawn_blocking(move || {
        let (bytes, len_ms) = body.finish();
        let mut w = W3gWriter::new(war3_version, build, tft);
        w.set_replay_length(len_ms);
        std::fs::write(path, w.pack(&bytes))
    })
    .await
    .map_err(|e| std::io::Error::other(e.to_string()))?
}
```

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p ghost-spectator`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/ghost-spectator/src/body.rs crates/ghost-spectator/src/lib.rs
git commit -m "feat(spectator): build the replay body and save w3g files off the actor thread"
```

---

### Task 4: Wire Replay Recording into the Game Actor

**Files:**
- Modify: `crates/ghost-engine/src/state.rs`, `crates/ghost-engine/src/actions.rs:109-140`, `crates/ghost-engine/src/lobby.rs`, `crates/ghost-engine/src/actor.rs`
- Modify: `crates/ghostrs/src/config.rs`, `crates/ghostrs/src/supervisor.rs`
- Test: `crates/ghost-engine/src/actions.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `ghost_spectator::{ReplayBody, save_replay}` (Task 3), `GameState::virtual_host_pid` (Task 1).
- Produces: `GameState::replay: Option<ReplayBody>`.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn every_action_tick_is_recorded_as_a_replay_timeslot() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
    st.replay = Some(ghost_spectator::ReplayBody::new(1, "host"));
    st.begin_playing();

    st.on_tick(0);
    st.on_tick(0);

    let (_body, len_ms) = st.replay.take().unwrap().finish();
    assert_eq!(len_ms, 200, "two 100 ms ticks, got {len_ms}");
}

#[tokio::test]
async fn a_skipped_tick_is_recorded_with_the_real_elapsed_time() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
    st.replay = Some(ghost_spectator::ReplayBody::new(1, "host"));
    st.begin_playing();

    st.on_tick(2); // two periods were lost to a stall

    let (_body, len_ms) = st.replay.take().unwrap().finish();
    assert_eq!(len_ms, 300, "300 ms of game time really elapsed");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghost-engine replay`
Expected: FAIL — `no field replay on GameState`.

- [ ] **Step 3: Add the field**

In `crates/ghost-engine/src/state.rs`, add to `GameState`:

```rust
    /// Accumulates the replay while the game runs. `None` disables recording.
    pub replay: Option<ghost_spectator::ReplayBody>,
```

Initialise `replay: None,` in `GameState::new`. Add `ghost-spectator = { path = "../ghost-spectator" }` to `crates/ghost-engine/Cargo.toml` if it is not already a dependency.

- [ ] **Step 4: Record timeslots**

In `crates/ghost-engine/src/actions.rs`, inside `send_all_actions`, immediately after each `INCOMING_ACTION` packet is built and before it is broadcast, record it. The recorded bytes are the packet payload minus the 4-byte frame header:

```rust
        if let Some(rep) = self.replay.as_mut() {
            // A replay timeslot carries the send interval plus CRC and action
            // blocks — exactly the INCOMING_ACTION payload after the header.
            rep.add_timeslot(send_interval, &packet[6..]);
        }
```

(`packet[0..4]` is `[0xF7, 0x0C, len_lo, len_hi]` and `packet[4..6]` is the send interval, which `add_timeslot` writes itself.)

- [ ] **Step 5: Record the start record and players**

In `GameState::begin_playing` (`crates/ghost-engine/src/actions.rs:61`), before the HCL injection:

```rust
        if let Some(rep) = self.replay.as_mut() {
            for p in self.players.iter().filter(|p| !p.virtual_host) {
                rep.add_player(p.pid, &p.name);
            }
            rep.set_start(self.slots.as_wire(), self.random_seed, 0, self.cfg.map.num_players);
        }
```

- [ ] **Step 6: Record chat and leavers**

In `GameState::send_chat_all` (`state.rs`), after the packet is built:

```rust
        if let Some(rep) = self.replay.as_mut() {
            rep.add_chat(from, flag, 0, message);
        }
```

In `reap_left_players`, inside the `for (pid, reason)` loop:

```rust
            if let Some(rep) = self.replay.as_mut() {
                rep.add_leaver(pid, 13, 0);
            }
```

- [ ] **Step 7: Save on game over**

In `crates/ghost-engine/src/actor.rs`, in the actor loop where `state.finished` is checked, before breaking:

```rust
    if let Some(rep) = state.replay.take() {
        let path = state.cfg.replay_path.clone();
        tokio::spawn(async move {
            if let Err(e) = ghost_spectator::save_replay(path, rep, 26, 6059, true).await {
                tracing::warn!(error = %e, "failed to save replay");
            }
        });
    }
```

Add `pub replay_path: std::path::PathBuf,` to `GameConfig` in `state.rs`, populate it in `supervisor.rs` from a new `[replay] path` config section (default `replays/`, filename `<sanitised game name>-<host_counter>.w3g`), and set `replay: Some(ReplayBody::new(virtual_host_pid_or_1, &cfg.virtual_host_name))` when the config enables recording.

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/ghost-engine crates/ghostrs
git commit -m "feat(engine): record games to w3g replays from the actor"
```

---

### Task 5: GProxy Buffer From First Packet

**Files:**
- Modify: `crates/ghost-engine/src/lobby.rs:44-97`, `crates/ghost-engine/src/actor.rs:113-131`
- Test: `crates/ghost-engine/src/gproxy.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `GProxyBuffer::new(capacity: usize)`.
- Produces: no new API; behaviour change only.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn packets_sent_before_the_first_ack_are_still_replayable() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
    st.begin_playing();
    st.players.by_pid_mut(1).unwrap().gproxy = true;

    // A tick happens before the client's first GPS_ACK ever arrives.
    st.on_tick(0);

    let buf = st.players.by_pid(1).unwrap().gproxy_buffer.as_ref()
        .expect("the buffer must exist from the moment the player joined");
    assert!(buf.total_sent() > 0, "early packets must be retained");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-engine packets_sent_before`
Expected: FAIL — `expect` panics, the buffer is `None`.

- [ ] **Step 3: Allocate at join**

In `crates/ghost-engine/src/lobby.rs`, after `player.reconnect_key = rand::random();`:

```rust
        // Allocate the replay ring immediately: a client can disconnect before
        // its first GPS_ACK, and everything sent until then must be recoverable.
        player.gproxy_buffer = Some(crate::gproxy::GProxyBuffer::new(GPROXY_BUFFER_PACKETS));
```

Add near the reject-code constants at the top of the file:

```rust
/// Packets retained per GProxy client. 500 ticks ≈ 50 s of game at 100 ms.
pub const GPROXY_BUFFER_PACKETS: usize = 500;
```

- [ ] **Step 4: Simplify the ACK handler**

In `crates/ghost-engine/src/actor.rs`, replace the lazy allocation at `:118-120` with a plain `p.gproxy = true;` — the buffer already exists.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-engine/src/lobby.rs crates/ghost-engine/src/actor.rs crates/ghost-engine/src/gproxy.rs
git commit -m "fix(engine): allocate the gproxy replay buffer at join, not at first ack"
```

---

### Task 6: DotaTV Protocol Codec (0xFD)

**Files:**
- Create: `crates/ghost-protocol/src/dtv/mod.rs`
- Modify: `crates/ghost-protocol/src/lib.rs`
- Test: `crates/ghost-protocol/src/dtv/mod.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `Frame`, `HeaderCodec`, `ProtoError`, `put_cstring`.
- Produces: `DTV_HEADER: u8 = 0xFD`, `DtvCodec = HeaderCodec<0xFD>`, `ids::{HELLO, PLAYERS, GAMEBLOCK, CHAT, GAMEOVER, VIEWER_CHAT}`, encoders `hello`, `players`, `gameblock`, `chat`, `gameover`, decoder `ViewerChat::decode`.

Wire format, identical framing to W3GS: `[0xFD][id][u16 LE total length including header][payload]`.

| id | Name | Direction | Payload |
|---|---|---|---|
| 0x01 | `HELLO` | relay → viewer | cstring game name, cstring map name, u8 num_slots, u32 delay_seconds |
| 0x02 | `PLAYERS` | relay → viewer | u8 count, then per player: u8 pid, u8 colour, u8 team, cstring name |
| 0x03 | `GAMEBLOCK` | relay → viewer | raw delayed W3GS `INCOMING_ACTION` packet, header included |
| 0x04 | `CHAT` | relay → viewer | cstring sender, cstring text |
| 0x05 | `GAMEOVER` | relay → viewer | u32 duration_seconds, u8 winner (0 none, 1 sentinel, 2 scourge) |
| 0x10 | `VIEWER_CHAT` | viewer → relay | cstring text |

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;

    #[test]
    fn every_message_is_framed_with_0xfd_and_a_correct_length() {
        let cases: Vec<(u8, Bytes)> = vec![
            (ids::HELLO, hello("dota -apem", "DotA v6.83d", 10, 300).unwrap()),
            (ids::PLAYERS, players(&[(1, 0, 0, "alice".into()), (2, 6, 1, "bob".into())]).unwrap()),
            (ids::GAMEBLOCK, gameblock(&Bytes::from_static(&[0xF7, 0x0C, 0x05, 0x00, 0x01])).unwrap()),
            (ids::CHAT, chat("alice", "gg").unwrap()),
            (ids::GAMEOVER, gameover(2410, 1)),
        ];
        for (id, p) in cases {
            assert_eq!(p[0], DTV_HEADER, "header byte for id {id:#04x}");
            assert_eq!(p[1], id);
            assert_eq!(u16::from_le_bytes([p[2], p[3]]) as usize, p.len(), "length for id {id:#04x}");
        }
    }

    #[test]
    fn a_players_message_round_trips() {
        let p = players(&[(1, 0, 0, "alice".into()), (11, 6, 1, "bob".into())]).unwrap();
        let mut buf = BytesMut::from(&p[..]);
        let f = DtvCodec::default().decode(&mut buf).unwrap().expect("frame");
        let list = PlayerList::decode(&f.payload).unwrap();
        assert_eq!(list.0.len(), 2);
        assert_eq!(list.0[1], (11, 6, 1, "bob".to_string()));
    }

    #[test]
    fn a_viewer_chat_message_round_trips() {
        let mut buf = BytesMut::from(&viewer_chat("hello relay").unwrap()[..]);
        let f = DtvCodec::default().decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, ids::VIEWER_CHAT);
        assert_eq!(ViewerChat::decode(&f.payload).unwrap().text, "hello relay");
    }

    #[test]
    fn a_truncated_message_is_buffered_rather_than_mis_decoded() {
        let full = chat("alice", "gg").unwrap();
        let mut buf = BytesMut::from(&full[..full.len() - 1]);
        assert!(DtvCodec::default().decode(&mut buf).unwrap().is_none());
        buf.extend_from_slice(&full[full.len() - 1..]);
        assert!(DtvCodec::default().decode(&mut buf).unwrap().is_some());
    }

    #[test]
    fn an_oversized_gameblock_is_rejected_rather_than_truncated() {
        let huge = Bytes::from(vec![0u8; 70_000]);
        assert!(matches!(gameblock(&huge), Err(ProtoError::TooLarge(_))));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghost-protocol dtv`
Expected: FAIL — `unresolved module dtv`.

- [ ] **Step 3: Implement the codec**

Create `crates/ghost-protocol/src/dtv/mod.rs`:

```rust
//! DotaTV spectator protocol. Same framing as W3GS with header byte 0xFD, so it
//! reuses `HeaderCodec` and cannot be confused with W3GS (0xF7), GPS (0xF8) or
//! BNCS (0xFF) on a shared port.
use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::bytes_ext::{BufExt, put_cstring};
use crate::error::ProtoError;
use crate::frame::{Frame, HeaderCodec};

pub const DTV_HEADER: u8 = 0xFD;
pub type DtvCodec = HeaderCodec<DTV_HEADER>;

pub mod ids {
    pub const HELLO: u8 = 0x01;
    pub const PLAYERS: u8 = 0x02;
    pub const GAMEBLOCK: u8 = 0x03;
    pub const CHAT: u8 = 0x04;
    pub const GAMEOVER: u8 = 0x05;
    pub const VIEWER_CHAT: u8 = 0x10;
}

pub fn hello(game_name: &str, map_name: &str, num_slots: u8, delay_seconds: u32) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::new();
    put_cstring(&mut p, game_name);
    put_cstring(&mut p, map_name);
    p.put_u8(num_slots);
    p.put_u32_le(delay_seconds);
    Frame::new(ids::HELLO, p.freeze()).encode_with(DTV_HEADER)
}

/// `(pid, colour, team, name)` for every seated player.
pub fn players(list: &[(u8, u8, u8, String)]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::new();
    p.put_u8(list.len() as u8);
    for (pid, colour, team, name) in list {
        p.put_u8(*pid);
        p.put_u8(*colour);
        p.put_u8(*team);
        put_cstring(&mut p, name);
    }
    Frame::new(ids::PLAYERS, p.freeze()).encode_with(DTV_HEADER)
}

/// Wraps an already-encoded W3GS packet for delayed delivery.
pub fn gameblock(w3gs_packet: &Bytes) -> Result<Bytes, ProtoError> {
    Frame::new(ids::GAMEBLOCK, w3gs_packet.clone()).encode_with(DTV_HEADER)
}

pub fn chat(sender: &str, text: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::new();
    put_cstring(&mut p, sender);
    put_cstring(&mut p, text);
    Frame::new(ids::CHAT, p.freeze()).encode_with(DTV_HEADER)
}

pub fn gameover(duration_seconds: u32, winner: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u32_le(duration_seconds);
    p.put_u8(winner);
    Frame::new(ids::GAMEOVER, p.freeze())
        .encode_with(DTV_HEADER)
        .expect("5-byte payload always fits")
}

pub fn viewer_chat(text: &str) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::new();
    put_cstring(&mut p, text);
    Frame::new(ids::VIEWER_CHAT, p.freeze()).encode_with(DTV_HEADER)
}

#[derive(Debug, PartialEq, Eq)]
pub struct PlayerList(pub Vec<(u8, u8, u8, String)>);

impl PlayerList {
    pub fn decode(mut src: &Bytes) -> Result<Self, ProtoError> {
        let mut b = src.clone();
        let n = b.try_get_u8()?;
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let pid = b.try_get_u8()?;
            let colour = b.try_get_u8()?;
            let team = b.try_get_u8()?;
            let name = b.try_get_cstring()?;
            out.push((pid, colour, team, name));
        }
        let _ = &mut src;
        Ok(Self(out))
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ViewerChat {
    pub text: String,
}

impl ViewerChat {
    pub fn decode(src: &Bytes) -> Result<Self, ProtoError> {
        let mut b = src.clone();
        Ok(Self { text: b.try_get_cstring()? })
    }
}
```

If `BufExt` does not already provide `try_get_u8` / `try_get_cstring`, add them there rather than duplicating bounds checks — `crates/ghost-protocol/src/bytes_ext.rs` is the single place that owns "read past the end is an error, not a panic".

Add `pub mod dtv;` to `crates/ghost-protocol/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghost-protocol dtv`
Expected: PASS (5 tests).

- [ ] **Step 5: Add a fuzz guard**

Add to the same test module:

```rust
    proptest::proptest! {
        #[test]
        fn the_decoder_never_panics_on_arbitrary_bytes(data: Vec<u8>) {
            let mut buf = BytesMut::from(&data[..]);
            let mut codec = DtvCodec::default();
            for _ in 0..8 {
                match codec.decode(&mut buf) {
                    Ok(Some(f)) => {
                        let _ = PlayerList::decode(&f.payload);
                        let _ = ViewerChat::decode(&f.payload);
                    }
                    Ok(None) | Err(_) => break,
                }
            }
        }
    }
```

- [ ] **Step 6: Run it**

Run: `cargo test -p ghost-protocol dtv`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-protocol/src/dtv crates/ghost-protocol/src/lib.rs crates/ghost-protocol/src/bytes_ext.rs
git commit -m "feat(protocol): add the DotaTV 0xFD spectator codec"
```

---

### Task 7: Relay Speaks DotaTV

**Files:**
- Create: `crates/ghost-spectator/src/conn.rs`
- Modify: `crates/ghost-spectator/src/relay.rs`, `crates/ghost-spectator/src/lib.rs`
- Test: `crates/ghost-spectator/src/relay.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `ghost_protocol::dtv` (Task 6), `ghost_net::{PlayerLink, ConnEvent}`.
- Produces: `spawn_dtv_conn(conn_id, stream, tx, cap) -> PlayerLink`, `DtvEvent`, `RelayCmd::{Hello, Players, GameBlock, ViewerChat, GameOver}`, `RelayHandle::{push_block, set_players, send_chat, game_over}`.

- [ ] **Step 1: Write the failing tests**

```rust
    #[tokio::test]
    async fn a_new_viewer_is_greeted_with_hello_and_the_player_list() {
        let cfg = RelayConfig {
            port: 0,
            delay: Duration::from_secs(300),
            max_viewers: 8,
            game_name: "dota -apem".into(),
            map_name: "DotA v6.83d".into(),
            num_slots: 10,
        };
        let mut relay = Relay::new(cfg);
        relay.set_players(vec![(1, 0, 0, "alice".into())]);

        let (tx, mut rx) = mpsc::channel(64);
        relay.add_viewer(1, PlayerLink::for_test(tx)).unwrap();

        let first = rx.try_recv().expect("HELLO");
        assert_eq!(first[0], ghost_protocol::dtv::DTV_HEADER);
        assert_eq!(first[1], ghost_protocol::dtv::ids::HELLO);
        let second = rx.try_recv().expect("PLAYERS");
        assert_eq!(second[1], ghost_protocol::dtv::ids::PLAYERS);
    }

    #[tokio::test(start_paused = true)]
    async fn game_blocks_reach_viewers_wrapped_in_a_dtv_frame_after_the_delay() {
        let (handle, _join) = spawn_relay(test_cfg(Duration::from_secs(120)));
        let (tx, mut rx) = mpsc::channel(64);
        handle.attach_viewer(1, PlayerLink::for_test(tx));
        tokio::task::yield_now().await;
        let _ = rx.try_recv(); // HELLO
        let _ = rx.try_recv(); // PLAYERS

        handle.push_block(Bytes::from_static(&[0xF7, 0x0C, 0x05, 0x00, 0x01]));
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(rx.try_recv().is_err(), "must still be held back");

        tokio::time::advance(Duration::from_secs(61)).await;
        tokio::task::yield_now().await;
        let got = rx.try_recv().expect("delayed block");
        assert_eq!(got[1], ghost_protocol::dtv::ids::GAMEBLOCK);
        assert_eq!(&got[4..], &[0xF7, 0x0C, 0x05, 0x00, 0x01]);
    }

    #[tokio::test]
    async fn the_delay_queue_is_bounded_and_drops_the_oldest_blocks() {
        let mut cfg = test_cfg(Duration::from_secs(3600));
        cfg.max_queued_blocks = 4;
        let mut relay = Relay::new(cfg);
        for i in 0..10u8 {
            relay.enqueue(Bytes::from(vec![i]));
        }
        assert_eq!(relay.delayed_blocks.len(), 4, "queue must not grow without bound");
        assert_eq!(relay.dropped_blocks, 6);
    }

    #[tokio::test]
    async fn a_viewer_that_stops_reading_is_dropped_not_silently_starved() {
        let mut relay = Relay::new(test_cfg(Duration::ZERO));
        let (tx, rx) = mpsc::channel(1);
        relay.add_viewer(1, PlayerLink::for_test(tx)).unwrap();
        drop(rx);
        relay.broadcast(&Bytes::from_static(&[1, 2, 3]));
        assert!(relay.viewers.is_empty(), "closed viewers must be reaped");
    }
```

Add a `fn test_cfg(delay: Duration) -> RelayConfig` helper to the test module.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ghost-spectator relay`
Expected: FAIL — missing `map_name`, `num_slots`, `max_queued_blocks`, `set_players`, `enqueue`, `attach_viewer`, `push_block`, `dropped_blocks`.

- [ ] **Step 3: Extend the config and state**

In `crates/ghost-spectator/src/relay.rs`:

```rust
#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub port: u16,
    pub delay: Duration,
    pub max_viewers: usize,
    pub game_name: String,
    pub map_name: String,
    pub num_slots: u8,
    /// Hard cap on the delay buffer. At 100 ms per block a 10-minute delay needs
    /// 6 000; the cap keeps a stalled relay from growing without bound.
    pub max_queued_blocks: usize,
}
```

```rust
pub struct Relay {
    pub cfg: RelayConfig,
    pub viewers: Vec<(u64, PlayerLink)>,
    pub delayed_blocks: VecDeque<(Instant, Bytes)>,
    pub released_count: usize,
    pub dropped_blocks: usize,
    pub players: Vec<(u8, u8, u8, String)>,
    pub started_at: Instant,
}
```

- [ ] **Step 4: Implement greeting, bounded enqueue and reaping**

```rust
impl Relay {
    pub fn set_players(&mut self, list: Vec<(u8, u8, u8, String)>) {
        self.players = list;
        if let Ok(pkt) = dtv::players(&self.players) {
            self.broadcast(&pkt);
        }
    }

    pub fn add_viewer(&mut self, conn_id: u64, link: PlayerLink) -> Result<(), RelayError> {
        if self.viewers.len() >= self.cfg.max_viewers {
            return Err(RelayError::Full);
        }
        if let Ok(pkt) = dtv::hello(
            &self.cfg.game_name,
            &self.cfg.map_name,
            self.cfg.num_slots,
            self.cfg.delay.as_secs() as u32,
        ) {
            let _ = link.try_send(pkt);
        }
        if let Ok(pkt) = dtv::players(&self.players) {
            let _ = link.try_send(pkt);
        }
        self.viewers.push((conn_id, link));
        Ok(())
    }

    /// Queues one already-encoded W3GS packet for delayed delivery.
    pub fn enqueue(&mut self, block: Bytes) {
        while self.delayed_blocks.len() >= self.cfg.max_queued_blocks {
            self.delayed_blocks.pop_front();
            self.dropped_blocks += 1;
        }
        self.delayed_blocks.push_back((Instant::now() + self.cfg.delay, block));
    }

    pub fn release_due_blocks(&mut self) {
        let now = Instant::now();
        while self.delayed_blocks.front().is_some_and(|&(at, _)| at <= now) {
            let (_, block) = self.delayed_blocks.pop_front().expect("checked above");
            if let Ok(pkt) = dtv::gameblock(&block) {
                self.broadcast(&pkt);
            }
            self.released_count += 1;
        }
    }

    pub fn broadcast(&mut self, bytes: &Bytes) {
        self.viewers.retain(|(id, link)| match link.try_send(bytes.clone()) {
            Ok(()) => true,
            Err(e) => {
                tracing::info!(conn_id = id, error = %e, "dropping spectator viewer");
                false
            }
        });
    }
}
```

- [ ] **Step 5: Add the DTV connection task**

Create `crates/ghost-spectator/src/conn.rs`:

```rust
//! One viewer socket, framed with the DotaTV codec. Mirrors `ghost_net::spawn_conn`
//! but speaks 0xFD instead of the W3GS/GPS dual codec — a viewer that sent 0xFD
//! bytes to `spawn_conn` would have them silently resynced away.
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use ghost_net::PlayerLink;
use ghost_protocol::dtv::DtvCodec;
use ghost_protocol::frame::Frame;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, FramedWrite};

#[derive(Debug)]
pub enum DtvEvent {
    Frame { conn_id: u64, frame: Frame },
    Closed { conn_id: u64 },
}

pub fn spawn_dtv_conn(
    conn_id: u64,
    stream: TcpStream,
    events: mpsc::Sender<DtvEvent>,
    write_capacity: usize,
) -> PlayerLink {
    let _ = stream.set_nodelay(true);
    let (read_half, write_half) = stream.into_split();
    let (tx, mut rx) = mpsc::channel::<Bytes>(write_capacity);

    tokio::spawn(async move {
        let mut w = FramedWrite::new(write_half, DtvCodec::default());
        while let Some(bytes) = rx.recv().await {
            if w.send(bytes).await.is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        let mut r = FramedRead::new(read_half, DtvCodec::default());
        while let Some(item) = r.next().await {
            match item {
                Ok(frame) => {
                    if events.send(DtvEvent::Frame { conn_id, frame }).await.is_err() {
                        break;
                    }
                }
                Err(e) => {
                    tracing::debug!(conn_id, error = %e, "viewer protocol error");
                    break;
                }
            }
        }
        let _ = events.send(DtvEvent::Closed { conn_id }).await;
    });

    PlayerLink::for_test(tx)
}
```

Add `pub mod conn;` and `pub use conn::{DtvEvent, spawn_dtv_conn};` to `crates/ghost-spectator/src/lib.rs`.

- [ ] **Step 6: Rewrite `run_relay` to handle both directions**

Replace the listener block in `spawn_relay` so viewers are attached with `spawn_dtv_conn` and their events are forwarded into the same `RelayCmd` channel; replace the discard loop at `relay.rs:121-123`:

```rust
                let (conn_tx, mut conn_rx) = mpsc::channel::<DtvEvent>(256);
                let ev_tx = tx_clone.clone();
                tokio::spawn(async move {
                    while let Some(ev) = conn_rx.recv().await {
                        let cmd = match ev {
                            DtvEvent::Frame { conn_id, frame } if frame.id == dtv::ids::VIEWER_CHAT => {
                                match dtv::ViewerChat::decode(&frame.payload) {
                                    Ok(c) => RelayCmd::ViewerChat { conn_id, text: c.text },
                                    Err(_) => continue,
                                }
                            }
                            DtvEvent::Frame { .. } => continue,
                            DtvEvent::Closed { conn_id } => RelayCmd::ViewerLeft { conn_id },
                        };
                        if ev_tx.send(cmd).await.is_err() {
                            break;
                        }
                    }
                });
```

Extend `RelayCmd` with `ViewerLeft { conn_id: u64 }`, change `ViewerChat` to carry `conn_id`, add `SetPlayers(Vec<(u8, u8, u8, String)>)`, and handle each in `run_relay`. `ViewerChat` must rebroadcast as `dtv::chat(sender, text)` where `sender` is the viewer's name (use `"viewer"` plus the conn id until the client sends a name). `GameOver` sends `dtv::gameover(elapsed_secs, winner)` after flushing.

- [ ] **Step 7: Add the handle methods**

```rust
impl RelayHandle {
    pub fn push_block(&self, block: Bytes) {
        let _ = self.tx.try_send(RelayCmd::GameBlock(block));
    }
    pub fn set_players(&self, list: Vec<(u8, u8, u8, String)>) {
        let _ = self.tx.try_send(RelayCmd::SetPlayers(list));
    }
    pub fn attach_viewer(&self, conn_id: u64, link: PlayerLink) {
        let _ = self.tx.try_send(RelayCmd::ViewerJoined { conn_id, link });
    }
    pub fn game_over(&self, duration_seconds: u32, winner: u8) {
        let _ = self.tx.try_send(RelayCmd::GameOver { duration_seconds, winner });
    }
}
```

Keep the existing `push` as a deprecated alias or delete it and update callers.

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p ghost-spectator`
Expected: PASS.

- [ ] **Step 9: Fix the supervisor call site**

`crates/ghostrs/src/supervisor.rs:79` constructs `RelayConfig`; add `map_name`, `num_slots`, `max_queued_blocks` (default `6000`) from the loaded map and config.

- [ ] **Step 10: Run the workspace tests and commit**

Run: `cargo test --workspace`
Expected: PASS.

```bash
git add crates/ghost-spectator crates/ghostrs/src/supervisor.rs
git commit -m "feat(spectator): relay speaks the DotaTV protocol in both directions"
```

---

### Task 8: Engine Feeds the Relay

**Files:**
- Modify: `crates/ghost-engine/src/actions.rs`, `crates/ghost-engine/src/state.rs`, `crates/ghost-engine/src/lobby.rs`
- Test: `crates/ghost-engine/src/actions.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `RelayHandle::{push_block, set_players, game_over}` (Task 7).
- Produces: no new API.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test(start_paused = true)]
async fn action_packets_are_forwarded_to_the_spectator_relay() {
    let (handle, _join) = ghost_spectator::spawn_relay(ghost_spectator::RelayConfig {
        port: 0,
        delay: std::time::Duration::ZERO,
        max_viewers: 4,
        game_name: "t".into(),
        map_name: "m".into(),
        num_slots: 10,
        max_queued_blocks: 100,
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    handle.attach_viewer(1, ghost_net::PlayerLink::for_test(tx));
    tokio::task::yield_now().await;
    while rx.try_recv().is_ok() {} // drain HELLO + PLAYERS

    let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
    st.relay = Some(handle);
    st.begin_playing();
    st.on_tick(0);
    tokio::task::yield_now().await;

    let got = rx.try_recv().expect("the relay must receive the tick's actions");
    assert_eq!(got[1], ghost_protocol::dtv::ids::GAMEBLOCK);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-engine forwarded_to_the_spectator_relay`
Expected: FAIL — no block arrives; `GameState` never pushes.

- [ ] **Step 3: Push blocks from the tick**

In `crates/ghost-engine/src/actions.rs`, in `send_all_actions`, right beside the replay recording added in Task 4:

```rust
        if let Some(relay) = &self.relay {
            // Refcount bump, not a copy: the same packet goes to players,
            // the replay and the relay.
            relay.push_block(packet.clone());
        }
```

- [ ] **Step 4: Push the player list**

In `GameState::begin_playing`, after the replay `set_start`:

```rust
        if let Some(relay) = &self.relay {
            let list: Vec<(u8, u8, u8, String)> = self
                .players
                .iter()
                .filter(|p| !p.virtual_host)
                .map(|p| {
                    let (colour, team) = self.slots.colour_and_team(p.pid).unwrap_or((0, 0));
                    (p.pid, colour, team, p.name.clone())
                })
                .collect();
            relay.set_players(list);
        }
```

Add `SlotTable::colour_and_team(&self, pid: u8) -> Option<(u8, u8)>` to `crates/ghost-engine/src/slots.rs`, reading the colour and team bytes of the slot that holds `pid`.

- [ ] **Step 5: Signal game over**

In `GameState::on_tick`, where `self.phase = GamePhase::Over` is set and in the `GamePhase::Over` arm:

```rust
            GamePhase::Over => {
                if !self.finished
                    && let Some(relay) = &self.relay
                {
                    let secs = self.created_at.elapsed().as_secs() as u32;
                    let winner = self.dota.as_ref().map(|d| d.winner).unwrap_or(0);
                    relay.game_over(secs, winner);
                }
                self.finished = true;
            }
```

Add `pub winner: u8` to `StatsDotA` in `crates/ghost-engine/src/stats_dota.rs` if it is not already exposed.

- [ ] **Step 6: Also relay chat**

In `send_chat_all`, after building the packet:

```rust
        if let Some(relay) = &self.relay {
            relay.send_chat(&self.cfg.virtual_host_name, message);
        }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): stream action blocks, players and chat to the spectator relay"
```

---

### Task 9: DotaTV C++ Client Framing

**Files:**
- Modify: `dotatv_client/include/NetClient.hpp`, `dotatv_client/src/NetClient.cpp`
- Modify: `dotatv_client/src/DotaTV.cpp`
- Create: `dotatv_client/include/DtvProtocol.hpp`

**Interfaces:**
- Consumes: the wire format defined in Task 6.
- Produces: `NetClient::SetMessageCallback(std::function<void(uint8_t id, const std::vector<uint8_t>& payload)>)`, `NetClient::SendViewerChat(const std::string&)`.

The current client sends bare text with `\n` (`NetClient.cpp:53-58`) and hands one raw `recv()` buffer to its callback (`:61-74`). TCP gives no message boundaries, so this breaks the moment two messages share a segment.

- [ ] **Step 1: Define the protocol header**

Create `dotatv_client/include/DtvProtocol.hpp`:

```cpp
#pragma once
#include <cstdint>

namespace DotaTV {
    constexpr uint8_t DTV_HEADER = 0xFD;

    enum DtvId : uint8_t {
        DTV_HELLO       = 0x01,
        DTV_PLAYERS     = 0x02,
        DTV_GAMEBLOCK   = 0x03,
        DTV_CHAT        = 0x04,
        DTV_GAMEOVER    = 0x05,
        DTV_VIEWER_CHAT = 0x10,
    };

    // [header][id][uint16 LE total length including the 4-byte header][payload]
    constexpr size_t DTV_HEADER_LEN = 4;
}
```

- [ ] **Step 2: Replace the receive loop with a framing accumulator**

In `dotatv_client/src/NetClient.cpp`, replace `ReceiveLoop`:

```cpp
    void NetClient::ReceiveLoop() {
        std::vector<uint8_t> acc;
        std::vector<uint8_t> buffer(16384);

        while (m_running && m_connected) {
            int bytes = recv(m_socket, (char*)buffer.data(), (int)buffer.size(), 0);
            if (bytes <= 0) {
                m_connected = false;
                break;
            }
            acc.insert(acc.end(), buffer.begin(), buffer.begin() + bytes);

            // Drain every complete frame the accumulator now holds.
            size_t pos = 0;
            while (acc.size() - pos >= DTV_HEADER_LEN) {
                if (acc[pos] != DTV_HEADER) {
                    // Resync: skip one byte and look for the next header.
                    ++pos;
                    continue;
                }
                uint8_t  id    = acc[pos + 1];
                uint16_t total = (uint16_t)(acc[pos + 2] | (acc[pos + 3] << 8));
                if (total < DTV_HEADER_LEN) {
                    ++pos;               // malformed length; resync
                    continue;
                }
                if (acc.size() - pos < total) {
                    break;               // wait for the rest of this frame
                }
                std::vector<uint8_t> payload(acc.begin() + pos + DTV_HEADER_LEN,
                                             acc.begin() + pos + total);
                if (m_messageCallback) {
                    m_messageCallback(id, payload);
                }
                pos += total;
            }
            acc.erase(acc.begin(), acc.begin() + pos);
        }
    }
```

- [ ] **Step 3: Replace `SendChat` with a framed send**

```cpp
    bool NetClient::SendViewerChat(const std::string& message) {
        if (!m_connected || m_socket == INVALID_SOCKET) return false;

        // [0xFD][0x10][len][cstring text]
        uint16_t total = (uint16_t)(DTV_HEADER_LEN + message.size() + 1);
        std::vector<uint8_t> frame;
        frame.reserve(total);
        frame.push_back(DTV_HEADER);
        frame.push_back(DTV_VIEWER_CHAT);
        frame.push_back((uint8_t)(total & 0xFF));
        frame.push_back((uint8_t)(total >> 8));
        frame.insert(frame.end(), message.begin(), message.end());
        frame.push_back(0);

        // send() may return short; loop until the whole frame is on the wire.
        size_t off = 0;
        while (off < frame.size()) {
            int sent = send(m_socket, (const char*)frame.data() + off, (int)(frame.size() - off), 0);
            if (sent <= 0) {
                m_connected = false;
                return false;
            }
            off += (size_t)sent;
        }
        return true;
    }
```

Update `NetClient.hpp`: replace `PacketCallback`/`SetPacketCallback`/`SendChat` with

```cpp
        typedef std::function<void(uint8_t id, const std::vector<uint8_t>& payload)> MessageCallback;
        // ...
        bool SendViewerChat(const std::string& message);
        void SetMessageCallback(MessageCallback cb) { m_messageCallback = cb; }
```

and the member `MessageCallback m_messageCallback;`. Add `#include "DtvProtocol.hpp"`.

- [ ] **Step 4: Handle the messages**

In `dotatv_client/src/DotaTV.cpp`, after `NetClient::Instance().Connect(...)` succeeds, register:

```cpp
    NetClient::Instance().SetMessageCallback([](uint8_t id, const std::vector<uint8_t>& p) {
        switch (id) {
            case DTV_HELLO:     DotaTV::OnHello(p);     break;
            case DTV_PLAYERS:   DotaTV::OnPlayers(p);   break;
            case DTV_GAMEBLOCK: DotaTV::OnGameBlock(p); break;
            case DTV_CHAT:      DotaTV::OnChat(p);      break;
            case DTV_GAMEOVER:  DotaTV::OnGameOver(p);  break;
            default: break;
        }
    });
```

Implement `OnHello` (store game/map/slots/delay and show them in `SpectatorHUD`), `OnPlayers` (populate the player list the HUD and `CameraManager` iterate over), `OnChat` (append to the HUD chat log), `OnGameOver` (show the result and disconnect). `OnGameBlock` receives a complete W3GS `INCOMING_ACTION` packet — feed it to whatever consumes game state; until that consumer exists, log its length so the transport can be verified independently of the game integration.

- [ ] **Step 5: Update the other call sites**

`dotatv_client/src/ConnectDialog.cpp` and `dotatv_client/src/MainMenuUI.cpp` call `Connect(host, port)` only; no change needed. Grep for `SendChat(` and `SetPacketCallback(` across `dotatv_client/src` and update every hit.

- [ ] **Step 6: Build the client**

Run: `msbuild dotatv_client\dotatv_client.vcxproj /p:Configuration=Release`
Expected: build succeeds with no unresolved externals.

If MSBuild is unavailable in this environment, state that explicitly in the commit message and verify only that the header and source compile in isolation.

- [ ] **Step 7: Commit**

```bash
git add dotatv_client/include dotatv_client/src
git commit -m "feat(dotatv): frame the client transport with the DotaTV 0xFD protocol"
```

---

### Task 10: Lobby and Slot Commands

**Files:**
- Create: `crates/ghost-engine/src/commands/mod.rs`, `crates/ghost-engine/src/commands/comp.rs`
- Modify: `crates/ghost-engine/src/chat.rs:40-115`, `crates/ghost-engine/src/slots.rs`, `crates/ghost-engine/src/lib.rs`
- Test: `crates/ghost-engine/src/commands/comp.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `SlotTable`, `ChatCommand`.
- Produces: `ChatCommand::{OpenAll, CloseAll, Comp(u8), CompColour(u8, u8), CompHandicap(u8, u8), CompRace(u8, String), CompTeam(u8, u8), Lock, Unlock, From, Check(String), CheckMe}`; `SlotTable::{open_all, close_all, add_computer, set_colour, set_handicap, set_race, set_team}`.

GHost++ reference: `game.cpp:639-899` (close/closeall/comp/compcolour/comphandicap/comprace/compteam), `:1137-1147` (lock), `:1221` (openall), `:993` (from), `:573` (check), `:1684` (checkme), `:1580` (unlock).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{ChatCommand, parse_command};

    #[test]
    fn the_computer_commands_parse_with_their_arguments() {
        assert_eq!(parse_command("!comp 3"), Some(ChatCommand::Comp(3)));
        assert_eq!(parse_command("!comp 3 2"), Some(ChatCommand::CompSkill(3, 2)));
        assert_eq!(parse_command("!compcolour 3 5"), Some(ChatCommand::CompColour(3, 5)));
        assert_eq!(parse_command("!comphandicap 3 75"), Some(ChatCommand::CompHandicap(3, 75)));
        assert_eq!(parse_command("!comprace 3 human"), Some(ChatCommand::CompRace(3, "human".into())));
        assert_eq!(parse_command("!compteam 3 2"), Some(ChatCommand::CompTeam(3, 2)));
        assert_eq!(parse_command("!openall"), Some(ChatCommand::OpenAll));
        assert_eq!(parse_command("!closeall"), Some(ChatCommand::CloseAll));
        assert_eq!(parse_command("!lock"), Some(ChatCommand::Lock));
        assert_eq!(parse_command("!unlock"), Some(ChatCommand::Unlock));
    }

    #[test]
    fn out_of_range_slots_and_colours_are_refused_at_parse_time() {
        assert_eq!(parse_command("!comp 0"), None, "slots are 1-based");
        assert_eq!(parse_command("!comp 13"), None);
        assert_eq!(parse_command("!compcolour 3 12"), None, "colours are 0..11");
        assert_eq!(parse_command("!comphandicap 3 42"), None, "handicap is 50/60/70/80/90/100");
    }

    #[test]
    fn adding_a_computer_occupies_the_slot_with_the_requested_skill() {
        let mut slots = SlotTable::for_test(12);
        slots.add_computer(2, 1).unwrap();
        let wire = slots.as_wire();
        // Slot layout: [pid, download, status, computer, team, colour, race, skill, handicap]
        assert_eq!(wire[2 * 9 + 2], 2, "status must be occupied");
        assert_eq!(wire[2 * 9 + 3], 1, "computer flag");
        assert_eq!(wire[2 * 9 + 7], 1, "skill");
    }

    #[test]
    fn close_all_only_touches_open_slots() {
        let mut slots = SlotTable::for_test(4);
        slots.occupy_slot(1, 7);
        slots.close_all();
        let wire = slots.as_wire();
        assert_eq!(wire[1 * 9], 7, "an occupied slot keeps its player");
        assert_eq!(wire[0 * 9 + 2], 1, "an open slot becomes closed");
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-engine commands::comp`
Expected: FAIL — unresolved module and variants.

- [ ] **Step 3: Extend `ChatCommand`**

Add to the enum in `crates/ghost-engine/src/chat.rs`:

```rust
    OpenAll,
    CloseAll,
    Comp(u8),
    CompSkill(u8, u8),
    CompColour(u8, u8),
    CompHandicap(u8, u8),
    CompRace(u8, String),
    CompTeam(u8, u8),
    Lock,
    Unlock,
    From,
    Check(String),
    CheckMe,
```

- [ ] **Step 4: Extend the parser**

Add to the `match` in `parse_command`, following the existing `slot_arg` idiom:

```rust
        "openall" => ChatCommand::OpenAll,
        "closeall" => ChatCommand::CloseAll,
        "comp" => {
            let slot = slot_arg(args.first()?)?;
            match args.get(1) {
                Some(s) => ChatCommand::CompSkill(slot, s.parse::<u8>().ok().filter(|v| *v <= 2)?),
                None => ChatCommand::Comp(slot),
            }
        }
        "compcolour" | "compcolor" => ChatCommand::CompColour(
            slot_arg(args.first()?)?,
            args.get(1)?.parse::<u8>().ok().filter(|v| *v <= 11)?,
        ),
        "comphandicap" => ChatCommand::CompHandicap(
            slot_arg(args.first()?)?,
            args.get(1)?.parse::<u8>().ok().filter(|v| matches!(v, 50 | 60 | 70 | 80 | 90 | 100))?,
        ),
        "comprace" => ChatCommand::CompRace(slot_arg(args.first()?)?, args.get(1)?.to_lowercase()),
        "compteam" => ChatCommand::CompTeam(
            slot_arg(args.first()?)?,
            args.get(1)?.parse::<u8>().ok().filter(|v| *v >= 1 && *v <= 12)?,
        ),
        "lock" => ChatCommand::Lock,
        "unlock" => ChatCommand::Unlock,
        "from" | "f" => ChatCommand::From,
        "check" => ChatCommand::Check(args.first()?.to_string()),
        "checkme" => ChatCommand::CheckMe,
```

`slot_arg` already exists and must reject anything outside `1..=12`, returning a 0-based index.

- [ ] **Step 5: Implement the slot operations**

Create `crates/ghost-engine/src/commands/comp.rs` with `impl SlotTable` methods (or add them to `slots.rs` and keep `comp.rs` for the command handlers — pick one and be consistent):

```rust
impl SlotTable {
    pub fn open_all(&mut self) {
        for s in self.slots_mut() {
            if s.status == SLOT_CLOSED {
                s.status = SLOT_OPEN;
            }
        }
    }

    pub fn close_all(&mut self) {
        for s in self.slots_mut() {
            if s.status == SLOT_OPEN {
                s.status = SLOT_CLOSED;
            }
        }
    }

    /// `skill`: 0 easy, 1 normal, 2 insane. Refuses an already-taken slot.
    pub fn add_computer(&mut self, slot: u8, skill: u8) -> Result<(), SlotError> {
        let s = self.slot_mut(slot).ok_or(SlotError::NoSuchSlot)?;
        if s.status == SLOT_OCCUPIED && s.computer == 0 {
            return Err(SlotError::Occupied);
        }
        s.pid = 0;
        s.status = SLOT_OCCUPIED;
        s.computer = 1;
        s.computer_type = skill;
        s.download_status = 100;
        Ok(())
    }

    pub fn set_colour(&mut self, slot: u8, colour: u8) -> Result<(), SlotError> {
        // A colour already in use must be swapped, never duplicated, or the
        // client renders two players in one colour (game.cpp:701).
        if let Some(other) = self.slot_index_with_colour(colour) {
            let old = self.slot(slot).ok_or(SlotError::NoSuchSlot)?.colour;
            self.slot_mut(other).expect("index came from the table").colour = old;
        }
        self.slot_mut(slot).ok_or(SlotError::NoSuchSlot)?.colour = colour;
        Ok(())
    }

    pub fn set_handicap(&mut self, slot: u8, handicap: u8) -> Result<(), SlotError> {
        self.slot_mut(slot).ok_or(SlotError::NoSuchSlot)?.handicap = handicap;
        Ok(())
    }

    pub fn set_race(&mut self, slot: u8, race: &str) -> Result<(), SlotError> {
        // Race values from gameslot.h; SLOTRACE_SELECTABLE (0x40) stays set.
        let bits = match race {
            "human" => 0x01,
            "orc" => 0x02,
            "night elf" | "nightelf" | "elf" => 0x04,
            "undead" => 0x08,
            "random" => 0x20,
            _ => return Err(SlotError::BadRace),
        };
        let s = self.slot_mut(slot).ok_or(SlotError::NoSuchSlot)?;
        s.race = bits | 0x40;
        Ok(())
    }

    pub fn set_team(&mut self, slot: u8, team: u8) -> Result<(), SlotError> {
        self.slot_mut(slot).ok_or(SlotError::NoSuchSlot)?.team = team.saturating_sub(1);
        Ok(())
    }
}
```

Add `SlotError` (`NoSuchSlot`, `Occupied`, `BadRace`) as a `thiserror::Error`, and `SlotTable::for_test(n: usize)` used by the tests. Reuse the existing slot-byte field names; if `SlotTable` currently stores raw bytes rather than a struct, add the accessors rather than reshaping the type.

- [ ] **Step 6: Dispatch the commands**

In `crates/ghost-engine/src/actor.rs`'s command handler, add arms for each new variant. Each one applies the change, calls `self.send_all_slot_info()` and replies with `self.send_chat_all(...)`. `Lock`/`Unlock` set a new `GameState::locked: bool` that makes every owner-only command refuse non-owners. `From` replies with each player's country (from a GeoIP lookup if configured, otherwise `"??"`). `Check`/`CheckMe` report ping, from, admin, spoofed and reserved status.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): add the lobby, computer-slot and lock commands"
```

---

### Task 11: Game-Flow and Vote Commands

**Files:**
- Create: `crates/ghost-engine/src/commands/vote.rs`
- Modify: `crates/ghost-engine/src/chat.rs`, `crates/ghost-engine/src/actor.rs`, `crates/ghost-engine/src/state.rs`
- Test: `crates/ghost-engine/src/commands/vote.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `ChatCommand::{End, Announce(u32, String), AutoStart(usize), Messages(bool), VoteKick(String), VoteCancel, Yes, ClearHcl, Download(String), FakePlayer, FpPause, FpResume, Refresh(bool), SendLan, VirtualHost(String), Priv(String), Pub(String)}`; `VoteKick` state on `GameState`.

GHost++ reference: `game.cpp:1742-1800` (votekick/yes), `:1630` (votecancel), `:482` (announce), `:543` (autostart), `:1147` (messages), `:947` (end), `:957-993` (fakeplayer/fppause/fpresume), `:629` (clearhcl), `:907` (download), `:1392` (refresh), `:1422` (sendlan), `:1620` (virtualhost), `:1314-1392` (priv/pub).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn a_votekick_needs_seventy_percent_of_the_other_players() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(4);
        st.start_votekick(1, "p1"); // pid 2 is the target; pids 1,3,4 may vote
        assert!(st.votekick.is_some());
        st.cast_votekick(3);
        assert!(st.votekick.is_some(), "2 of 3 is 66%, below the 70% threshold");
        st.cast_votekick(4);
        assert!(
            st.players.by_pid(2).is_none_or(|p| p.left.is_some()),
            "3 of 3 passes and the target is kicked"
        );
    }

    #[tokio::test]
    async fn a_votekick_expires_after_sixty_seconds() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(4);
        st.start_votekick(1, "p1");
        st.expire_votekick(Duration::from_secs(61));
        assert!(st.votekick.is_none(), "a stale vote must not linger");
    }

    #[tokio::test]
    async fn a_second_votekick_cannot_start_while_one_is_running() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(4);
        assert!(st.start_votekick(1, "p1"));
        assert!(!st.start_votekick(3, "p2"), "only one vote at a time");
    }

    #[tokio::test]
    async fn autostart_begins_the_countdown_at_the_configured_headcount() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(0);
        st.autostart_players = Some(2);
        for i in 0..2u64 {
            let (tx, _rx) = tokio::sync::mpsc::channel(64);
            st.add_conn(10 + i, ghost_net::PlayerLink::for_test(tx), [0; 4]);
            st.handle_req_join(10 + i, &crate::actor::tests_support::reqjoin_bytes(&format!("p{i}")));
        }
        assert!(matches!(st.phase, crate::state::GamePhase::Countdown { .. }));
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-engine commands::vote`
Expected: FAIL.

- [ ] **Step 3: Add the state**

In `crates/ghost-engine/src/state.rs`:

```rust
/// An in-flight `!votekick`. GHost++ requires 70% of the *other* players
/// (game.cpp:1782) and expires the vote after 60 seconds.
#[derive(Debug)]
pub struct VoteKick {
    pub target_pid: u8,
    pub started_by: u8,
    pub started_at: Instant,
    pub votes: Vec<u8>,
}
```

and to `GameState`:

```rust
    pub votekick: Option<VoteKick>,
    /// Start the countdown automatically at this headcount.
    pub autostart_players: Option<usize>,
    /// `!messages off` suppresses bot chatter in game.
    pub messages_enabled: bool,
    pub locked: bool,
```

- [ ] **Step 4: Implement the vote logic**

Create `crates/ghost-engine/src/commands/vote.rs`:

```rust
use std::time::Duration;
use tokio::time::Instant;

use crate::state::{GameState, VoteKick};

/// Fraction of the other players that must agree, from game.cpp:1782.
const VOTEKICK_THRESHOLD: f32 = 0.70;
const VOTEKICK_TTL: Duration = Duration::from_secs(60);

impl GameState {
    /// Returns false when a vote is already running or the target is unknown.
    pub fn start_votekick(&mut self, by_pid: u8, target_name: &str) -> bool {
        if self.votekick.is_some() {
            self.send_chat_all("A votekick is already in progress.");
            return false;
        }
        let Ok(target) = self.players.by_name_partial(target_name) else {
            return false;
        };
        let target_pid = target.pid;
        if target_pid == by_pid {
            return false;
        }
        self.votekick = Some(VoteKick {
            target_pid,
            started_by: by_pid,
            started_at: Instant::now(),
            votes: vec![by_pid],
        });
        let needed = self.votekick_votes_needed();
        self.send_chat_all(&format!(
            "A votekick against [{target_name}] has begun. {needed} more votes are needed; type !yes to vote."
        ));
        true
    }

    fn votekick_votes_needed(&self) -> usize {
        let Some(v) = &self.votekick else { return 0 };
        let eligible = self
            .players
            .iter()
            .filter(|p| !p.virtual_host && p.pid != v.target_pid)
            .count();
        let required = (eligible as f32 * VOTEKICK_THRESHOLD).ceil() as usize;
        required.saturating_sub(v.votes.len())
    }

    pub fn cast_votekick(&mut self, pid: u8) {
        let Some(v) = self.votekick.as_mut() else { return };
        if pid == v.target_pid || v.votes.contains(&pid) {
            return;
        }
        v.votes.push(pid);
        if self.votekick_votes_needed() > 0 {
            let needed = self.votekick_votes_needed();
            self.send_chat_all(&format!("{needed} more votes are needed to kick."));
            return;
        }
        let target = self.votekick.take().expect("checked above").target_pid;
        if let Some(p) = self.players.by_pid_mut(target) {
            p.left = Some("was kicked by vote".into());
        }
        self.send_chat_all("The votekick passed.");
    }

    pub fn cancel_votekick(&mut self) {
        if self.votekick.take().is_some() {
            self.send_chat_all("The votekick was cancelled.");
        }
    }

    /// Called once per tick. `elapsed` is injectable so tests need no clock.
    pub fn expire_votekick(&mut self, elapsed: Duration) {
        let expired = self.votekick.as_ref().is_some_and(|v| {
            elapsed >= VOTEKICK_TTL || v.started_at.elapsed() >= VOTEKICK_TTL
        });
        if expired {
            self.votekick = None;
            self.send_chat_all("The votekick expired.");
        }
        let _ = std::mem::replace(&mut self.locked, self.locked);
    }
}
```

Drop the no-op `mem::replace` line; it is there only to show the borrow shape — remove it when implementing.

- [ ] **Step 5: Add autostart**

At the end of `handle_req_join` in `crates/ghost-engine/src/lobby.rs`:

```rust
        // GHost++ starts the countdown as soon as the requested headcount is
        // reached (game.cpp:543).
        if let Some(target) = self.autostart_players
            && self.players.iter().filter(|p| !p.virtual_host).count() >= target
            && matches!(self.phase, GamePhase::Lobby)
        {
            self.start_countdown("autostart");
        }
```

- [ ] **Step 6: Add the parser entries and dispatch**

Extend `parse_command` with `end`, `announce <interval> <msg>`, `autostart <n>`, `messages on|off`, `votekick <name>`, `votecancel`, `yes`, `clearhcl`, `download <name>` / `dl <name>`, `fakeplayer`, `fppause`, `fpresume`, `refresh on|off`, `sendlan`, `virtualhost <name>`, `priv <name>`, `pub <name>` — each returning the matching variant. Add the dispatch arms in `actor.rs`. Call `self.expire_votekick(Duration::ZERO)` once per tick from `on_tick`.

For `fakeplayer`/`fppause`/`fpresume`, follow `game.cpp:957-993`: the fake player occupies a real slot and its pause/resume are `W3GS_INCOMING_ACTION` blocks containing action `0x01` (pause) and `0x02` (resume) attributed to its PID.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): add votekick, autostart, announce and the fake-player commands"
```

---

### Task 12: BNET Command Router

**Files:**
- Create: `crates/ghost-bnet/src/commands.rs`
- Modify: `crates/ghost-bnet/src/client.rs:344-357`, `crates/ghost-bnet/src/lib.rs`, `crates/ghostrs/src/supervisor.rs:220-350`
- Test: `crates/ghost-bnet/src/commands.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `BnetEvent` (extended with `Whisper { from: String, text: String }` and `ChannelChat { from: String, text: String }`).
- Produces: `BnetCommand` enum and `parse_bnet_command(trigger: char, text: &str) -> Option<BnetCommand>`.

`crates/ghost-bnet/src/client.rs:344-353` currently logs chat events `0x04` (whisper) and `0x05` (talk) and does nothing else. GHost++ routes 45 commands through `CBNET::ProcessChatEvent` (`bnet.cpp:1191-2103`).

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hosting_commands_parse() {
        assert_eq!(parse_bnet_command('!', "!pub my game"), Some(BnetCommand::Pub("my game".into())));
        assert_eq!(parse_bnet_command('!', "!priv my game"), Some(BnetCommand::Priv("my game".into())));
        assert_eq!(
            parse_bnet_command('!', "!pubby alice my game"),
            Some(BnetCommand::PubBy { owner: "alice".into(), name: "my game".into() })
        );
        assert_eq!(parse_bnet_command('!', "!map dota"), Some(BnetCommand::Map(Some("dota".into()))));
        assert_eq!(parse_bnet_command('!', "!map"), Some(BnetCommand::Map(None)));
        assert_eq!(parse_bnet_command('!', "!unhost"), Some(BnetCommand::Unhost));
    }

    #[test]
    fn the_admin_commands_parse() {
        assert_eq!(parse_bnet_command('!', "!addadmin bob"), Some(BnetCommand::AddAdmin("bob".into())));
        assert_eq!(
            parse_bnet_command('!', "!addban bob spamming"),
            Some(BnetCommand::AddBan { name: "bob".into(), reason: "spamming".into() })
        );
        assert_eq!(parse_bnet_command('!', "!ban bob"), Some(BnetCommand::AddBan { name: "bob".into(), reason: String::new() }));
        assert_eq!(parse_bnet_command('!', "!countbans"), Some(BnetCommand::CountBans));
        assert_eq!(parse_bnet_command('!', "!quit"), Some(BnetCommand::Exit));
        assert_eq!(parse_bnet_command('!', "!exit"), Some(BnetCommand::Exit));
    }

    #[test]
    fn the_trigger_is_configurable_and_required() {
        assert_eq!(parse_bnet_command('.', ".pub x"), Some(BnetCommand::Pub("x".into())));
        assert_eq!(parse_bnet_command('.', "!pub x"), None, "wrong trigger");
        assert_eq!(parse_bnet_command('!', "pub x"), None, "no trigger");
        assert_eq!(parse_bnet_command('!', "!"), None, "trigger alone");
    }

    #[test]
    fn commands_are_case_insensitive_and_tolerate_extra_whitespace() {
        assert_eq!(parse_bnet_command('!', "!PUB   my   game  "), Some(BnetCommand::Pub("my   game".into())));
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-bnet commands`
Expected: FAIL — unresolved module.

- [ ] **Step 3: Implement the router**

Create `crates/ghost-bnet/src/commands.rs` with a `BnetCommand` enum covering every GHost++ command listed in `bnet.cpp:1191-2103` and a `parse_bnet_command` following the same shape as `ghost_engine::chat::parse_command`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BnetCommand {
    Accept(String),
    AddAdmin(String),
    AddBan { name: String, reason: String },
    Autohost(Option<usize>),
    AutohostMm(Option<usize>),
    Channel(String),
    CheckAdmin(String),
    CheckBan(String),
    CountAdmins,
    CountBans,
    DbStatus,
    DelAdmin(String),
    DelBan(String),
    Disable,
    Enable,
    Exit,
    GetClan,
    GetFriends,
    GetGame(usize),
    GetGames,
    Invite(String),
    Map(Option<String>),
    Motd,
    Priv(String),
    PrivBy { owner: String, name: String },
    Pub(String),
    PubBy { owner: String, name: String },
    Reload,
    Remove(String),
    Say(String),
    SayGames(String),
    Stats(Option<String>),
    StatsDota(Option<String>),
    Unhost,
    Version,
    Whisper { to: String, text: String },
}

/// Splits `<trigger><command> <rest>`; the rest is trimmed but its internal
/// whitespace is preserved so game names survive verbatim.
pub fn parse_bnet_command(trigger: char, text: &str) -> Option<BnetCommand> {
    let rest = text.strip_prefix(trigger)?;
    let (head, tail) = match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], rest[i..].trim()),
        None => (rest, ""),
    };
    if head.is_empty() {
        return None;
    }
    let cmd = head.to_ascii_lowercase();
    let one = || (!tail.is_empty()).then(|| tail.to_string());
    let two = || {
        let mut it = tail.splitn(2, char::is_whitespace);
        let a = it.next().filter(|s| !s.is_empty())?.to_string();
        let b = it.next().unwrap_or("").trim().to_string();
        Some((a, b))
    };

    Some(match cmd.as_str() {
        "pub" => BnetCommand::Pub(one()?),
        "priv" => BnetCommand::Priv(one()?),
        "pubby" => { let (owner, name) = two()?; BnetCommand::PubBy { owner, name } }
        "privby" => { let (owner, name) = two()?; BnetCommand::PrivBy { owner, name } }
        "map" => BnetCommand::Map(one()),
        "unhost" => BnetCommand::Unhost,
        "addadmin" => BnetCommand::AddAdmin(one()?),
        "deladmin" => BnetCommand::DelAdmin(one()?),
        "checkadmin" => BnetCommand::CheckAdmin(one()?),
        "countadmins" => BnetCommand::CountAdmins,
        "addban" | "ban" => { let (name, reason) = two()?; BnetCommand::AddBan { name, reason } }
        "delban" | "unban" => BnetCommand::DelBan(one()?),
        "checkban" => BnetCommand::CheckBan(one()?),
        "countbans" => BnetCommand::CountBans,
        "dbstatus" => BnetCommand::DbStatus,
        "autohost" => BnetCommand::Autohost(tail.parse().ok()),
        "autohostmm" => BnetCommand::AutohostMm(tail.parse().ok()),
        "channel" => BnetCommand::Channel(one()?),
        "disable" => BnetCommand::Disable,
        "enable" => BnetCommand::Enable,
        "exit" | "quit" => BnetCommand::Exit,
        "getclan" => BnetCommand::GetClan,
        "getfriends" => BnetCommand::GetFriends,
        "getgame" => BnetCommand::GetGame(tail.parse().ok()?),
        "getgames" => BnetCommand::GetGames,
        "invite" => BnetCommand::Invite(one()?),
        "motd" => BnetCommand::Motd,
        "reload" => BnetCommand::Reload,
        "remove" => BnetCommand::Remove(one()?),
        "say" => BnetCommand::Say(one()?),
        "saygames" => BnetCommand::SayGames(one()?),
        "stats" => BnetCommand::Stats(one()),
        "statsdota" | "sd" => BnetCommand::StatsDota(one()),
        "version" => BnetCommand::Version,
        "accept" => BnetCommand::Accept(one()?),
        "w" => { let (to, text) = two()?; BnetCommand::Whisper { to, text } }
        _ => return None,
    })
}
```

- [ ] **Step 4: Emit the chat events**

In `crates/ghost-bnet/src/client.rs`, replace the `0x04`/`0x05` arms (`:347-352`) with sends of `BnetEvent::Whisper { from, text }` and `BnetEvent::ChannelChat { from, text }`. Add both variants to `BnetEvent`.

- [ ] **Step 5: Dispatch in the supervisor**

`crates/ghostrs/src/supervisor.rs:220-350` already handles a handful of commands ad hoc. Replace that block with a single `match parse_bnet_command(self.cfg.bnet.command_trigger, &text)`, gate every admin command on `self.is_admin(&from).await`, and reply via `BnetCmd::SendChat(format!("/w {from} ..."))`. Every command from the enum must have an arm; the ones that need work not in this plan (`GetClan`, `GetFriends`, `Motd`, `Invite`, `Accept`) reply with a clear "not supported" message rather than silently doing nothing.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-bnet crates/ghostrs/src/supervisor.rs
git commit -m "feat(bnet): route the Battle.net whisper and channel commands"
```

---

### Task 13: Stats Queries and the Downloads Table

**Files:**
- Create: `crates/ghost-store/src/queries.rs`
- Modify: `crates/ghost-store/src/schema.rs`, `crates/ghost-store/src/lib.rs`, `crates/ghost-engine/src/mapxfer.rs`
- Test: `crates/ghost-store/src/queries.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `Store::player_stats(name) -> Option<PlayerStats>`, `Store::dota_stats(name) -> Option<DotaStats>`, `Store::record_download(name, ip, spoofed, map, map_size, downloaded, duration)`.

GHost++ reference: `ghostdbsqlite.cpp` `GamePlayerSummaryCheck`, `DotAPlayerSummaryCheck`, `DownloadAdd`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn seeded() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_schema(&conn).unwrap();
        conn.execute("INSERT INTO games (id, name, map, started, ended, duration) VALUES (1, 'g', 'dota', 0, 100, 100)", []).unwrap();
        conn.execute(
            "INSERT INTO dotaplayers (game_id, colour, name, hero, kills, deaths, assists, creep_kills, creep_denies, neutral_kills, tower_kills, rax_kills, courier_kills)
             VALUES (1, 1, 'alice', 'Sniper', 10, 2, 5, 100, 12, 30, 2, 1, 0)", []).unwrap();
        conn.execute("INSERT INTO dotagames (game_id, winner, duration) VALUES (1, 1, 100)", []).unwrap();
        conn
    }

    #[test]
    fn dota_stats_aggregate_across_games() {
        let conn = seeded();
        let s = dota_stats(&conn, "alice").expect("alice has stats");
        assert_eq!(s.games, 1);
        assert_eq!(s.kills, 10);
        assert_eq!(s.deaths, 2);
        assert_eq!(s.assists, 5);
        assert_eq!(s.creep_kills, 100);
    }

    #[test]
    fn an_unknown_player_has_no_stats() {
        let conn = seeded();
        assert!(dota_stats(&conn, "nobody").is_none());
    }

    #[test]
    fn name_lookup_is_case_insensitive() {
        let conn = seeded();
        assert!(dota_stats(&conn, "ALICE").is_some());
    }

    #[test]
    fn a_download_is_recorded_with_its_duration() {
        let conn = seeded();
        record_download(&conn, "bob", "1.2.3.4", 1, "dota.w3x", 8_000_000, 8_000_000, 42).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM downloads WHERE name = 'bob'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-store queries`
Expected: FAIL — unresolved module, no `downloads` table.

- [ ] **Step 3: Add the missing tables**

Append to `SCHEMA` in `crates/ghost-store/src/schema.rs`:

```sql
CREATE TABLE IF NOT EXISTS downloads (
    id         INTEGER PRIMARY KEY,
    map        TEXT NOT NULL,
    map_size   INTEGER NOT NULL,
    name       TEXT NOT NULL,
    ip         TEXT NOT NULL DEFAULT '',
    spoofed    INTEGER NOT NULL DEFAULT 0,
    downloaded INTEGER NOT NULL DEFAULT 0,
    duration   INTEGER NOT NULL DEFAULT 0,
    created    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_downloads_name ON downloads(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS scores (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL,
    server TEXT NOT NULL DEFAULT '',
    score  REAL NOT NULL DEFAULT 0,
    UNIQUE(name, server)
);
CREATE INDEX IF NOT EXISTS idx_scores_name ON scores(name COLLATE NOCASE);
```

Schema changes must be additive: existing databases run `init_schema` on every start and `CREATE TABLE IF NOT EXISTS` makes that safe.

- [ ] **Step 4: Implement the queries**

Create `crates/ghost-store/src/queries.rs`:

```rust
use rusqlite::{Connection, OptionalExtension, Result, params};

#[derive(Debug, Default, PartialEq)]
pub struct DotaStats {
    pub games: u32,
    pub wins: u32,
    pub losses: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_kills: u32,
    pub creep_denies: u32,
    pub neutral_kills: u32,
    pub tower_kills: u32,
    pub rax_kills: u32,
    pub courier_kills: u32,
}

pub fn dota_stats(conn: &Connection, name: &str) -> Option<DotaStats> {
    conn.query_row(
        "SELECT COUNT(*),
                SUM(kills), SUM(deaths), SUM(assists),
                SUM(creep_kills), SUM(creep_denies), SUM(neutral_kills),
                SUM(tower_kills), SUM(rax_kills), SUM(courier_kills)
         FROM dotaplayers WHERE name = ?1 COLLATE NOCASE",
        params![name],
        |r| {
            Ok(DotaStats {
                games: r.get(0)?,
                kills: r.get::<_, Option<u32>>(1)?.unwrap_or(0),
                deaths: r.get::<_, Option<u32>>(2)?.unwrap_or(0),
                assists: r.get::<_, Option<u32>>(3)?.unwrap_or(0),
                creep_kills: r.get::<_, Option<u32>>(4)?.unwrap_or(0),
                creep_denies: r.get::<_, Option<u32>>(5)?.unwrap_or(0),
                neutral_kills: r.get::<_, Option<u32>>(6)?.unwrap_or(0),
                tower_kills: r.get::<_, Option<u32>>(7)?.unwrap_or(0),
                rax_kills: r.get::<_, Option<u32>>(8)?.unwrap_or(0),
                courier_kills: r.get::<_, Option<u32>>(9)?.unwrap_or(0),
                ..Default::default()
            })
        },
    )
    .optional()
    .ok()
    .flatten()
    .filter(|s| s.games > 0)
}

#[allow(clippy::too_many_arguments)]
pub fn record_download(
    conn: &Connection,
    name: &str,
    ip: &str,
    spoofed: u8,
    map: &str,
    map_size: u64,
    downloaded: u64,
    duration_seconds: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO downloads (map, map_size, name, ip, spoofed, downloaded, duration, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%s', 'now'))",
        params![map, map_size as i64, name, ip, spoofed, downloaded as i64, duration_seconds as i64],
    )?;
    Ok(())
}
```

Add a `wins`/`losses` query joining `dotagames` on the player's team, and a `player_stats` equivalent over `game_players`. Route both through the existing blocking writer/reader task in `crates/ghost-store/src/writer.rs` — never call `rusqlite` from the actor.

- [ ] **Step 5: Record downloads**

In `crates/ghost-engine/src/mapxfer.rs`, when a download completes, emit a store command carrying name, IP, spoofed flag, map name, size, bytes sent and elapsed seconds.

- [ ] **Step 6: Wire `!stats` / `!statsdota`**

The `ChatCommand::Stats` and `ChatCommand::StatsDotA` arms in `actor.rs` currently exist but have no data source. Have them send a store query and reply with the GHost++ wording from `language.cpp`.

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/ghost-store crates/ghost-engine/src/mapxfer.rs crates/ghost-engine/src/actor.rs
git commit -m "feat(store): add download and score tables plus the stats queries"
```

---

### Task 14: Real NLS (SRP-6a) Logon

**Files:**
- Create: `crates/ghost-bnet/src/nls.rs`
- Modify: `crates/ghost-bnet/src/client.rs:281-343`, `crates/ghost-bnet/src/lib.rs`, `crates/ghost-protocol/src/bncs/incoming.rs`
- Test: `crates/ghost-bnet/src/nls.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `Nls::new(username: &str, password: &str) -> Self`, `Nls::client_public_key(&self) -> [u8; 32]`, `Nls::client_proof(&mut self, salt: &[u8; 32], server_key: &[u8; 32]) -> [u8; 20]`, `Nls::verify_server_proof(&self, proof: &[u8; 20]) -> bool`.

`crates/ghost-bnet/src/auth.rs:116-122` currently returns 32 random bytes as the "client key", which is not an SRP public value, and `client.rs:312` accepts `SID_AUTH_ACCOUNTLOGONPROOF` without ever computing a proof. Against a PvPGN configured for NLS this fails; only the legacy `SID_LOGONRESPONSE2` path works today.

Battle.net NLS is SRP-6a with fixed parameters (bncsutil `nls.c`):
- `N` = the 256-bit safe prime `F8FF1A8B 6E6C6DCB ... 3F0FD48D` (little-endian on the wire)
- `g` = 47
- `x = SHA1(salt ‖ SHA1(USERNAME:PASSWORD uppercased))`
- `v = g^x mod N`, `A = g^a mod N`, `u` = first 32 bits of `SHA1(B)` reversed
- `S = (B - g^x)^(a + u·x) mod N`, `K` = the odd/even SHA1 interleave of `S`
- `M1 = SHA1(SHA1(N) xor SHA1(g) ‖ SHA1(USERNAME) ‖ salt ‖ A ‖ B ‖ K)`
- `M2 = SHA1(A ‖ M1 ‖ K)`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Vector produced by bncsutil's nls_test with a fixed private value.
    const SALT: [u8; 32] = [0x11; 32];
    const SERVER_KEY: [u8; 32] = [0x22; 32];

    #[test]
    fn the_client_public_key_is_deterministic_for_a_fixed_private_value() {
        let a = Nls::with_private_key("TEST", "test", [0x33; 32]);
        let b = Nls::with_private_key("TEST", "test", [0x33; 32]);
        assert_eq!(a.client_public_key(), b.client_public_key());
        assert_ne!(a.client_public_key(), [0u8; 32], "A must not be zero");
    }

    #[test]
    fn a_different_password_yields_a_different_proof() {
        let mut a = Nls::with_private_key("TEST", "correct", [0x33; 32]);
        let mut b = Nls::with_private_key("TEST", "wrong", [0x33; 32]);
        assert_ne!(a.client_proof(&SALT, &SERVER_KEY), b.client_proof(&SALT, &SERVER_KEY));
    }

    #[test]
    fn the_username_is_case_insensitive() {
        let mut a = Nls::with_private_key("test", "pw", [0x33; 32]);
        let mut b = Nls::with_private_key("TEST", "pw", [0x33; 32]);
        assert_eq!(a.client_proof(&SALT, &SERVER_KEY), b.client_proof(&SALT, &SERVER_KEY));
    }

    #[test]
    fn the_server_proof_check_rejects_a_forged_value() {
        let mut n = Nls::with_private_key("TEST", "pw", [0x33; 32]);
        let _ = n.client_proof(&SALT, &SERVER_KEY);
        assert!(!n.verify_server_proof(&[0u8; 20]));
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-bnet nls`
Expected: FAIL — unresolved module.

- [ ] **Step 3: Add a bignum dependency**

Add `num-bigint = "0.4"` and `num-traits = "0.2"` to `crates/ghost-bnet/Cargo.toml`. Do not hand-roll modular exponentiation.

- [ ] **Step 4: Implement `nls.rs`**

Implement the algorithm above with `BigUint::modpow`. Keep every wire value **little-endian 32 bytes**: Battle.net sends and expects LE, while `num-bigint` works in BE, so convert at the boundary in exactly two helper functions (`le_to_big`, `big_to_le32`) and nowhere else. Provide both `Nls::new` (random `a` from `rand::random`) and `Nls::with_private_key` (fixed `a`, for tests).

- [ ] **Step 5: Wire it into the client**

In `crates/ghost-bnet/src/client.rs`:
- Replace `auth::generate_client_key()` with `nls.client_public_key()` when the server advertises NLS.
- Add a decoder for `SID_AUTH_ACCOUNTLOGON` (0x53) in `crates/ghost-protocol/src/bncs/incoming.rs`: `u32 status`, `[u8; 32] salt`, `[u8; 32] server_key`. Status `0x00` = ok, `0x01` = no such account, `0x05` = upgrade required.
- Add the `Stage::AwaitAccountLogon` arm that computes `client_proof` and sends `outgoing::account_logon_proof(&proof)`.
- In the `SID_AUTH_ACCOUNTLOGONPROOF` arm, verify the server proof and log a warning (not a failure) on mismatch — some PvPGN builds send a zero proof.

- [ ] **Step 6: Keep the old-logon fallback**

If `SID_AUTH_INFO` reports the server does not support NLS, keep the existing `logon_response2` path. Select between them explicitly rather than by trying both.

- [ ] **Step 7: Extend the handshake test**

`crates/ghost-bnet/tests/handshake.rs` already drives a scripted server. Add an NLS branch that serves a canned `SID_AUTH_ACCOUNTLOGON` and asserts the client answers with a 20-byte `SID_AUTH_ACCOUNTLOGONPROOF`.

- [ ] **Step 8: Run tests to verify they pass**

Run: `cargo test -p ghost-bnet`
Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/ghost-bnet crates/ghost-protocol/src/bncs/incoming.rs
git commit -m "feat(bnet): implement SRP-6a NLS account logon"
```

---

### Task 15: CD-Key Decode and Version Hash

**Files:**
- Create: `crates/ghost-bnet/src/cdkey.rs`
- Modify: `crates/ghost-bnet/src/auth.rs:95-113`, `crates/ghost-bnet/src/client.rs:281-311`
- Test: `crates/ghost-bnet/src/cdkey.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `CdKey::decode(key: &str) -> Result<CdKey, CdKeyError>` with fields `product: u32`, `public_value: u32`, `private_value: [u8; 10]`; `CdKey::key_info(&self, client_token: u32, server_token: u32) -> Vec<u8>`; `version_hash(value_string: &str, files: &[PathBuf]) -> Result<(u32 version, u32 checksum, [u8; 20] digest), VersionError>`.

`crates/ghost-bnet/src/auth.rs:97-113` hardcodes `public value = 1`, `product = 4/7` and `SHA1(ct ‖ st ‖ key_ascii)`. The real format (bncsutil `decodeKey.c`) shuffles and decodes the 26-character key into a product id, a public value and a 10-byte private value, and hashes `ct ‖ st ‖ product ‖ public ‖ 0 ‖ private`.

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // A well-formed 26-character TFT key. Not a real key: the decoder is
    // deterministic, so any structurally valid key exercises the same path.
    const KEY: &str = "MFRDGMBWGA2DQMBWGA2DQMBWGA";

    #[test]
    fn a_twenty_six_character_key_decodes_to_the_expected_shape() {
        let k = CdKey::decode(KEY).expect("structurally valid");
        assert_eq!(k.private_value.len(), 10);
        assert_ne!(k.public_value, 0);
    }

    #[test]
    fn a_malformed_key_is_rejected_rather_than_silently_accepted() {
        assert!(CdKey::decode("").is_err());
        assert!(CdKey::decode("TOO-SHORT").is_err());
        assert!(CdKey::decode("!!!!!!!!!!!!!!!!!!!!!!!!!!").is_err(), "invalid alphabet");
    }

    #[test]
    fn key_info_is_thirty_six_bytes_with_the_declared_length_first() {
        let k = CdKey::decode(KEY).unwrap();
        let info = k.key_info(0x1234, 0x5678);
        assert_eq!(info.len(), 36);
        assert_eq!(u32::from_le_bytes([info[0], info[1], info[2], info[3]]), 26);
    }

    #[test]
    fn the_same_key_with_different_tokens_hashes_differently() {
        let k = CdKey::decode(KEY).unwrap();
        assert_ne!(k.key_info(1, 2)[16..], k.key_info(3, 4)[16..]);
    }
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-bnet cdkey`
Expected: FAIL — unresolved module.

- [ ] **Step 3: Implement the decoder**

Port `bncsutil/src/bncsutil/decodeKey.c` (present at `ref/ghostpp/bncsutil/src/bncsutil/decodeKey.c`) into `crates/ghost-bnet/src/cdkey.rs`: the base-N translate table, the two shuffle passes, the final nibble swap, then split into product (bits 0-9 of the first dword), public value and private value. `key_info` emits `[u32 len][u32 product][u32 public][u32 zero][20-byte SHA1(ct ‖ st ‖ product ‖ public ‖ 0 ‖ private)]`.

- [ ] **Step 4: Implement the version hash**

`version_hash` implements `checkRevision` (`bncsutil/src/bncsutil/checkrevision.c`): parse the four-operand `ValueStringFormula` the server sends in `SID_AUTH_INFO`, seed four 32-bit registers, then stream `war3.exe`, `storm.dll` and `game.dll` through the formula. The file paths come from `cfg.bot.war3_path`. Return the computed `version`, `checksum` and the 20-byte digest.

If any of the three files is missing, return `VersionError::MissingFile(path)` and have the client log it and fall back to the configured static `war3_version`, so a bot without a WC3 install still starts (as today) but says why the hash is not real.

- [ ] **Step 5: Wire into `SID_AUTH_CHECK`**

Replace the `auth::create_key_info` call in `client.rs` with `CdKey::decode(&cfg.cdkey_tft)?.key_info(client_token, server_token)`, and the placeholder version fields with the `version_hash` result. Delete `auth::create_key_info` and `auth::generate_client_key`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ghost-bnet`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-bnet
git commit -m "feat(bnet): decode CD-keys and compute the real version hash"
```

---

### Task 16: Remove Blocking Work From the Actor Thread

**Files:**
- Modify: `crates/ghost-engine/src/mapxfer.rs:140-190`, `crates/ghost-engine/src/map.rs`, `crates/ghost-engine/src/actor.rs`
- Test: `crates/ghost-engine/benches/tick.rs`, `crates/ghost-engine/src/mapxfer.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `MapInfo::data: Option<Arc<Vec<u8>>>` stays, but map *loading* moves to `spawn_blocking`; `pump_downloads` gains a per-tick byte budget.

Two blocking hazards remain on the tick after Tasks 3 and 13:
1. `crates/ghost-engine/src/map.rs` parses the MPQ (a multi-megabyte file read plus decompression) — verify it happens only in the supervisor before `spawn_game`, never inside the actor.
2. `pump_downloads` slices map chunks with no cap, so a 8 MB map to 8 downloaders can enqueue tens of megabytes in one tick.

- [ ] **Step 1: Write the failing test**

```rust
#[tokio::test]
async fn map_upload_is_rate_limited_per_tick() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
    st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 8 * 1024 * 1024]));
    st.downloads.push(Download::new(1));

    st.pump_downloads();

    let sent = st.downloads[0].bytes_sent;
    assert!(sent > 0, "the download must progress");
    assert!(
        sent <= MAX_DOWNLOAD_BYTES_PER_TICK,
        "one tick sent {sent} bytes, above the {MAX_DOWNLOAD_BYTES_PER_TICK} budget"
    );
}

#[tokio::test]
async fn the_budget_is_shared_across_concurrent_downloaders() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(3);
    st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 8 * 1024 * 1024]));
    for pid in 1..=3u8 {
        st.downloads.push(Download::new(pid));
    }
    st.pump_downloads();
    let total: usize = st.downloads.iter().map(|d| d.bytes_sent).sum();
    assert!(total <= MAX_DOWNLOAD_BYTES_PER_TICK, "budget is global, not per-player");
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test -p ghost-engine download`
Expected: FAIL — `MAX_DOWNLOAD_BYTES_PER_TICK` not defined; the budget is unbounded.

- [ ] **Step 3: Add the budget**

In `crates/ghost-engine/src/mapxfer.rs`:

```rust
/// Bytes of map data one tick may enqueue across all downloaders. At 100 ms per
/// tick this is a 1.5 MB/s ceiling, enough to serve an 8 MB map in ~6 s while
/// leaving the write queues room for gameplay traffic.
pub const MAX_DOWNLOAD_BYTES_PER_TICK: usize = 150 * 1024;
```

and thread a `remaining` budget through `pump_downloads`, decrementing per chunk and breaking when it reaches zero. Round-robin the starting downloader each tick so no player is starved.

- [ ] **Step 4: Assert the map is parsed off the actor**

Add to `crates/ghost-engine/src/map.rs` tests:

```rust
#[test]
fn parsing_a_map_never_happens_inside_the_actor() {
    // The actor holds only the already-parsed MapInfo; ParsedMap::from_path is
    // called by the supervisor before spawn_game. This test documents the
    // invariant so a future refactor cannot quietly reintroduce file I/O.
    let src = include_str!("actor.rs");
    assert!(!src.contains("ParsedMap::from_path"), "map parsing must not appear in the actor");
    assert!(!src.contains("std::fs::"), "the actor must not touch the filesystem");
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected: PASS.

- [ ] **Step 6: Add a tick-latency benchmark for the download path**

Extend `crates/ghost-engine/benches/tick.rs` with a case that runs `on_tick` with three active downloads and an 8 MB map, so a regression in the budget shows up as a latency spike.

- [ ] **Step 7: Run the benchmark**

Run: `cargo bench -p ghost-engine --bench tick`
Expected: completes; record the reported time in the commit message.

- [ ] **Step 8: Commit**

```bash
git add crates/ghost-engine
git commit -m "perf(engine): cap map upload per tick and pin map parsing off the actor"
```

---

### Task 17: Hot-Path Allocation Removal

**Files:**
- Modify: `crates/ghost-engine/src/actions.rs:109-140`, `crates/ghost-engine/src/state.rs:191-238`, `crates/ghost-engine/src/slots.rs`
- Test: `crates/ghost-engine/benches/tick.rs`

**Interfaces:**
- No API change. `GameState` gains reusable scratch buffers.

Per-tick allocations found by reading the hot path:
- `send_all_actions` does `std::mem::take(&mut self.actions)` then builds a fresh `Vec<ActionBlock> batch` every tick.
- `send_chat_all` collects a fresh `Vec<u8>` of PIDs on every bot message.
- `send_all_slot_info` calls `slots.as_wire()`, which allocates a `Vec<u8>` — and it is called on every join and leave.
- `reap_left_players` allocates a `Vec<(u8, String)>` every tick even when nobody left.

- [ ] **Step 1: Write the failing benchmark assertion**

Add to `crates/ghost-engine/benches/tick.rs`:

```rust
fn bench_steady_state_tick(c: &mut Criterion) {
    // 10 players, 4 queued actions per tick: the DotA steady state.
    let mut group = c.benchmark_group("tick");
    group.bench_function("steady_state_10p", |b| {
        let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
        b.iter_batched(
            || rt.block_on(async { seated_state(10, 4) }),
            |mut st| st.on_tick(0),
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}
```

- [ ] **Step 2: Record the baseline**

Run: `cargo bench -p ghost-engine --bench tick -- steady_state_10p`
Write the reported time into `docs/PERF.md` under "before".

- [ ] **Step 3: Add the scratch buffers**

In `GameState`:

```rust
    /// Reused across ticks so the steady state allocates nothing.
    batch_scratch: Vec<ActionBlock>,
    pid_scratch: Vec<u8>,
    reap_scratch: Vec<(u8, String)>,
    slot_wire_scratch: Vec<u8>,
```

- [ ] **Step 4: Use them**

Replace each allocation with `self.<field>.clear()` followed by the same pushes, taking care to `std::mem::take` the buffer where the borrow checker requires it and to put it back afterwards. Change `SlotTable::as_wire` to `SlotTable::write_wire(&self, out: &mut Vec<u8>)` and keep `as_wire` as a thin wrapper for tests and the replay body.

Guard `reap_left_players` with an early return:

```rust
        if self.players.iter().all(|p| p.left.is_none()) {
            return;
        }
```

- [ ] **Step 5: Re-run the benchmark**

Run: `cargo bench -p ghost-engine --bench tick -- steady_state_10p`
Expected: no regression; record the "after" number in `docs/PERF.md`. If the change does not measurably help, keep the early return and the `write_wire` change and revert the scratch buffers — an unmeasured optimisation is not worth the complexity.

- [ ] **Step 6: Run the tests**

Run: `cargo test --workspace`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-engine docs/PERF.md
git commit -m "perf(engine): reuse tick scratch buffers and skip the reap fast path"
```

---

### Task 18: End-to-End Verification and Documentation

**Files:**
- Modify: `crates/ghost-loadtest/src/main.rs`, `README.md`, `docs/PERF.md`
- Create: `tests/e2e_dotatv.rs`

**Interfaces:**
- Consumes: everything above.

- [ ] **Step 1: Write the end-to-end DotaTV test**

Create `tests/e2e_dotatv.rs`:

```rust
//! Starts a game actor and a relay, connects a synthetic viewer over a real
//! TCP socket, and asserts the viewer receives HELLO, PLAYERS and delayed
//! GAMEBLOCKs in that order.
use std::time::Duration;

use bytes::BytesMut;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio_util::codec::Decoder;

#[tokio::test]
async fn a_viewer_receives_the_greeting_then_delayed_game_blocks() {
    let cfg = ghost_spectator::RelayConfig {
        port: 0, // bind an ephemeral port and read it back
        delay: Duration::from_millis(200),
        max_viewers: 4,
        game_name: "e2e".into(),
        map_name: "DotA".into(),
        num_slots: 10,
        max_queued_blocks: 100,
    };
    let (handle, port, _join) = ghost_spectator::spawn_relay_bound(cfg).await.unwrap();

    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut framed = ghost_protocol::dtv::DtvCodec::default().framed(stream);

    handle.set_players(vec![(1, 0, 0, "alice".into())]);
    handle.push_block(bytes::Bytes::from_static(&[0xF7, 0x0C, 0x05, 0x00, 0x01]));

    use futures_util::StreamExt;
    let f1 = framed.next().await.unwrap().unwrap();
    assert_eq!(f1.id, ghost_protocol::dtv::ids::HELLO);
    let f2 = framed.next().await.unwrap().unwrap();
    assert_eq!(f2.id, ghost_protocol::dtv::ids::PLAYERS);

    let f3 = tokio::time::timeout(Duration::from_secs(2), framed.next())
        .await
        .expect("the block must arrive after the delay")
        .unwrap()
        .unwrap();
    assert_eq!(f3.id, ghost_protocol::dtv::ids::GAMEBLOCK);
    assert_eq!(&f3.payload[..], &[0xF7, 0x0C, 0x05, 0x00, 0x01]);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --test e2e_dotatv`
Expected: FAIL — `spawn_relay_bound` does not exist.

- [ ] **Step 3: Add `spawn_relay_bound`**

In `crates/ghost-spectator/src/relay.rs`, add an async variant that binds the listener before returning and yields `(RelayHandle, u16, JoinHandle<()>)`. Have `spawn_relay` call it. This removes the race where a test connects before the listener exists.

- [ ] **Step 4: Run it to verify it passes**

Run: `cargo test --test e2e_dotatv`
Expected: PASS.

- [ ] **Step 5: Extend the load test with viewers**

In `crates/ghost-loadtest/src/main.rs`, add a `--viewers N` flag that opens N DotaTV sockets against the relay and reports the p50/p99 delay between a block being pushed and each viewer receiving it.

- [ ] **Step 6: Measure**

Run: `cargo run --release -p ghost-loadtest -- --games 20 --players 10 --viewers 50`
Record the tick jitter histogram and the viewer delivery percentiles in `docs/PERF.md`.

- [ ] **Step 7: Update the docs**

In `README.md`, document: the `[replay]` and extended `[spectator]` config sections, the DotaTV protocol table from Task 6, the full command list (in-game and BNET), and an explicit "not implemented" list — admin game, savegame `!load`, MySQL backend, localisation, BNLS, Warden.

- [ ] **Step 8: Final verification**

Run: `cargo check --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: all clean.

- [ ] **Step 9: Commit**

```bash
git add tests/e2e_dotatv.rs crates/ghost-loadtest README.md docs/PERF.md crates/ghost-spectator/src/relay.rs
git commit -m "test: add the end-to-end DotaTV path and refresh the performance baseline"
```

---

## Dependency Graph

```
1 (virtual host) ──┬─> 4 (replay wiring) ──┐
2 (w3g container) ─┴─> 3 (replay body) ────┘
5 (gproxy buffer)  ── independent
6 (dtv codec) ─> 7 (relay) ─> 8 (engine feed) ─> 9 (c++ client)
                        └─────────────────────> 18 (e2e)
10 (lobby cmds) ─> 11 (game cmds) ─┐
12 (bnet cmds) ────────────────────┼─> 13 (stats queries)
14 (nls) ─> 15 (cdkey)             │
16 (blocking) ─> 17 (allocations) ─┴─> 18 (verification)
```

Tasks 1-5, 6-9, 10-13, 14-15 and 16-17 are four independent tracks that can run in parallel; Task 18 gates on all of them.

## Self-Review

**Spec coverage.** The request was: compare `ghostrs` to `ghostpp`, list what is missing, what needs finishing, performance problems, and DotaTV. Coverage: missing features → the gap table and Tasks 10-15; unfinished work → Tasks 1-5 (four verified defects); performance → Tasks 16-17 plus the bounded relay queue in Task 7; DotaTV → Tasks 6-9 and 18. The admin game, savegame, MySQL, localisation, BNLS and Warden are named as deliberate exclusions rather than left unmentioned.

**Placeholders.** Every code step carries real code. Two places state a judgement instead of an exact value and say so explicitly: Task 10 Step 6 (`From` needs a GeoIP source, which the repo does not have — it falls back to `"??"`), and Task 15 Step 4 (`version_hash` falls back to the configured version when the WC3 files are absent). Task 3 Step 3 flags an ordering subtlety in the test rather than hiding it.

**Type consistency.** `RelayConfig` gains `map_name`, `num_slots`, `max_queued_blocks` in Task 7 and every construction site is updated in the same task (relay tests, Task 8's test, the supervisor, Task 18's e2e test). `ReplayBody` methods are named identically in Tasks 3, 4 and 8. `GProxyBuffer::new` keeps its signature; only the call site moves. `SlotTable::as_wire` survives Task 17 as a wrapper so Tasks 3 and 8 keep compiling.

## Assumptions

1. **The admin game is dropped.** Chosen by the user. `game_admin.cpp`'s 45 commands are reachable through the BNET whisper router in Task 12, so nothing is lost except compatibility with GHost++ `admingame_*` config keys. `crates/ghostrs/src/config.rs` should reject those keys with a clear message rather than ignoring them.
2. **The DotaTV client is ours to change.** Task 9 modifies `dotatv_client`. If that project has other consumers, Task 9 must ship behind a version byte in `HELLO` instead.
3. **Replay recording is on by default.** Task 4 adds a `[replay] enabled` flag defaulting to `true`, matching GHost++'s `bot_savereplays`. Disk usage is ~1 MB per hour-long game.
4. **`SlotTable` exposes per-slot fields.** Task 10's code assumes named fields (`status`, `computer`, `colour`, `race`, `team`, `handicap`). If it stores packed bytes, add accessors in Task 10 Step 5 rather than reshaping the type — nothing else in the plan depends on the representation.
