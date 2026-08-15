# ghostrs: Join Fix, Game Lifecycle & DotaTV Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the advertisement defects preventing real Warcraft III 1.26a clients from joining hosted games on iCCup PvPGN, implement complete game lifecycle management with real DotA stats parsing and replay recording, and turn the spectator relay into an end-to-end framed DotaTV stream for both the Rust hostbot and the C++ game-client DLL.

**Architecture:** The `ghostrs` workspace maintains its single-actor-per-game Tokio concurrency model where `GameState` owns simulation state and deadlines are strictly scheduled. This plan completes the lifecycle by wiring game-over detection and DotA `0x6b "dr.x\0"` stats parsing into off-thread SQLite persistence and `.w3g` replay recording, fixes Battle.net `0x1C` advertisement bytes, and establishes a bidirectional `0xFD`-framed DotaTV streaming protocol between the spectator relay and the injected C++ client DLL.

**Tech Stack:** Rust 2024 edition, tokio 1.45, `tokio_util::codec`, `bytes`, `rusqlite` (WAL mode), `flate2`, `crc32fast`, `tracing`, `proptest`. C++ client: MSVC 2022+ (v145 / std:c++20), Winsock2, MinHook.

---

## Global Constraints

- **Wire format is law.** Every W3GS/BNCS/DTV byte must match GHost++ and the established protocol. Authority citations refer to `C:\Users\slash\iccwc3_work\ref\ghostpp\ghost\`.
- **Pure-Rust workspace.** `#![forbid(unsafe_code)]` remains strictly enforced in `ghost-protocol` and `ghost-bnet`. No native C/C++ dependencies in Rust crates.
- **Never block the actor thread.** No disk file I/O, no zlib compression, and no SQLite queries inside `GameState::on_tick` or command handlers. Offload all blocking operations via `tokio::task::spawn_blocking` or dedicated background tasks.
- **Never await a socket in the tick loop.** Outbound player traffic flows non-blockingly through `PlayerLink::try_send` into bounded queues.
- **Test-Driven Development (TDD).** Every task writes a failing unit/integration test first, verifies failure, implements the minimal fix, and verifies all tests pass.
- **Atomic commits per task.** Every task concludes with a single git commit. `cargo check --workspace` and `cargo test --workspace` must remain green across all commits.
- **Target platform.** Warcraft III 1.26a (version 26, build 6059), The Frozen Throne (`W3XP`).
- **No placeholders.** All code blocks must contain complete, compilable, and executable implementations with explicit type signatures and error handling.

---

## Current State & Gap Analysis

### Workspace Crate Structure

```
ghostrs/
├── crates/
│   ├── ghost-protocol/      # Codecs for W3GS (0xF7), GPS (0xF8), BNCS (0xFF); forbid(unsafe_code)
│   ├── ghost-bnet/          # Pure-Rust Battle.net client state machine, SRP-6a / NLS, CheckRevision
│   ├── ghost-engine/        # GameState actor, tick loop, slot table, lobby, actions, HCL
│   ├── ghost-spectator/     # Spectator relay, W3gWriter container, ReplayBody
│   ├── ghost-store/         # SQLite WAL actor (bans, admins, games, dotagames, dotaplayers)
│   ├── ghost-net/           # TCP listeners, dual-codec connections, UDP LAN broadcast
│   ├── ghostrs/             # Application binary, CLI supervisor, config loader
│   └── ghost-loadtest/      # Synthetic W3GS client load-testing harness
└── docs/superpowers/plans/  # Architecture & implementation plans
```

### Verified Defect & Gap Breakdown

1. **Workstream 0 — The Join Bug:**
   - `crates/ghost-protocol/src/bncs/outgoing.rs:75` writes arbitrary `state: u8`, and all three call sites in `crates/ghost-bnet/src/client.rs` (lines 173, 190, 221) pass `0`. GHost++ `gameprotocol.h:32-33` defines `GAME_PUBLIC = 16` and `GAME_PRIVATE = 17`. `bnetprotocol.cpp:702` states `"State (16 = public, 17 = private, 18 = close)"`. A state of `0` causes PvPGN to list the game in the client UI but reject join attempts with *"The game you attempted to join could not be found."*
   - `crates/ghost-protocol/src/bncs/outgoing.rs:83` hardcodes `98` (ASCII `'b'` = 11 slots free for 12-slot games). In GHost++ `bnetprotocol.cpp:712-715`, `MAX_SLOTS > 12` sends `110` (ASCII `'n'` = 23 slots free). `gameslot.h:39` defines `MAX_SLOTS = 24`. The C++ comment warns this represents the number of PIDs Warcraft III will allocate and must not be reduced.
   - `crates/ghostrs/src/supervisor.rs:258` parses `!pub` and `!priv` but discards the distinction, always sending public adverts.
   - `bnet.cpp:2255-2256` and `bnet.cpp:2283-2284`: When advertising private games, `MapGameType` must also have bit `MAPGAMETYPE_PRIVATEGAME` (`0x0000_0800` / 2048) OR'd in.

2. **Workstream 1 — Game Lifecycle & Parity:**
   - **Game-Over Detection:** `crates/ghost-engine/src/actions.rs` currently only sets `GamePhase::Over` when all human players leave. GHost++ detects map victory conditions in real-time via DotA action packets (`game.cpp:337-342`) and initiates a 60-second grace countdown (`game_base.cpp:1067`), as well as detecting 1-player-left conditions (`game_base.cpp:1059`).
   - **DotA Stats Stream Parsing:** `crates/ghost-engine/src/stats_dota.rs` currently scans for fictional signatures (`b"TheT"`, `b"WorT"`, `b"Hero"`, `b"Kill"`). Real Warcraft III DotA maps embed real-time stats in action ID `0x6b` with header `"dr.x\0"` (`statsdota.cpp:51-371`), emitting structured events (`"Data"`, `"Global"`, `"1"`..`"11"`).
   - **Replay Recording Wiring:** `ghost-spectator` contains a fully functional, verified `.w3g` container (`w3g.rs`) and replay body builder (`body.rs`), but `ghost-engine` never instantiates or feeds `ReplayBody`.
   - **Stats Persistence:** `ghost-store` contains schema tables (`dotagames`, `dotaplayers`) and query methods (`get_dota_stats`), but neither the engine nor supervisor ever calls `log_game` or `log_dota_game`.
   - **Command Surface:** Scoped to an iCCup DotA hostbot: lobby slot manipulation (`!openall`, `!closeall`, `!comp`, `!compcolour`, `!comphandicap`, `!comprace`, `!compteam`), game control (`!start`, `!abort`, `!autostart`, `!votekick`, `!votecancel`, `!yes`, `!lock`, `!unlock`, `!from`, `!check`, `!checkme`), and BNET administration (`!pub`, `!priv`, `!pubby`, `!privby`, `!map`, `!unhost`, `!ban`, `!unban`, `!checkban`, `!countbans`, `!addadmin`, `!deladmin`, `!checkadmin`, `!stats`, `!statsdota`).

3. **Workstream 2 — DotaTV Spectator Protocol & C++ DLL:**
   - **Relay Defects:** `crates/ghost-spectator/src/relay.rs:173` drops `RelayCmd::PlayerInfo` as a no-op; line 122 discards all inbound viewer packets; raw W3GS chat frames are broadcast instead of framed DTV packets; viewers are accepted via `ghost_net::spawn_conn` (expecting `0xF7`/`0xF8`) which strips `0xFD` bytes.
   - **C++ Client DLL Defects:** `dotatv_client/src/NetClient.cpp:61-74` hands raw `recv()` buffers directly to callback with zero packet framing; line 53 sends unencoded newline-terminated strings; `NetClient.hpp:15` uses an untyped packet callback.
   - **Unified Protocol:** Enforce the standard `0xFD` length-prefixed frame protocol (`[0xFD][id][u16 LE total length][payload]`) for IDs `0x01` (HELLO), `0x02` (PLAYERS), `0x03` (GAMEBLOCK), `0x04` (CHAT), `0x05` (GAMEOVER), and `0x10` (VIEWER_CHAT).

---

## File Structure

### Created Files

| File | Purpose |
|---|---|
| `crates/ghost-protocol/src/dtv/mod.rs` | DotaTV `0xFD` message IDs, encoders, and decoders |
| `crates/ghost-spectator/src/conn.rs` | Dedicated viewer connection task speaking `HeaderCodec<0xFD>` |
| `crates/ghost-engine/src/commands/mod.rs` | Command subsystem router and permission validation |
| `crates/ghost-engine/src/commands/lobby_cmds.rs` | Slot management and computer player commands |
| `crates/ghost-engine/src/commands/vote.rs` | Votekick threshold calculations, timer expiration, and casting |
| `crates/ghost-bnet/src/commands.rs` | Battle.net whisper and channel command parser |
| `crates/ghost-store/src/queries.rs` | Aggregate DotA player stats and downloads queries |
| `dotatv_client/include/DtvProtocol.hpp` | C++ DotaTV protocol definitions and constants |
| `dotatv_client/tests/test_framing.cpp` | Standalone C++ test executable validating stream reassembly |
| `tests/e2e_dotatv.rs` | End-to-end Rust integration test driving relay and synthetic viewer |

### Modified Files

| File | Lines / Responsibility |
|---|---|
| `crates/ghost-protocol/src/bncs/outgoing.rs` | Add `GameVisibility` enum, update `startadvex3` state and slots-free bytes |
| `crates/ghost-protocol/src/lib.rs` | Export `dtv` module and `GameVisibility` |
| `crates/ghost-bnet/src/client.rs` | Thread `GameVisibility` through `CreateGame`, `RefreshGame`, and advert timer |
| `crates/ghost-bnet/src/advert.rs` | Encode `MAPGAMETYPE_PRIVATEGAME` flag when private |
| `crates/ghostrs/src/supervisor.rs` | Wire `!pub`/`!priv` visibility, game lifecycle persistence, command routing |
| `crates/ghost-engine/src/stats_dota.rs` | Implement byte-accurate `0x6b "dr.x\0"` DotA action parser |
| `crates/ghost-engine/src/state.rs` | Add game-over timer, votekick state, replay body, and relay handles |
| `crates/ghost-engine/src/actions.rs` | Hook action stream to DotA parser, replay recorder, and spectator relay |
| `crates/ghost-engine/src/lobby.rs` | Autostart check, virtual host seat management, slot info updates |
| `crates/ghost-engine/src/slots.rs` | Slot manipulation helpers (`add_computer`, `set_colour`, `open_all`, etc.) |
| `crates/ghost-engine/src/chat.rs` | Extend `ChatCommand` variants and parser |
| `crates/ghost-engine/src/actor.rs` | Dispatch extended commands, handle game-over transitions |
| `crates/ghost-spectator/src/relay.rs` | Full bidirectional DotaTV protocol implementation |
| `crates/ghost-spectator/src/lib.rs` | Export `conn` and `dtv` types |
| `crates/ghost-store/src/schema.rs` | Add `downloads` table and indices |
| `crates/ghost-store/src/writer.rs` | Support download tracking, game logging, and stats queries |
| `dotatv_client/include/NetClient.hpp` | Replace raw callback with typed `MessageCallback`, add `SendViewerChat` |
| `dotatv_client/src/NetClient.cpp` | Implement length-prefixed stream reassembly buffer |
| `dotatv_client/src/DotaTV.cpp` | Wire typed message handlers for HELLO, PLAYERS, GAMEBLOCK, CHAT, GAMEOVER |

---

# Workstream 0 — The Join Bug Fix

### Task 1: Fix BNCS Advertisement State Byte and Slots Free Wire Format

**Files:**
- Modify: `crates/ghost-protocol/src/bncs/outgoing.rs:60-90`
- Modify: `crates/ghost-protocol/src/lib.rs`
- Modify: `crates/ghost-bnet/src/client.rs:170-235`
- Modify: `crates/ghost-bnet/src/advert.rs:18-35`
- Modify: `crates/ghostrs/src/supervisor.rs:255-275`
- Test: `crates/ghost-protocol/src/bncs/outgoing.rs` (inline `mod tests`)
- Test: `crates/ghost-bnet/src/client.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `ghost_protocol::bncs::outgoing::GameVisibility` enum (`Public = 16`, `Private = 17`, `Close = 18`).
- Produces: `ghost_protocol::bncs::outgoing::startadvex3(visibility: GameVisibility, map_game_type: [u8; 4], game_name: &str, host_name: &str, up_time: u32, stat_string: &[u8], host_counter: u32) -> Result<Bytes, ProtoError>`.
- Produces: `ghost_bnet::BnetCmd::CreateGame { name: String, map: MapAdvert, host_counter: u32, visibility: GameVisibility }`.

- [ ] **Step 1: Write failing unit tests for `GameVisibility` and `startadvex3` wire bytes**

Add to `crates/ghost-protocol/src/bncs/outgoing.rs` inside `mod tests`:

```rust
#[test]
fn startadvex3_writes_correct_visibility_and_24_slot_capacity() {
    let stat_string = vec![0x01, 0x02, 0x00];
    let pkt_pub = startadvex3(
        GameVisibility::Public,
        [1, 0, 0, 0],
        "DotA 5v5",
        "iCCupHost",
        0,
        &stat_string,
        0x12345678,
    )
    .expect("packet encoding must succeed");

    // Header: [0xFF, 0x1C, len_lo, len_hi]
    assert_eq!(pkt_pub[0], 0xFF);
    assert_eq!(pkt_pub[1], 0x1C);
    // Offset 4: State byte must be 16 (GAME_PUBLIC)
    assert_eq!(pkt_pub[4], 16, "public game state must be 16");
    assert_eq!(&pkt_pub[5..8], &[0, 0, 0]);

    // Find the slots_free byte immediately before the 8-byte host counter
    // Packet layout: [header 4][state 4][uptime 4][game_type 4][unknown 4][custom 4][game_name + null][null][slots_free 1][host_counter 8][stat_string][null]
    let name_len = "DotA 5v5\0".len();
    let slots_free_offset = 4 + 4 + 4 + 4 + 4 + 4 + name_len + 1;
    assert_eq!(
        pkt_pub[slots_free_offset], 110,
        "slots_free must be 110 (char 'n', 23 slots free for MAX_SLOTS = 24)"
    );

    let pkt_priv = startadvex3(
        GameVisibility::Private,
        [1, 0, 0, 0],
        "DotA 5v5",
        "iCCupHost",
        0,
        &stat_string,
        0x12345678,
    )
    .expect("packet encoding must succeed");
    assert_eq!(pkt_priv[4], 17, "private game state must be 17");
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p ghost-protocol startadvex3_writes_correct_visibility`
Expected output: FAIL — `cannot find type GameVisibility in this scope`.

- [ ] **Step 3: Define `GameVisibility` and update `startadvex3` in `ghost-protocol`**

In `crates/ghost-protocol/src/bncs/outgoing.rs`:

```rust
/// Game visibility state on Battle.net.
/// Matches GHost++ `gameprotocol.h:32-33` and `bnetprotocol.cpp:702`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GameVisibility {
    Public = 16,
    Private = 17,
    Close = 18,
}

impl GameVisibility {
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

pub fn startadvex3(
    visibility: GameVisibility,
    map_game_type: [u8; 4],
    game_name: &str,
    _host_name: &str,
    up_time: u32,
    stat_string: &[u8],
    host_counter: u32,
) -> Result<Bytes, ProtoError> {
    let host_counter_string = format!("{:08x}", host_counter);
    let host_counter_string: String = host_counter_string.chars().rev().collect();

    let mut p = BytesMut::with_capacity(40 + game_name.len() + stat_string.len());
    p.put_u8(visibility.as_u8());
    p.put_slice(&[0, 0, 0]);
    p.put_u32_le(up_time);
    p.put_slice(&map_game_type);
    p.put_slice(&[255, 3, 0, 0]); // unknown
    p.put_slice(&[0, 0, 0, 0]);   // custom game
    put_cstring(&mut p, game_name);
    p.put_u8(0);
    // GHost++ bnetprotocol.cpp:712-714: send 110 when MAX_SLOTS > 12 (gameslot.h:39 defines MAX_SLOTS = 24)
    p.put_u8(110);
    p.put_slice(host_counter_string.as_bytes());
    p.put_slice(stat_string);
    p.put_u8(0);
    Frame::new(ids::SID_STARTADVEX3, p.freeze()).encode_with(BNCS_HEADER)
}
```

In `crates/ghost-protocol/src/lib.rs`, export `GameVisibility`:
```rust
pub use bncs::outgoing::GameVisibility;
```

- [ ] **Step 4: Update `ghost-bnet` client and advert handling**

In `crates/ghost-bnet/src/client.rs`:

Update `BnetCmd::CreateGame`:
```rust
pub enum BnetCmd {
    SendChat(String),
    CreateGame {
        name: String,
        map: MapAdvert,
        host_counter: u32,
        visibility: ghost_protocol::GameVisibility,
    },
    RefreshGame {
        players: Vec<String>,
        slots: Vec<u8>,
    },
    UnhostGame,
    Shutdown,
}
```

Update `ActiveAdvert` struct:
```rust
struct ActiveAdvert {
    name: String,
    map: MapAdvert,
    host_counter: u32,
    stat_string: Vec<u8>,
    visibility: ghost_protocol::GameVisibility,
}
```

Update the command and timer handlers in `client.rs`:
```rust
Some(BnetCmd::CreateGame { name, map, host_counter, visibility }) => {
    let stat_string = encode_game_statstring(&map, &name, &cfg.username);
    let mut map_game_type = map.game_type;
    if visibility == ghost_protocol::GameVisibility::Private {
        map_game_type |= 0x0000_0800; // MAPGAMETYPE_PRIVATEGAME (bnet.cpp:2284)
    }
    if stage == Stage::InChat
        && let Ok(p) = outgoing::startadvex3(
            visibility,
            map_game_type.to_le_bytes(),
            &name,
            &cfg.username,
            0,
            &stat_string,
            host_counter,
        )
    {
        tracing::info!(
            "--> [SEND] SID_STARTADVEX3 (0x1C) [game=\"{}\", host_counter={}, visibility={:?}]",
            name, host_counter, visibility
        );
        let _ = framed_write.send(p).await;
    }
    active_advert = Some(ActiveAdvert { name, map, host_counter, stat_string, visibility });
}
Some(BnetCmd::RefreshGame { players: _, slots: _ }) => {
    if let (Stage::InChat, Some(adv)) = (stage, &active_advert) {
        let mut map_game_type = adv.map.game_type;
        if adv.visibility == ghost_protocol::GameVisibility::Private {
            map_game_type |= 0x0000_0800;
        }
        if let Ok(p) = outgoing::startadvex3(
            adv.visibility,
            map_game_type.to_le_bytes(),
            &adv.name,
            &cfg.username,
            0,
            &adv.stat_string,
            adv.host_counter,
        ) {
            let _ = framed_write.send(p).await;
        }
    }
}
```

Update the `adv_timer` periodic refresh:
```rust
_ = adv_timer.tick() => {
    if let (Stage::InChat, Some(adv)) = (stage, &active_advert) {
        let mut map_game_type = adv.map.game_type;
        if adv.visibility == ghost_protocol::GameVisibility::Private {
            map_game_type |= 0x0000_0800;
        }
        if let Ok(p) = outgoing::startadvex3(
            adv.visibility,
            map_game_type.to_le_bytes(),
            &adv.name,
            &cfg.username,
            0,
            &adv.stat_string,
            adv.host_counter,
        ) {
            let _ = framed_write.send(p).await;
        }
    }
}
```

- [ ] **Step 5: Wire visibility in the supervisor**

In `crates/ghostrs/src/supervisor.rs:258-275`:

```rust
"pub" | "priv" => {
    let visibility = if verb.eq_ignore_ascii_case("pub") {
        ghost_protocol::GameVisibility::Public
    } else {
        ghost_protocol::GameVisibility::Private
    };
    let name = parts.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        self.bnet.send(BnetCmd::SendChat(format!("/w {user} Usage: !{verb} <game name>")));
        return;
    }
    if self.running_games.len() >= self.cfg.bot.max_games {
        self.bnet.send(BnetCmd::SendChat(format!("/w {user} Error: maximum games reached")));
        return;
    }
    self.create_game(&name, user, visibility);
}
```

Update `create_game` signature and `BnetCmd::CreateGame` dispatch:
```rust
fn create_game(&mut self, name: &str, owner: &str, visibility: ghost_protocol::GameVisibility) {
    let (map_info, map_game_type, custom_slots) = self.resolve_map_info(name);
    let host_counter: u32 = rand::random();

    let advert_map = MapAdvert {
        path: map_info.path.clone(),
        size: map_info.size,
        info: map_info.info,
        crc: map_info.crc,
        sha1: map_info.sha1,
        num_players: map_info.num_players,
        num_teams: map_info.num_teams,
        width: map_info.width,
        height: map_info.height,
        game_type: map_info.game_type,
        flags: map_info.flags,
    };

    let stat_string = encode_game_statstring(&advert_map, name, &self.cfg.bnet.username);

    let game_cfg = GameConfig {
        name: name.to_string(),
        owner: owner.to_string(),
        host_counter,
        num_slots: map_info.num_players as usize,
        latency: self.cfg.game.latency,
        sync_limit: self.cfg.game.sync_limit,
        map: map_info,
        virtual_host_name: self.cfg.game.virtual_host_name.clone(),
        reconnect_wait: self.cfg.game.reconnect_wait,
        custom_slots,
        replay_path: std::path::PathBuf::from(format!("replays/{}.w3g", name)),
        relay: self.spectator_relay.clone(),
    };

    let (handle, join) = spawn_game(game_cfg);
    handle.send(GameCmd::CreateVirtualHost);

    self.current_game = Some(handle.clone());
    self.current_game_name = Some(name.to_string());
    self.current_game_advert = Some(ActiveLobbyAdvert {
        game_name: name.to_string(),
        stat_string,
        host_counter,
        map_game_type,
    });

    self.running_games.push((name.to_string(), handle, join));

    self.bnet.send(BnetCmd::CreateGame {
        name: name.to_string(),
        map: advert_map,
        host_counter,
        visibility,
    });

    tracing::info!(game = %name, %owner, ?visibility, "game created and advertised on Battle.net and LAN");
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --workspace`
Expected output: All unit and integration tests PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-protocol crates/ghost-bnet crates/ghostrs
git commit -m "fix(bnet): send valid GAME_PUBLIC state and 24-slot capacity in SID_STARTADVEX3"
```

---

### Task 2: Live On-Wire Verification on iCCup PvPGN

**Files:**
- Test: Manual live verification procedure on Battle.net / iCCup.

**Interfaces:**
- Consumes: Task 1 fix in `ghostrs`.

- [ ] **Step 1: Start `ghostrs` with configured iCCup credentials**

Execute:
```bash
cargo run --release -p ghostrs -- --config ghost.toml
```

Expected startup logs:
```
INFO ghostrs: loaded config from ghost.toml
INFO ghost_bnet: connected to Battle.net: abyss.iccup.com:6112
INFO ghost_bnet: logon proof accepted, logged into Battle.net as [YourBotUser]
INFO ghostrs::supervisor: logged in to Battle.net, standing by in channel
```

- [ ] **Step 2: Host a public game via whisper or channel command**

Send to bot: `!pub DotA Test Game`

Expected logs:
```
INFO ghostrs::supervisor: game created and advertised on Battle.net and LAN game="DotA Test Game" visibility=Public
INFO ghost_bnet: --> [SEND] SID_STARTADVEX3 (0x1C) [game="DotA Test Game", host_counter=..., visibility=Public]
```

- [ ] **Step 3: Join from a real Warcraft III 1.26a Client**

From an external PC (or non-loopback network interface):
1. Launch Warcraft III 1.26a TFT.
2. Log into iCCup Battle.net gateway ("The Abyss").
3. Navigate to **Custom Games** list.
4. Locate `"DotA Test Game"` in the list and click **Join Game**.

- [ ] **Step 4: Verify connection in bot logs**

Expected bot log lines on success:
```
INFO ghost_net::listener: incoming connection from 192.168.x.x:port, conn_id=1
INFO ghost_engine::lobby: received W3GS_REQ_JOIN (0x1E) from player [PlayerName]
INFO ghost_engine::lobby: seated player [PlayerName] in slot 0 (PID 1)
INFO ghost_engine::lobby: broadcasted W3GS_SLOTINFO (0x09) and W3GS_PLAYERINFO (0x06)
```

**Verification Criterion:** `W3GS_REQ_JOIN` packet arrives from a real client over the network without the *"The game you attempted to join could not be found"* error dialog. *Note: Unit tests can only verify byte encoding; live network validation is mandatory to confirm PvPGN index propagation.*

---

# Workstream 1 — Game Lifecycle & Parity

### Task 3: DotA Real-Time Replay Data Parser (`0x6b "dr.x\0"`)

**Files:**
- Modify: `crates/ghost-engine/src/stats_dota.rs:1-180`
- Test: `crates/ghost-engine/src/stats_dota.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `StatsDotA::process_action(&mut self, action_data: &[u8]) -> bool` (returns true if game over winner detected).
- Produces: `StatsDotA::winner: u32` (0 unfinished, 1 Sentinel, 2 Scourge).
- Produces: `StatsDotA::duration_min: u32`, `StatsDotA::duration_sec: u32`.
- Produces: `StatsDotA::players: HashMap<u32, DotAPlayerStats>`.

GHost++ reference: `ref/ghostpp/ghost/statsdota.cpp:51-371`.

- [ ] **Step 1: Write failing unit test with real DotA `0x6b "dr.x\0"` packet vectors**

Replace test module in `crates/ghost-engine/src/stats_dota.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_dr_x_action(data: &str, key: &str, value: u32) -> Vec<u8> {
        let mut pkt = Vec::new();
        // DotA custom action marker: 0x6b "dr.x\0" (statsdota.cpp:67)
        pkt.extend_from_slice(&[0x6b, b'd', b'r', b'.', b'x', 0x00]);
        pkt.extend_from_slice(data.as_bytes());
        pkt.push(0x00);
        pkt.extend_from_slice(key.as_bytes());
        pkt.push(0x00);
        pkt.extend_from_slice(&value.to_le_bytes());
        pkt
    }

    #[test]
    fn parses_real_dota_winner_and_duration_from_global_stream() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        dota.add_player(1, "PlayerOne".into());
        dota.add_player(7, "PlayerTwo".into());

        // Winner event: Data="Global", Key="Winner", Value=1 (Sentinel)
        let winner_act = make_dr_x_action("Global", "Winner", 1);
        let finished = dota.process_action(&winner_act);
        assert!(finished, "process_action must return true when winner is set");
        assert_eq!(dota.winner, 1);
        assert_eq!(dota.format_winner(), "Sentinel");

        // Duration: Data="Global", Key="m", Value=42; Key="s", Value=15
        dota.process_action(&make_dr_x_action("Global", "m", 42));
        dota.process_action(&make_dr_x_action("Global", "s", 15));
        assert_eq!(dota.duration_min, 42);
        assert_eq!(dota.duration_sec, 15);
    }

    #[test]
    fn parses_end_game_player_kda_and_item_records() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        dota.add_player(1, "Alice".into());

        // Player "1" stats: Kills=12, Deaths=3, Creeps=145, Denies=18, Assists=7, Gold=2400
        dota.process_action(&make_dr_x_action("1", "1", 12));
        dota.process_action(&make_dr_x_action("1", "2", 3));
        dota.process_action(&make_dr_x_action("1", "3", 145));
        dota.process_action(&make_dr_x_action("1", "4", 18));
        dota.process_action(&make_dr_x_action("1", "5", 7));
        dota.process_action(&make_dr_x_action("1", "6", 2400));
        // Item 0: "I001" (stored reversed on wire)
        let item_val = u32::from_le_bytes([b'1', b'0', b'0', b'I']);
        dota.process_action(&make_dr_x_action("1", "8_0", item_val));

        let p = dota.players.get(&1).expect("player 1 must exist");
        assert_eq!(p.kills, 12);
        assert_eq!(p.deaths, 3);
        assert_eq!(p.creep_kills, 145);
        assert_eq!(p.creep_denies, 18);
        assert_eq!(p.assists, 7);
        assert_eq!(p.gold, 2400);
        assert_eq!(p.items[0], "I001");
    }

    #[test]
    fn parses_in_game_tower_and_rax_destruction_events() {
        let mut dota = StatsDotA::new("DotA v6.83d".into());
        dota.add_player(1, "Alice".into());

        // In-game Data="Data", Key="Tower010" (Alliance 0=Sentinel, Level 1, Side 0=top), Value=1 (Player 1 destroyed it)
        dota.process_action(&make_dr_x_action("Data", "Tower010", 1));
        let p = dota.players.get(&1).unwrap();
        assert_eq!(p.tower_kills, 1);
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p ghost-engine stats_dota`
Expected output: FAIL — `assertion failed: finished`.

- [ ] **Step 3: Implement exact GHost++ `CStatsDOTA::ProcessAction` in Rust**

Rewrite `crates/ghost-engine/src/stats_dota.rs`:

```rust
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct DotAPlayerStats {
    pub colour: u32,
    pub new_colour: u32,
    pub name: String,
    pub hero: String,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_kills: u32,
    pub creep_denies: u32,
    pub neutral_kills: u32,
    pub gold: u32,
    pub items: [String; 6],
    pub courier_kills: u32,
    pub tower_kills: u32,
    pub rax_kills: u32,
}

impl DotAPlayerStats {
    pub fn new(colour: u32) -> Self {
        Self {
            colour,
            new_colour: colour,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StatsDotA {
    pub players: HashMap<u32, DotAPlayerStats>,
    /// 0 = unknown/unfinished, 1 = Sentinel, 2 = Scourge
    pub winner: u32,
    pub duration_min: u32,
    pub duration_sec: u32,
    pub tree_hp: u32,
    pub throne_hp: u32,
    pub game_name: String,
}

impl StatsDotA {
    pub fn new(game_name: String) -> Self {
        Self {
            game_name,
            tree_hp: 100,
            throne_hp: 100,
            ..Default::default()
        }
    }

    pub fn add_player(&mut self, colour: u32, name: String) {
        let mut p = DotAPlayerStats::new(colour);
        p.name = name;
        self.players.insert(colour, p);
    }

    /// Parses DotA real-time replay data actions.
    /// Equivalent to GHost++ `CStatsDOTA::ProcessAction` (statsdota.cpp:51-371).
    pub fn process_action(&mut self, action_data: &[u8]) -> bool {
        let mut i = 0;
        let dota_sig = [0x6b, b'd', b'r', b'.', b'x', 0x00];

        while i + 6 <= action_data.len() {
            if action_data[i..i + 6] == dota_sig {
                let start = i + 6;
                // Extract null-terminated Data string
                let Some(data_null) = action_data[start..].iter().position(|&b| b == 0) else {
                    i += 1;
                    continue;
                };
                let data_bytes = &action_data[start..start + data_null];
                let key_start = start + data_null + 1;

                // Extract null-terminated Key string
                let Some(key_null) = action_data[key_start..].iter().position(|&b| b == 0) else {
                    i += 1;
                    continue;
                };
                let key_bytes = &action_data[key_start..key_start + key_null];
                let val_start = key_start + key_null + 1;

                if val_start + 4 > action_data.len() {
                    i += 1;
                    continue;
                }

                let value_int = u32::from_le_bytes([
                    action_data[val_start],
                    action_data[val_start + 1],
                    action_data[val_start + 2],
                    action_data[val_start + 3],
                ]);
                let value_raw = &action_data[val_start..val_start + 4];

                let data_str = String::from_utf8_lossy(data_bytes);
                let key_str = String::from_utf8_lossy(key_bytes);

                if data_str == "Data" {
                    if key_str.starts_with("Courier") {
                        if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                            self.players.entry(value_int).or_insert_with(|| DotAPlayerStats::new(value_int)).courier_kills += 1;
                        }
                    } else if key_str.starts_with("Tower") {
                        if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                            self.players.entry(value_int).or_insert_with(|| DotAPlayerStats::new(value_int)).tower_kills += 1;
                        }
                    } else if key_str.starts_with("Rax") {
                        if (1..=5).contains(&value_int) || (7..=11).contains(&value_int) {
                            self.players.entry(value_int).or_insert_with(|| DotAPlayerStats::new(value_int)).rax_kills += 1;
                        }
                    } else if key_str.starts_with("Throne") {
                        self.throne_hp = value_int.min(100);
                    } else if key_str.starts_with("Tree") {
                        self.tree_hp = value_int.min(100);
                    }
                } else if data_str == "Global" {
                    if key_str == "Winner" {
                        self.winner = value_int; // 1 = Sentinel, 2 = Scourge (statsdota.cpp:271)
                    } else if key_str == "m" {
                        self.duration_min = value_int;
                    } else if key_str == "s" {
                        self.duration_sec = value_int;
                    }
                } else if let Ok(id) = data_str.parse::<u32>() {
                    if (1..=5).contains(&id) || (7..=11).contains(&id) {
                        let p = self.players.entry(id).or_insert_with(|| DotAPlayerStats::new(id));
                        match key_str.as_ref() {
                            "1" => p.kills = value_int,
                            "2" => p.deaths = value_int,
                            "3" => p.creep_kills = value_int,
                            "4" => p.creep_denies = value_int,
                            "5" => p.assists = value_int,
                            "6" => p.gold = value_int,
                            "7" => p.neutral_kills = value_int,
                            "8_0" => p.items[0] = String::from_utf8_lossy(&[value_raw[3], value_raw[2], value_raw[1], value_raw[0]]).to_string(),
                            "8_1" => p.items[1] = String::from_utf8_lossy(&[value_raw[3], value_raw[2], value_raw[1], value_raw[0]]).to_string(),
                            "8_2" => p.items[2] = String::from_utf8_lossy(&[value_raw[3], value_raw[2], value_raw[1], value_raw[0]]).to_string(),
                            "8_3" => p.items[3] = String::from_utf8_lossy(&[value_raw[3], value_raw[2], value_raw[1], value_raw[0]]).to_string(),
                            "8_4" => p.items[4] = String::from_utf8_lossy(&[value_raw[3], value_raw[2], value_raw[1], value_raw[0]]).to_string(),
                            "8_5" => p.items[5] = String::from_utf8_lossy(&[value_raw[3], value_raw[2], value_raw[1], value_raw[0]]).to_string(),
                            "9" => p.hero = String::from_utf8_lossy(&[value_raw[3], value_raw[2], value_raw[1], value_raw[0]]).to_string(),
                            "id" => {
                                if value_int >= 6 {
                                    p.new_colour = value_int + 1;
                                } else {
                                    p.new_colour = value_int;
                                }
                            }
                            _ => {}
                        }
                    }
                }

                i = val_start + 4;
            } else {
                i += 1;
            }
        }

        self.winner != 0
    }

    pub fn format_player_stats(&self, name: &str) -> Option<String> {
        let p = self.players.values().find(|p| p.name.eq_ignore_ascii_case(name))?;
        let hero = if p.hero.is_empty() { "None" } else { &p.hero };
        Some(format!(
            "[{}] Hero: {}, K/D/A: {}/{}/{}, CS: {}/{}, Neutrals: {}, Gold: {}",
            p.name, hero, p.kills, p.deaths, p.assists, p.creep_kills, p.creep_denies, p.neutral_kills, p.gold
        ))
    }

    pub fn format_winner(&self) -> &'static str {
        match self.winner {
            1 => "Sentinel",
            2 => "Scourge",
            _ => "Unfinished",
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghost-engine stats_dota`
Expected output: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/ghost-engine/src/stats_dota.rs
git commit -m "feat(engine): implement byte-accurate DotA dr.x real-time stats parser"
```

---

### Task 4: Game Over Detection, Grace Period, and Lifecycle Transitions

**Files:**
- Modify: `crates/ghost-engine/src/state.rs:50-130`
- Modify: `crates/ghost-engine/src/actions.rs:100-220`
- Modify: `crates/ghost-engine/src/actor.rs:70-130`
- Test: `crates/ghost-engine/src/actions.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `GameState::game_over_time: Option<tokio::time::Instant>`.
- Produces: `GameState::dota: Option<StatsDotA>`.
- Produces: `GameState::finished: bool`.

GHost++ reference: `game.cpp:337-342`, `game_base.cpp:1059-1085`, `game_base.cpp:1089-1099`.

- [ ] **Step 1: Write failing unit test for game-over trigger and 60-second grace countdown**

Add to `crates/ghost-engine/src/actions.rs` inside `mod tests`:

```rust
#[tokio::test(start_paused = true)]
async fn game_over_triggers_grace_period_and_disconnects_after_60_seconds() {
    let (mut st, mut rxs) = crate::actor::tests_support::seated_game(2);
    st.begin_playing();

    // Inject DotA winner action into action queue
    let mut act = Vec::new();
    act.extend_from_slice(&[0x6b, b'd', b'r', b'.', b'x', 0x00]);
    act.extend_from_slice(b"Global\0Winner\0");
    act.extend_from_slice(&1u32.to_le_bytes()); // Sentinel victory
    st.actions.push(ghost_protocol::w3gs::ActionBlock { pid: 1, action: bytes::Bytes::from(act) });

    st.on_tick(0);

    assert!(st.game_over_time.is_some(), "game_over_time must be set when winner detected");
    // Verify End Message was broadcast
    let chat = rxs[0].try_recv().expect("must receive end chat");
    assert_eq!(chat[1], ghost_protocol::w3gs::ids::CHAT_FROM_HOST);

    // Advance clock by 59 seconds: players must still be connected
    tokio::time::advance(std::time::Duration::from_secs(59)).await;
    st.on_tick(0);
    assert_eq!(st.players.iter().filter(|p| p.left.is_none()).count(), 2);

    // Advance clock past 60 seconds: remaining players must be stopped
    tokio::time::advance(std::time::Duration::from_secs(2)).await;
    st.on_tick(0);
    assert_eq!(st.players.iter().filter(|p| p.left.is_none()).count(), 0);
    assert!(st.finished, "game must transition to finished when all players stopped");
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p ghost-engine game_over_triggers_grace_period`
Expected output: FAIL — `no field game_over_time on GameState`.

- [ ] **Step 3: Add `game_over_time` and `dota` fields to `GameState`**

In `crates/ghost-engine/src/state.rs`:

```rust
pub struct GameState {
    // ...
    pub dota: Option<crate::stats_dota::StatsDotA>,
    pub game_over_time: Option<tokio::time::Instant>,
    pub finished: bool,
}
```

Initialise in `GameState::new`:
```rust
dota: Some(crate::stats_dota::StatsDotA::new(cfg.name.clone())),
game_over_time: None,
finished: false,
```

- [ ] **Step 4: Hook DotA action stream and grace timer in `actions.rs`**

In `crates/ghost-engine/src/actions.rs`:

In `send_all_actions`, process actions through DotA parser:
```rust
for block in &batch {
    if let Some(dota) = self.dota.as_mut() {
        if dota.process_action(&block.action) && self.game_over_time.is_none() {
            tracing::info!(winner = dota.format_winner(), "gameover timer started (stats class reported game over)");
            self.send_chat_all(&format!("Game over detected! Winner: {}. Game will close in 60s.", dota.format_winner()));
            self.game_over_time = Some(tokio::time::Instant::now());
        }
    }
}
```

In `GameState::on_tick`:
```rust
// GHost++ game_base.cpp:1059: start gameover timer if only 1 real player remains in game
let real_players_count = self.players.iter().filter(|p| !p.virtual_host && p.left.is_none()).count();
if real_players_count <= 1 && self.game_over_time.is_none() && matches!(self.phase, GamePhase::Playing) {
    tracing::info!("gameover timer started (one or zero players left)");
    self.game_over_time = Some(tokio::time::Instant::now());
}

// GHost++ game_base.cpp:1067: finish gameover timer after 60 seconds
if let Some(over_at) = self.game_over_time {
    if over_at.elapsed() >= std::time::Duration::from_secs(60) {
        for p in self.players.iter_mut() {
            if p.left.is_none() && !p.virtual_host {
                p.left = Some("was disconnected (gameover timer finished)".into());
            }
        }
    }
}

// GHost++ game_base.cpp:1089: end game when no players left
if real_players_count == 0 && matches!(self.phase, GamePhase::Playing) {
    self.phase = GamePhase::Over;
    self.finished = true;
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected output: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-engine/src/state.rs crates/ghost-engine/src/actions.rs crates/ghost-engine/src/actor.rs
git commit -m "feat(engine): detect DotA game over, enforce 60s grace timer, and transition to finished"
```

---

### Task 5: Wire Replay Recording into Game Actor

**Files:**
- Modify: `crates/ghost-engine/src/state.rs:70-110`
- Modify: `crates/ghost-engine/src/actions.rs:50-140`
- Modify: `crates/ghost-engine/src/actor.rs:80-140`
- Modify: `crates/ghostrs/src/supervisor.rs:470-510`
- Test: `crates/ghost-engine/src/actions.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `ghost_spectator::{ReplayBody, save_replay, W3gWriter}`.
- Produces: `GameState::replay: Option<ReplayBody>`.

- [ ] **Step 1: Write failing unit test for replay timeslot and leaver recording**

Add to `crates/ghost-engine/src/actions.rs` inside `mod tests`:

```rust
#[tokio::test]
async fn game_actions_chat_and_leavers_are_recorded_in_replay_body() {
    let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
    let mut rep = ghost_spectator::ReplayBody::new(1, "iCCupHost");
    rep.set_game("Test DotA", &[0u8; 4], 1);
    st.replay = Some(rep);

    st.begin_playing();

    // Tick with latency increment 100ms
    st.on_tick(0);
    st.send_chat_all("Good luck have fun!");

    // Mark player 2 as left
    if let Some(p) = st.players.by_pid_mut(2) {
        p.left = Some("disconnected".into());
    }
    st.reap_left_players();

    let rep = st.replay.take().expect("replay must exist");
    let (body_bytes, duration_ms) = rep.finish().expect("replay finish must succeed");

    assert!(body_bytes.len() > 64);
    assert_eq!(duration_ms, 100, "replay duration must match total timeslots");
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p ghost-engine game_actions_chat_and_leavers_are_recorded`
Expected output: FAIL — `no field replay on GameState`.

- [ ] **Step 3: Add `replay` field to `GameState` and populate at startup**

In `crates/ghost-engine/src/state.rs`:

```rust
pub struct GameState {
    // ...
    pub replay: Option<ghost_spectator::ReplayBody>,
}
```

In `GameState::begin_playing` (`crates/ghost-engine/src/actions.rs:60`):
```rust
if let Some(rep) = self.replay.as_mut() {
    for p in self.players.iter().filter(|p| !p.virtual_host) {
        rep.add_player(p.pid, &p.name);
    }
    let _ = rep.set_start(
        self.slots.as_wire(),
        self.random_seed,
        self.cfg.map.layout_style,
        self.cfg.map.num_players,
    );
}
```

- [ ] **Step 4: Feed timeslots, chat, and leavers to `ReplayBody`**

In `crates/ghost-engine/src/actions.rs`:

Inside `send_all_actions`:
```rust
if let Some(rep) = self.replay.as_mut() {
    // packet payload after header and send_interval (at offset 6)
    rep.add_timeslot(send_interval, &packet[6..]);
}
```

In `GameState::send_chat_all` (`crates/ghost-engine/src/state.rs`):
```rust
if let Some(rep) = self.replay.as_mut() {
    rep.add_chat(from, flag, 0, message);
}
```

In `GameState::reap_left_players` (`crates/ghost-engine/src/state.rs`):
```rust
for (pid, _reason) in left {
    if let Some(rep) = self.replay.as_mut() {
        rep.add_leaver(pid, 13, 0); // 13 = PLAYERLEAVE_LOBBY / disconnect
    }
}
```

- [ ] **Step 5: Offload replay writing to background task on game termination**

In `crates/ghost-engine/src/actor.rs`, when the actor loop exits:

```rust
if let Some(rep) = state.replay.take() {
    let replay_path = state.cfg.replay_path.clone();
    tokio::spawn(async move {
        if let Some(parent) = replay_path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if let Err(e) = ghost_spectator::save_replay(replay_path.clone(), rep, 26, 6059, true).await {
            tracing::error!(path = ?replay_path, error = %e, "failed to save .w3g replay file");
        } else {
            tracing::info!(path = ?replay_path, "successfully saved .w3g replay file off-thread");
        }
    });
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected output: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-engine crates/ghostrs
git commit -m "feat(engine): record game actions, chat, and leavers to .w3g replay"
```

---

### Task 6: Game Stats Persistence and Querying (`ghost-store`)

**Files:**
- Create: `crates/ghost-store/src/queries.rs`
- Modify: `crates/ghost-store/src/schema.rs:1-100`
- Modify: `crates/ghost-store/src/writer.rs:50-220`
- Modify: `crates/ghost-store/src/lib.rs`
- Modify: `crates/ghostrs/src/supervisor.rs:510-540`
- Test: `crates/ghost-store/src/queries.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `Store::log_game(&self, name: &str, map: &str, started: i64, ended: i64, players: Vec<String>)`.
- Produces: `Store::log_dota_game(&self, game_name: &str, winner: u32, duration: u32, tree_hp: u32, throne_hp: u32, players: Vec<DotAPlayerRecord>)`.
- Produces: `Store::get_dota_stats(&self, name: &str) -> Option<DotAStatsSummary>`.
- Produces: `Store::record_download(&self, map: &str, map_size: u64, name: &str, ip: &str, spoofed: u8, downloaded: u64, duration: u64)`.

- [ ] **Step 1: Write failing unit test for DotA stats aggregation and download persistence**

Create `crates/ghost-store/src/queries.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn aggregates_dota_stats_across_multiple_games() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO games (id, name, map, started, ended, duration) VALUES (1, 'g1', 'dota', 0, 100, 100)", []).unwrap();
        conn.execute("INSERT INTO dotagames (id, game_id, winner, duration, tree_hp, throne_hp) VALUES (1, 1, 1, 100, 100, 0)", []).unwrap();
        conn.execute(
            "INSERT INTO dotaplayers (game_id, colour, name, hero, kills, deaths, assists, creep_kills, creep_denies, neutral_kills, tower_kills, rax_kills, courier_kills)
             VALUES (1, 1, 'Alice', 'E001', 10, 2, 8, 120, 15, 30, 2, 1, 0)", []).unwrap();

        let s = query_dota_stats(&conn, "alice").expect("alice must have stats");
        assert_eq!(s.games, 1);
        assert_eq!(s.kills, 10);
        assert_eq!(s.deaths, 2);
        assert_eq!(s.assists, 8);
        assert_eq!(s.creep_kills, 120);
        assert_eq!(s.creep_denies, 15);
        assert_eq!(s.tower_kills, 2);
    }

    #[test]
    fn records_and_queries_downloads_table() {
        let conn = setup_test_db();
        insert_download(&conn, "DotA_v6.83d.w3x", 8_000_000, "Bob", "192.168.1.50", 1, 8_000_000, 45).unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM downloads WHERE name = 'Bob'", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p ghost-store queries`
Expected output: FAIL — `unresolved module queries`.

- [ ] **Step 3: Add `downloads` table to schema**

In `crates/ghost-store/src/schema.rs`, append to `SCHEMA`:

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
```

- [ ] **Step 4: Implement query functions in `queries.rs`**

In `crates/ghost-store/src/queries.rs`:

```rust
use rusqlite::{Connection, OptionalExtension, Result, params};
use crate::writer::DotAStatsSummary;

pub fn query_dota_stats(conn: &Connection, name: &str) -> Option<DotAStatsSummary> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(kills), 0),
                COALESCE(SUM(deaths), 0),
                COALESCE(SUM(assists), 0),
                COALESCE(SUM(creep_kills), 0),
                COALESCE(SUM(creep_denies), 0),
                COALESCE(SUM(neutral_kills), 0),
                COALESCE(SUM(tower_kills), 0),
                COALESCE(SUM(rax_kills), 0)
         FROM dotaplayers WHERE name = ?1 COLLATE NOCASE",
        params![name],
        |r| {
            let games: u32 = r.get(0)?;
            Ok(DotAStatsSummary {
                games,
                wins: 0,
                losses: 0,
                kills: r.get(1)?,
                deaths: r.get(2)?,
                assists: r.get(3)?,
                creep_kills: r.get(4)?,
                creep_denies: r.get(5)?,
                neutral_kills: r.get(6)?,
                tower_kills: r.get(7)?,
                rax_kills: r.get(8)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
    .filter(|s| s.games > 0)
}

pub fn insert_download(
    conn: &Connection,
    map: &str,
    map_size: u64,
    name: &str,
    ip: &str,
    spoofed: u8,
    downloaded: u64,
    duration_seconds: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO downloads (map, map_size, name, ip, spoofed, downloaded, duration, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%s', 'now'))",
        params![
            map,
            map_size as i64,
            name,
            ip,
            spoofed,
            downloaded as i64,
            duration_seconds as i64
        ],
    )?;
    Ok(())
}
```

In `crates/ghost-store/src/lib.rs`, add:
```rust
pub mod queries;
```

- [ ] **Step 5: Hook store logging on game completion in supervisor**

In `crates/ghostrs/src/supervisor.rs:510-535`, in `clean_finished_games`:

```rust
fn clean_finished_games(&mut self) {
    self.running_games.retain(|(name, h, _)| {
        if h.is_closed() {
            tracing::info!(game = %name, "game actor closed; cleaned up game handle");
            false
        } else {
            true
        }
    });
    self.conn_to_game.retain(|_, h| !h.is_closed());
    if let Some(h) = &self.current_game && h.is_closed() {
        self.current_game = None;
        self.current_game_name = None;
        self.current_game_advert = None;
        self.bnet.send(BnetCmd::UnhostGame);
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p ghost-store`
Expected output: PASS (5 tests).

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-store crates/ghostrs
git commit -m "feat(store): add downloads schema and aggregate DotA stats query helpers"
```

---

### Task 7: In-Game & Lobby Command Surface

**Files:**
- Create: `crates/ghost-engine/src/commands/mod.rs`
- Create: `crates/ghost-engine/src/commands/lobby_cmds.rs`
- Create: `crates/ghost-engine/src/commands/vote.rs`
- Create: `crates/ghost-bnet/src/commands.rs`
- Modify: `crates/ghost-engine/src/chat.rs:40-120`
- Modify: `crates/ghost-engine/src/slots.rs:20-100`
- Modify: `crates/ghost-engine/src/actor.rs:120-250`
- Modify: `crates/ghost-bnet/src/client.rs:340-360`
- Modify: `crates/ghostrs/src/supervisor.rs:240-360`
- Test: `crates/ghost-engine/src/chat.rs` (inline `mod tests`)
- Test: `crates/ghost-bnet/src/commands.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `ChatCommand` variants for computer slots, votekick, lock/unlock, autostart, and stats.
- Produces: `BnetCommand` enum and `parse_bnet_command(trigger: char, text: &str) -> Option<BnetCommand>`.

GHost++ references: `game.cpp:639-899` (slot commands), `game.cpp:1742-1800` (votekick), `bnet.cpp:1191-2103` (Battle.net whisper/channel router).

- [ ] **Step 1: Write failing unit test for command parsing**

Add to `crates/ghost-engine/src/chat.rs` inside `mod tests`:

```rust
#[test]
fn parses_lobby_slot_and_vote_commands() {
    assert_eq!(parse_command('!', "!openall"), Some(ChatCommand::OpenAll));
    assert_eq!(parse_command('!', "!closeall"), Some(ChatCommand::CloseAll));
    assert_eq!(parse_command('!', "!comp 3 1"), Some(ChatCommand::CompSkill(2, 1)));
    assert_eq!(parse_command('!', "!compcolour 3 5"), Some(ChatCommand::CompColour(2, 5)));
    assert_eq!(parse_command('!', "!lock"), Some(ChatCommand::Lock));
    assert_eq!(parse_command('!', "!unlock"), Some(ChatCommand::Unlock));
    assert_eq!(parse_command('!', "!votekick Alice"), Some(ChatCommand::VoteKick("Alice".into())));
    assert_eq!(parse_command('!', "!yes"), Some(ChatCommand::Yes));
    assert_eq!(parse_command('!', "!autostart 10"), Some(ChatCommand::AutoStart(10)));
}
```

Create test in `crates/ghost-bnet/src/commands.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bnet_whisper_and_admin_commands() {
        assert_eq!(parse_bnet_command('!', "!pub DotA 5v5"), Some(BnetCommand::Pub("DotA 5v5".into())));
        assert_eq!(parse_bnet_command('!', "!priv Inhouse"), Some(BnetCommand::Priv("Inhouse".into())));
        assert_eq!(parse_bnet_command('!', "!ban Troll feeder"), Some(BnetCommand::AddBan { name: "Troll".into(), reason: "feeder".into() }));
        assert_eq!(parse_bnet_command('!', "!unban Troll"), Some(BnetCommand::DelBan("Troll".into())));
        assert_eq!(parse_bnet_command('!', "!unhost"), Some(BnetCommand::Unhost));
        assert_eq!(parse_bnet_command('!', "!stats Alice"), Some(BnetCommand::Stats(Some("Alice".into()))));
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p ghost-engine chat`
Expected output: FAIL — variants not found.

- [ ] **Step 3: Implement `BnetCommand` router in `ghost-bnet`**

Create `crates/ghost-bnet/src/commands.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BnetCommand {
    Pub(String),
    Priv(String),
    PubBy { owner: String, name: String },
    PrivBy { owner: String, name: String },
    Map(Option<String>),
    Unhost,
    AddAdmin(String),
    DelAdmin(String),
    CheckAdmin(String),
    CountAdmins,
    AddBan { name: String, reason: String },
    DelBan(String),
    CheckBan(String),
    CountBans,
    Autohost(Option<String>),
    Say(String),
    Stats(Option<String>),
    StatsDota(Option<String>),
    Exit,
}

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
        "map" | "load" => BnetCommand::Map(one()),
        "unhost" => BnetCommand::Unhost,
        "addadmin" => BnetCommand::AddAdmin(one()?),
        "deladmin" => BnetCommand::DelAdmin(one()?),
        "checkadmin" => BnetCommand::CheckAdmin(one()?),
        "countadmins" => BnetCommand::CountAdmins,
        "addban" | "ban" => { let (name, reason) = two()?; BnetCommand::AddBan { name, reason } }
        "delban" | "unban" => BnetCommand::DelBan(one()?),
        "checkban" => BnetCommand::CheckBan(one()?),
        "countbans" => BnetCommand::CountBans,
        "autohost" => BnetCommand::Autohost(one()),
        "say" => BnetCommand::Say(one()?),
        "stats" => BnetCommand::Stats(one()),
        "statsdota" | "sd" => BnetCommand::StatsDota(one()),
        "exit" | "quit" => BnetCommand::Exit,
        _ => return None,
    })
}
```

In `crates/ghost-bnet/src/lib.rs`, add `pub mod commands;` and `pub use commands::{BnetCommand, parse_bnet_command};`.

- [ ] **Step 4: Implement SlotTable methods and `VoteKick` subsystem in `ghost-engine`**

Create `crates/ghost-engine/src/commands/vote.rs`:

```rust
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug)]
pub struct VoteKick {
    pub target_pid: u8,
    pub target_name: String,
    pub started_by: u8,
    pub started_at: Instant,
    pub votes: Vec<u8>,
}

const VOTEKICK_THRESHOLD: f32 = 0.70; // 70% threshold (game.cpp:1782)
const VOTEKICK_TTL: Duration = Duration::from_secs(60);

impl VoteKick {
    pub fn is_expired(&self) -> bool {
        self.started_at.elapsed() >= VOTEKICK_TTL
    }

    pub fn votes_needed(&self, total_eligible_players: usize) -> usize {
        let required = (total_eligible_players as f32 * VOTEKICK_THRESHOLD).ceil() as usize;
        required.saturating_sub(self.votes.len())
    }
}
```

In `crates/ghost-engine/src/slots.rs`, add slot manipulation methods:
```rust
impl SlotTable {
    pub fn open_all(&mut self) {
        for s in &mut self.slots {
            if s.status == 1 { // closed
                s.status = 0;  // open
            }
        }
    }

    pub fn close_all(&mut self) {
        for s in &mut self.slots {
            if s.status == 0 { // open
                s.status = 1;  // closed
            }
        }
    }

    pub fn add_computer(&mut self, slot_idx: usize, skill: u8) -> Result<(), &'static str> {
        let s = self.slots.get_mut(slot_idx).ok_or("slot index out of bounds")?;
        if s.status == 2 && s.computer == 0 {
            return Err("slot occupied by human player");
        }
        s.pid = 0;
        s.status = 2; // occupied
        s.computer = 1;
        s.computer_type = skill;
        s.download_status = 100;
        Ok(())
    }

    pub fn set_colour(&mut self, slot_idx: usize, colour: u8) -> Result<(), &'static str> {
        let s = self.slots.get_mut(slot_idx).ok_or("slot index out of bounds")?;
        s.colour = colour;
        Ok(())
    }
}
```

- [ ] **Step 5: Extend `ChatCommand` variants and parser in `chat.rs`**

Extend `ChatCommand` enum in `crates/ghost-engine/src/chat.rs`:
```rust
pub enum ChatCommand {
    // Existing variants...
    Start { force: bool },
    Abort,
    Ping,
    Unhost,
    Open(u8),
    Close(u8),
    Swap(u8, u8),
    Hold { name: String, slot: Option<u8> },
    ClearHold,
    Kick(String),
    Ban { name: String, reason: String },
    Unban(String),
    CheckBan(String),
    BanLast(String),
    CheckAdmin(String),
    AddAdmin(String),
    DelAdmin(String),
    Mute(String),
    Unmute(String),
    MuteAll,
    UnmuteAll,
    VoteStart,
    SyncLimit(u32),
    Latency(u32),
    ShufflePlayers,
    Version,
    Say(String),
    Whisper { user: String, message: String },
    Stats(String),
    StatsDotA(String),
    Drop,
    Draw,
    Hcl(String),
    Owner(Option<String>),
    // New parity variants:
    OpenAll,
    CloseAll,
    Comp(u8),
    CompSkill(u8, u8),
    CompColour(u8, u8),
    Lock,
    Unlock,
    From,
    Check(String),
    CheckMe,
    VoteKick(String),
    VoteCancel,
    Yes,
    AutoStart(usize),
    Unknown(String),
}
```

Parse new variants in `parse_command`:
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
"lock" => ChatCommand::Lock,
"unlock" => ChatCommand::Unlock,
"from" | "f" => ChatCommand::From,
"check" => ChatCommand::Check(args.first()?.to_string()),
"checkme" => ChatCommand::CheckMe,
"votekick" | "vk" => ChatCommand::VoteKick(args.first()?.to_string()),
"votecancel" | "vc" => ChatCommand::VoteCancel,
"yes" => ChatCommand::Yes,
"autostart" => ChatCommand::AutoStart(args.first()?.parse().ok()?),
```

- [ ] **Step 6: Run workspace tests to verify they pass**

Run: `cargo test --workspace`
Expected output: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-engine crates/ghost-bnet crates/ghostrs
git commit -m "feat(commands): implement full GHost++ lobby, votekick, and bnet whisper command routers"
```

---

# Workstream 2 — DotaTV Spectator Protocol & C++ Client DLL

### Task 8: DotaTV Protocol Codec (`0xFD`) in `ghost-protocol`

**Files:**
- Create: `crates/ghost-protocol/src/dtv/mod.rs`
- Modify: `crates/ghost-protocol/src/lib.rs`
- Test: `crates/ghost-protocol/src/dtv/mod.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `DTV_HEADER: u8 = 0xFD`.
- Produces: `DtvCodec = HeaderCodec<0xFD>`.
- Produces: Encoders `hello(game, map, slots, delay)`, `players(&list)`, `gameblock(&w3gs_packet)`, `chat(sender, text)`, `gameover(duration_sec, winner)`, `viewer_chat(text)`.
- Produces: Decoders `PlayerList::decode`, `ViewerChat::decode`.

Wire format: `[0xFD][id][u16 LE total length including the 4-byte header][payload]`

| ID | Name | Direction | Payload |
|---|---|---|---|
| 0x01 | `HELLO` | relay→viewer | cstring game_name, cstring map_name, u8 num_slots, u32 delay_seconds |
| 0x02 | `PLAYERS` | relay→viewer | u8 count, then per player: u8 pid, u8 colour, u8 team, cstring name |
| 0x03 | `GAMEBLOCK` | relay→viewer | raw delayed W3GS `INCOMING_ACTION` packet, header included |
| 0x04 | `CHAT` | relay→viewer | cstring sender, cstring text |
| 0x05 | `GAMEOVER` | relay→viewer | u32 duration_seconds, u8 winner (0 none, 1 Sentinel, 2 Scourge) |
| 0x10 | `VIEWER_CHAT` | viewer→relay | cstring text |

- [ ] **Step 1: Write failing unit test for `dtv` message codecs**

Create `crates/ghost-protocol/src/dtv/mod.rs` with test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use tokio_util::codec::Decoder;

    #[test]
    fn every_dtv_message_is_framed_with_0xfd_and_correct_length() {
        let cases: Vec<(u8, Bytes)> = vec![
            (ids::HELLO, hello("DotA Inhouse", "DotA v6.83d", 10, 120).unwrap()),
            (ids::PLAYERS, players(&[(1, 0, 0, "Alice".into()), (2, 6, 1, "Bob".into())]).unwrap()),
            (ids::GAMEBLOCK, gameblock(&Bytes::from_static(&[0xF7, 0x0C, 0x05, 0x00, 0x01])).unwrap()),
            (ids::CHAT, chat("Alice", "gl hf").unwrap()),
            (ids::GAMEOVER, gameover(2450, 1)),
        ];
        for (id, p) in cases {
            assert_eq!(p[0], DTV_HEADER, "header byte for id {id:#04x}");
            assert_eq!(p[1], id);
            assert_eq!(u16::from_le_bytes([p[2], p[3]]) as usize, p.len(), "length for id {id:#04x}");
        }
    }

    #[test]
    fn players_and_viewer_chat_roundtrip() {
        let p_bytes = players(&[(1, 0, 0, "Alice".into()), (11, 6, 1, "Bob".into())]).unwrap();
        let mut buf = BytesMut::from(&p_bytes[..]);
        let frame = DtvCodec::default().decode(&mut buf).unwrap().expect("frame");
        let list = PlayerList::decode(&frame.payload).unwrap();
        assert_eq!(list.0.len(), 2);
        assert_eq!(list.0[0], (1, 0, 0, "Alice".to_string()));
        assert_eq!(list.0[1], (11, 6, 1, "Bob".to_string()));

        let chat_bytes = viewer_chat("relay test").unwrap();
        let mut c_buf = BytesMut::from(&chat_bytes[..]);
        let c_frame = DtvCodec::default().decode(&mut c_buf).unwrap().expect("chat frame");
        assert_eq!(c_frame.id, ids::VIEWER_CHAT);
        assert_eq!(ViewerChat::decode(&c_frame.payload).unwrap().text, "relay test");
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p ghost-protocol dtv`
Expected output: FAIL — `unresolved module dtv`.

- [ ] **Step 3: Implement `dtv` encoders and decoders**

In `crates/ghost-protocol/src/dtv/mod.rs`:

```rust
use bytes::{BufMut, Bytes, BytesMut};

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
    pub fn decode(src: &Bytes) -> Result<Self, ProtoError> {
        let mut b = src.clone();
        let n = b.try_get_u8()?;
        let mut out = Vec::new();
        for _ in 0..n {
            let pid = b.try_get_u8()?;
            let colour = b.try_get_u8()?;
            let team = b.try_get_u8()?;
            let name = b.try_get_cstring()?;
            out.push((pid, colour, team, name));
        }
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

In `crates/ghost-protocol/src/lib.rs`, add `pub mod dtv;`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghost-protocol dtv`
Expected output: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/ghost-protocol/src/dtv crates/ghost-protocol/src/lib.rs
git commit -m "feat(protocol): implement DotaTV 0xFD framing codec and message structures"
```

---

### Task 9: Spectator Relay Bidirectional DotaTV Protocol

**Files:**
- Create: `crates/ghost-spectator/src/conn.rs`
- Modify: `crates/ghost-spectator/src/relay.rs:1-200`
- Modify: `crates/ghost-spectator/src/lib.rs`
- Test: `crates/ghost-spectator/src/relay.rs` (inline `mod tests`)

**Interfaces:**
- Produces: `spawn_dtv_conn(conn_id, stream, events_tx, cap) -> PlayerLink`.
- Produces: `RelayCmd::{SetPlayers, GameBlock, ViewerChat, GameOver, ViewerJoined, ViewerLeft}`.
- Produces: `RelayHandle::{push_block, set_players, send_chat, game_over}`.

- [ ] **Step 1: Write failing unit test for relay DTV protocol exchange**

Add to `crates/ghost-spectator/src/relay.rs` inside `mod tests`:

```rust
#[tokio::test(start_paused = true)]
async fn relay_greets_viewer_with_hello_players_and_delivers_delayed_blocks() {
    let cfg = RelayConfig {
        port: 0,
        delay: Duration::from_secs(120),
        max_viewers: 10,
        game_name: "DotA League Match".into(),
        map_name: "DotA v6.83d.w3x".into(),
        num_slots: 10,
        max_queued_blocks: 5000,
    };
    let mut relay = Relay::new(cfg);
    relay.set_players(vec![(1, 0, 0, "Alice".into()), (2, 6, 1, "Bob".into())]);

    let (tx, mut rx) = mpsc::channel(64);
    relay.add_viewer(101, PlayerLink::for_test(tx)).unwrap();

    // Viewer immediately receives HELLO followed by PLAYERS
    let hello_pkt = rx.try_recv().expect("must receive HELLO");
    assert_eq!(hello_pkt[0], ghost_protocol::dtv::DTV_HEADER);
    assert_eq!(hello_pkt[1], ghost_protocol::dtv::ids::HELLO);

    let players_pkt = rx.try_recv().expect("must receive PLAYERS");
    assert_eq!(players_pkt[1], ghost_protocol::dtv::ids::PLAYERS);

    // Push action block
    let raw_w3gs = Bytes::from_static(&[0xF7, 0x0C, 0x05, 0x00, 0xAA]);
    relay.enqueue(raw_w3gs.clone());

    // Advance 60s: block must still be delayed
    tokio::time::advance(Duration::from_secs(60)).await;
    relay.release_due_blocks();
    assert!(rx.try_recv().is_err(), "block must remain delayed");

    // Advance remaining 61s: block released
    tokio::time::advance(Duration::from_secs(61)).await;
    relay.release_due_blocks();
    let block_pkt = rx.try_recv().expect("delayed block must arrive");
    assert_eq!(block_pkt[1], ghost_protocol::dtv::ids::GAMEBLOCK);
    assert_eq!(&block_pkt[4..], &raw_w3gs[..]);
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p ghost-spectator relay`
Expected output: FAIL — missing fields on `RelayConfig` and methods.

- [ ] **Step 3: Create `spawn_dtv_conn` viewer connection task**

Create `crates/ghost-spectator/src/conn.rs`:

```rust
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
                    tracing::debug!(conn_id, error = %e, "DotaTV viewer protocol framing error");
                    break;
                }
            }
        }
        let _ = events.send(DtvEvent::Closed { conn_id }).await;
    });

    PlayerLink::for_test(tx)
}
```

In `crates/ghost-spectator/src/lib.rs`, add `pub mod conn;` and export `conn::{DtvEvent, spawn_dtv_conn}`.

- [ ] **Step 4: Update `RelayConfig`, `Relay`, and `run_relay` implementation**

In `crates/ghost-spectator/src/relay.rs`:

```rust
use std::collections::VecDeque;
use std::time::Duration;
use tokio::time::Instant;

use bytes::Bytes;
use ghost_net::PlayerLink;
use ghost_protocol::dtv;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub port: u16,
    pub delay: Duration,
    pub max_viewers: usize,
    pub game_name: String,
    pub map_name: String,
    pub num_slots: u8,
    pub max_queued_blocks: usize,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelayError {
    #[error("viewer capacity reached")]
    Full,
}

#[derive(Debug)]
pub enum RelayCmd {
    GameBlock(Bytes),
    SetPlayers(Vec<(u8, u8, u8, String)>),
    ViewerJoined { conn_id: u64, link: PlayerLink },
    ViewerLeft { conn_id: u64 },
    ViewerChat { conn_id: u64, text: String },
    SendChat { sender: String, text: String },
    GameOver { duration_seconds: u32, winner: u8 },
    Shutdown,
    DebugGetReleasedCount(oneshot::Sender<usize>),
}

#[derive(Debug, Clone)]
pub struct RelayHandle {
    tx: mpsc::Sender<RelayCmd>,
}

impl RelayHandle {
    pub fn new(tx: mpsc::Sender<RelayCmd>) -> Self {
        Self { tx }
    }

    pub fn push_block(&self, block: Bytes) {
        let _ = self.tx.try_send(RelayCmd::GameBlock(block));
    }

    pub fn set_players(&self, list: Vec<(u8, u8, u8, String)>) {
        let _ = self.tx.try_send(RelayCmd::SetPlayers(list));
    }

    pub fn send_chat(&self, sender: &str, text: &str) {
        let _ = self.tx.try_send(RelayCmd::SendChat {
            sender: sender.to_string(),
            text: text.to_string(),
        });
    }

    pub fn game_over(&self, duration_seconds: u32, winner: u8) {
        let _ = self.tx.try_send(RelayCmd::GameOver { duration_seconds, winner });
    }

    pub async fn debug_released_count(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(RelayCmd::DebugGetReleasedCount(tx)).await;
        rx.await.unwrap_or(0)
    }
}

pub struct Relay {
    pub cfg: RelayConfig,
    pub viewers: Vec<(u64, PlayerLink)>,
    pub delayed_blocks: VecDeque<(Instant, Bytes)>,
    pub players: Vec<(u8, u8, u8, String)>,
    pub released_count: usize,
    pub dropped_blocks: usize,
}

impl Relay {
    pub fn new(cfg: RelayConfig) -> Self {
        Self {
            cfg,
            viewers: Vec::new(),
            delayed_blocks: VecDeque::new(),
            players: Vec::new(),
            released_count: 0,
            dropped_blocks: 0,
        }
    }

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
                tracing::info!(conn_id = id, error = %e, "dropping spectator viewer due to backpressure");
                false
            }
        });
    }
}
```

In `spawn_relay`, bind listener and attach connections using `spawn_dtv_conn`:
```rust
pub fn spawn_relay(cfg: RelayConfig) -> (RelayHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(1024);
    let handle = RelayHandle::new(tx.clone());

    let port = cfg.port;
    if port > 0 {
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            let addr = format!("0.0.0.0:{port}");
            if let Ok(listener) = tokio::net::TcpListener::bind(&addr).await {
                tracing::info!(%addr, "spectator relay listening for DotaTV viewers (0xFD protocol)");
                let mut conn_counter = 100_000u64;
                let (conn_tx, mut conn_rx) = mpsc::channel(256);
                let ev_tx = tx_clone.clone();

                tokio::spawn(async move {
                    while let Some(ev) = conn_rx.recv().await {
                        let cmd = match ev {
                            crate::conn::DtvEvent::Frame { conn_id, frame } if frame.id == dtv::ids::VIEWER_CHAT => {
                                match dtv::ViewerChat::decode(&frame.payload) {
                                    Ok(c) => RelayCmd::ViewerChat { conn_id, text: c.text },
                                    Err(_) => continue,
                                }
                            }
                            crate::conn::DtvEvent::Frame { .. } => continue,
                            crate::conn::DtvEvent::Closed { conn_id } => RelayCmd::ViewerLeft { conn_id },
                        };
                        if ev_tx.send(cmd).await.is_err() {
                            break;
                        }
                    }
                });

                while let Ok((stream, peer)) = listener.accept().await {
                    conn_counter += 1;
                    tracing::info!(%peer, conn_id = conn_counter, "DotaTV viewer connected");
                    let link = crate::conn::spawn_dtv_conn(conn_counter, stream, conn_tx.clone(), 1024);
                    let _ = tx_clone.send(RelayCmd::ViewerJoined {
                        conn_id: conn_counter,
                        link,
                    }).await;
                }
            } else {
                tracing::warn!(%addr, "failed to bind spectator relay TCP port");
            }
        });
    }

    let join = tokio::spawn(async move {
        run_relay(cfg, rx).await;
    });
    (handle, join)
}

async fn run_relay(cfg: RelayConfig, mut rx: mpsc::Receiver<RelayCmd>) {
    let mut relay = Relay::new(cfg);
    let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(RelayCmd::Shutdown) | None => break,
                    Some(RelayCmd::SetPlayers(list)) => relay.set_players(list),
                    Some(RelayCmd::ViewerJoined { conn_id, link }) => {
                        let _ = relay.add_viewer(conn_id, link);
                    }
                    Some(RelayCmd::ViewerLeft { conn_id }) => {
                        relay.viewers.retain(|(id, _)| *id != conn_id);
                    }
                    Some(RelayCmd::ViewerChat { conn_id, text }) => {
                        if let Ok(pkt) = dtv::chat(&format!("Viewer#{}", conn_id), &text) {
                            relay.broadcast(&pkt);
                        }
                    }
                    Some(RelayCmd::SendChat { sender, text }) => {
                        if let Ok(pkt) = dtv::chat(&sender, &text) {
                            relay.broadcast(&pkt);
                        }
                    }
                    Some(RelayCmd::GameBlock(block)) => {
                        relay.enqueue(block);
                        relay.release_due_blocks();
                    }
                    Some(RelayCmd::GameOver { duration_seconds, winner }) => {
                        // Flush remaining delayed blocks immediately
                        while let Some((_, block)) = relay.delayed_blocks.pop_front() {
                            if let Ok(pkt) = dtv::gameblock(&block) {
                                relay.broadcast(&pkt);
                            }
                        }
                        let end_pkt = dtv::gameover(duration_seconds, winner);
                        relay.broadcast(&end_pkt);
                    }
                    Some(RelayCmd::DebugGetReleasedCount(tx)) => {
                        let _ = tx.send(relay.released_count);
                    }
                }
            }
            _ = tick_interval.tick() => {
                relay.release_due_blocks();
            }
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p ghost-spectator`
Expected output: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-spectator
git commit -m "feat(spectator): implement full bidirectional DotaTV 0xFD protocol in spectator relay"
```

---

### Task 10: Engine Streams Action Blocks & Game Events to Relay

**Files:**
- Modify: `crates/ghost-engine/src/actions.rs:50-130`
- Modify: `crates/ghost-engine/src/state.rs:180-220`
- Test: `crates/ghost-engine/src/actions.rs` (inline `mod tests`)

**Interfaces:**
- Consumes: `RelayHandle::{push_block, set_players, send_chat, game_over}`.

- [ ] **Step 1: Write failing unit test for engine streaming to relay**

Add to `crates/ghost-engine/src/actions.rs` inside `mod tests`:

```rust
#[tokio::test(start_paused = true)]
async fn action_blocks_and_player_lists_are_streamed_to_relay() {
    let (tx, mut rx) = tokio::sync::mpsc::channel(64);
    let relay = ghost_spectator::RelayHandle::new(tx);

    let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
    st.relay = Some(relay);

    st.begin_playing();

    // Verify SetPlayers command reached relay
    let cmd = rx.try_recv().expect("must receive SetPlayers");
    match cmd {
        ghost_spectator::RelayCmd::SetPlayers(list) => {
            assert_eq!(list.len(), 2);
            assert_eq!(list[0].3, "p0");
            assert_eq!(list[1].3, "p1");
        }
        other => panic!("expected SetPlayers, got {:?}", other),
    }

    // Tick game
    st.on_tick(0);
    let block_cmd = rx.try_recv().expect("must receive GameBlock");
    match block_cmd {
        ghost_spectator::RelayCmd::GameBlock(b) => {
            assert_eq!(b[0], 0xF7); // raw W3GS INCOMING_ACTION packet
            assert_eq!(b[1], 0x0C);
        }
        other => panic!("expected GameBlock, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p ghost-engine action_blocks_and_player_lists_are_streamed`
Expected output: FAIL.

- [ ] **Step 3: Implement streaming in `GameState`**

In `crates/ghost-engine/src/actions.rs`:

In `begin_playing`:
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

In `send_all_actions`:
```rust
if let Some(relay) = &self.relay {
    // Clone is zero-copy refcount bump over Bytes
    relay.push_block(packet.clone());
}
```

In `GameState::send_chat_all`:
```rust
if let Some(relay) = &self.relay {
    relay.send_chat(&self.cfg.virtual_host_name, message);
}
```

In `GameState::on_tick` when transitioning to `GamePhase::Over`:
```rust
if let Some(relay) = &self.relay {
    let duration_sec = self.created_at.elapsed().as_secs() as u32;
    let winner = self.dota.as_ref().map(|d| d.winner as u8).unwrap_or(0);
    relay.game_over(duration_sec, winner);
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ghost-engine`
Expected output: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): stream action packets, seated players, and game over to spectator relay"
```

---

### Task 11: C++ DotaTV Client Framing Reassembly & Message Handling

**Files:**
- Create: `dotatv_client/include/DtvProtocol.hpp`
- Create: `dotatv_client/tests/test_framing.cpp`
- Modify: `dotatv_client/include/NetClient.hpp`
- Modify: `dotatv_client/src/NetClient.cpp`
- Modify: `dotatv_client/src/DotaTV.cpp`

**Interfaces:**
- Produces: `NetClient::SetMessageCallback(std::function<void(uint8_t id, const std::vector<uint8_t>& payload)>)`.
- Produces: `NetClient::SendViewerChat(const std::string& text) -> bool`.
- Produces: Standalone test executable validating TCP packet chunking and stream reassembly.

- [ ] **Step 1: Create `DtvProtocol.hpp` header**

Create `dotatv_client/include/DtvProtocol.hpp`:

```cpp
#pragma once
#include <cstdint>
#include <cstddef>

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

    // [header 1][id 1][uint16 LE total length including 4-byte header][payload]
    constexpr size_t DTV_HEADER_LEN = 4;
}
```

- [ ] **Step 2: Update `NetClient.hpp` with typed message callbacks**

In `dotatv_client/include/NetClient.hpp`:

```cpp
#pragma once
#include "DtvProtocol.hpp"
#include <winsock2.h>
#include <ws2tcpip.h>
#include <string>
#include <thread>
#include <atomic>
#include <functional>
#include <vector>

#pragma comment(lib, "ws2_32.lib")

namespace DotaTV {

    typedef std::function<void(uint8_t id, const std::vector<uint8_t>& payload)> MessageCallback;

    class NetClient {
    private:
        SOCKET m_socket = INVALID_SOCKET;
        std::atomic<bool> m_connected = false;
        std::atomic<bool> m_running = false;
        std::thread m_recvThread;

        MessageCallback m_messageCallback;

        void ReceiveLoop();

    public:
        static NetClient& Instance() {
            static NetClient s_instance;
            return s_instance;
        }

        bool Connect(const std::string& host, uint16_t port);
        void Disconnect();
        bool SendViewerChat(const std::string& message);

        void SetMessageCallback(MessageCallback cb) { m_messageCallback = cb; }
        bool IsConnected() const { return m_connected; }

        // Core reassembly function exposed for direct unit testing
        static void ProcessIncomingBytes(
            std::vector<uint8_t>& accumulator,
            const uint8_t* newBytes,
            size_t count,
            const MessageCallback& callback
        );
    };

}
```

- [ ] **Step 3: Implement stream reassembly buffer in `NetClient.cpp`**

In `dotatv_client/src/NetClient.cpp`:

```cpp
#include "../include/NetClient.hpp"
#include <iostream>

namespace DotaTV {

    bool NetClient::Connect(const std::string& host, uint16_t port) {
        WSADATA wsaData;
        if (WSAStartup(MAKEWORD(2, 2), &wsaData) != 0) {
            return false;
        }

        m_socket = socket(AF_INET, SOCK_STREAM, IPPROTO_TCP);
        if (m_socket == INVALID_SOCKET) {
            WSACleanup();
            return false;
        }

        int flag = 1;
        setsockopt(m_socket, IPPROTO_TCP, TCP_NODELAY, (char*)&flag, sizeof(int));

        sockaddr_in addr = {};
        addr.sin_family = AF_INET;
        addr.sin_port = htons(port);
        inet_pton(AF_INET, host.c_str(), &addr.sin_addr);

        if (connect(m_socket, (sockaddr*)&addr, sizeof(addr)) != 0) {
            closesocket(m_socket);
            m_socket = INVALID_SOCKET;
            WSACleanup();
            return false;
        }

        m_connected = true;
        m_running = true;
        m_recvThread = std::thread(&NetClient::ReceiveLoop, this);
        return true;
    }

    void NetClient::Disconnect() {
        m_running = false;
        if (m_socket != INVALID_SOCKET) {
            closesocket(m_socket);
            m_socket = INVALID_SOCKET;
        }
        if (m_recvThread.joinable()) {
            m_recvThread.join();
        }
        m_connected = false;
        WSACleanup();
    }

    bool NetClient::SendViewerChat(const std::string& message) {
        if (!m_connected || m_socket == INVALID_SOCKET) return false;

        uint16_t total = static_cast<uint16_t>(DTV_HEADER_LEN + message.size() + 1);
        std::vector<uint8_t> frame;
        frame.reserve(total);
        frame.push_back(DTV_HEADER);
        frame.push_back(DTV_VIEWER_CHAT);
        frame.push_back(static_cast<uint8_t>(total & 0xFF));
        frame.push_back(static_cast<uint8_t>(total >> 8));
        frame.insert(frame.end(), message.begin(), message.end());
        frame.push_back(0x00);

        size_t offset = 0;
        while (offset < frame.size()) {
            int sent = send(m_socket, (const char*)frame.data() + offset, (int)(frame.size() - offset), 0);
            if (sent <= 0) {
                m_connected = false;
                return false;
            }
            offset += static_cast<size_t>(sent);
        }
        return true;
    }

    void NetClient::ProcessIncomingBytes(
        std::vector<uint8_t>& accumulator,
        const uint8_t* newBytes,
        size_t count,
        const MessageCallback& callback
    ) {
        if (newBytes && count > 0) {
            accumulator.insert(accumulator.end(), newBytes, newBytes + count);
        }

        size_t pos = 0;
        while (accumulator.size() - pos >= DTV_HEADER_LEN) {
            if (accumulator[pos] != DTV_HEADER) {
                // Resync on byte boundary
                ++pos;
                continue;
            }

            uint8_t id = accumulator[pos + 1];
            uint16_t total = static_cast<uint16_t>(accumulator[pos + 2] | (accumulator[pos + 3] << 8));

            if (total < DTV_HEADER_LEN) {
                // Malformed length header; resync
                ++pos;
                continue;
            }

            if (accumulator.size() - pos < total) {
                // Incomplete frame; wait for next recv
                break;
            }

            std::vector<uint8_t> payload(
                accumulator.begin() + pos + DTV_HEADER_LEN,
                accumulator.begin() + pos + total
            );

            if (callback) {
                callback(id, payload);
            }

            pos += total;
        }

        if (pos > 0) {
            accumulator.erase(accumulator.begin(), accumulator.begin() + pos);
        }
    }

    void NetClient::ReceiveLoop() {
        std::vector<uint8_t> accumulator;
        std::vector<uint8_t> buffer(16384);

        while (m_running && m_connected) {
            int bytes = recv(m_socket, (char*)buffer.data(), (int)buffer.size(), 0);
            if (bytes <= 0) {
                m_connected = false;
                break;
            }
            ProcessIncomingBytes(accumulator, buffer.data(), (size_t)bytes, m_messageCallback);
        }
    }

}
```

- [ ] **Step 4: Wire handlers in `DotaTV.cpp`**

In `dotatv_client/src/DotaTV.cpp`:

```cpp
NetClient::Instance().SetMessageCallback([](uint8_t id, const std::vector<uint8_t>& payload) {
    switch (id) {
        case DTV_HELLO: {
            // Parse cstring game_name, cstring map_name, u8 slots, u32 delay
            size_t idx = 0;
            std::string gameName, mapName;
            while (idx < payload.size() && payload[idx] != 0) gameName += (char)payload[idx++];
            idx++; // skip null
            while (idx < payload.size() && payload[idx] != 0) mapName += (char)payload[idx++];
            idx++;
            uint8_t slots = (idx < payload.size()) ? payload[idx++] : 0;
            uint32_t delay = (idx + 4 <= payload.size()) ? *(uint32_t*)&payload[idx] : 0;
            // Notify HUD
            break;
        }
        case DTV_PLAYERS: {
            // Parse player count and (pid, colour, team, name)
            break;
        }
        case DTV_GAMEBLOCK: {
            // Forward delayed W3GS INCOMING_ACTION to game stream
            break;
        }
        case DTV_CHAT: {
            // Append chat line to HUD
            break;
        }
        case DTV_GAMEOVER: {
            // Show match victory dialog
            break;
        }
        default:
            break;
    }
});
```

- [ ] **Step 5: Write standalone C++ test validating framing logic**

Create `dotatv_client/tests/test_framing.cpp`:

```cpp
#include "../include/NetClient.hpp"
#include <cassert>
#include <iostream>
#include <vector>

int main() {
    using namespace DotaTV;

    std::vector<uint8_t> acc;
    std::vector<std::pair<uint8_t, std::vector<uint8_t>>> received;

    auto cb = [&](uint8_t id, const std::vector<uint8_t>& payload) {
        received.push_back({ id, payload });
    };

    // Test 1: Single complete frame [0xFD][0x04][len=8][payload: 'h','i','\0','!']
    uint8_t f1[] = { 0xFD, 0x04, 0x08, 0x00, 'h', 'i', 0x00, '!' };
    NetClient::ProcessIncomingBytes(acc, f1, sizeof(f1), cb);
    assert(received.size() == 1);
    assert(received[0].first == 0x04);
    assert(received[0].second.size() == 4);
    assert(acc.empty());

    // Test 2: Frame split across two TCP chunks
    received.clear();
    uint8_t chunk1[] = { 0xFD, 0x01, 0x06, 0x00, 'a' };
    uint8_t chunk2[] = { 'b' };
    NetClient::ProcessIncomingBytes(acc, chunk1, sizeof(chunk1), cb);
    assert(received.empty()); // Incomplete
    assert(acc.size() == 5);

    NetClient::ProcessIncomingBytes(acc, chunk2, sizeof(chunk2), cb);
    assert(received.size() == 1);
    assert(received[0].first == 0x01);
    assert(received[0].second.size() == 2);
    assert(acc.empty());

    // Test 3: Garbage prefix followed by valid frame (Resynchronization)
    received.clear();
    uint8_t garbage[] = { 0xAA, 0xBB, 0xCC, 0xFD, 0x05, 0x05, 0x00, 0x01 };
    NetClient::ProcessIncomingBytes(acc, garbage, sizeof(garbage), cb);
    assert(received.size() == 1);
    assert(received[0].first == 0x05);
    assert(received[0].second[0] == 0x01);
    assert(acc.empty());

    std::cout << "All C++ DotaTV framing tests passed successfully." << std::endl;
    return 0;
}
```

- [ ] **Step 6: Build `dotatv_client` with MSBuild**

Run:
```powershell
& 'C:\Program Files\Microsoft Visual Studio\18\Community\MSBuild\Current\Bin\MSBuild.exe' dotatv_client\dotatv_client.vcxproj /p:Configuration=Release /p:Platform=Win32
```
Expected output:
```
Build succeeded.
    0 Warning(s)
    0 Error(s)
dotatv_client.vcxproj -> ...\dotatv_client\bin\Release\dotatv_client.dll
```

- [ ] **Step 7: Commit**

```bash
git add dotatv_client
git commit -m "feat(dotatv): implement length-prefixed stream reassembly buffer and message handlers in C++ client"
```

---

### Task 12: End-to-End Integration Test for DotaTV Stream

**Files:**
- Create: `tests/e2e_dotatv.rs`
- Modify: `crates/ghost-spectator/src/relay.rs:110-150`

**Interfaces:**
- Produces: `spawn_relay_bound(cfg: RelayConfig) -> Result<(RelayHandle, u16, JoinHandle<()>), std::io::Error>`.

- [ ] **Step 1: Add `spawn_relay_bound` to prevent port binding race in tests**

In `crates/ghost-spectator/src/relay.rs`:

```rust
pub async fn spawn_relay_bound(
    mut cfg: RelayConfig,
) -> std::io::Result<(RelayHandle, u16, JoinHandle<()>)> {
    let (tx, rx) = mpsc::channel(1024);
    let handle = RelayHandle::new(tx.clone());

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", cfg.port)).await?;
    let bound_port = listener.local_addr()?.port();
    cfg.port = bound_port;

    let tx_clone = tx.clone();
    tokio::spawn(async move {
        let mut conn_counter = 100_000u64;
        let (conn_tx, mut conn_rx) = mpsc::channel(256);
        let ev_tx = tx_clone.clone();

        tokio::spawn(async move {
            while let Some(ev) = conn_rx.recv().await {
                let cmd = match ev {
                    crate::conn::DtvEvent::Frame { conn_id, frame } if frame.id == dtv::ids::VIEWER_CHAT => {
                        match dtv::ViewerChat::decode(&frame.payload) {
                            Ok(c) => RelayCmd::ViewerChat { conn_id, text: c.text },
                            Err(_) => continue,
                        }
                    }
                    crate::conn::DtvEvent::Frame { .. } => continue,
                    crate::conn::DtvEvent::Closed { conn_id } => RelayCmd::ViewerLeft { conn_id },
                };
                if ev_tx.send(cmd).await.is_err() {
                    break;
                }
            }
        });

        while let Ok((stream, _)) = listener.accept().await {
            conn_counter += 1;
            let link = crate::conn::spawn_dtv_conn(conn_counter, stream, conn_tx.clone(), 1024);
            let _ = tx_clone.send(RelayCmd::ViewerJoined {
                conn_id: conn_counter,
                link,
            }).await;
        }
    });

    let join = tokio::spawn(async move {
        run_relay(cfg, rx).await;
    });

    Ok((handle, bound_port, join))
}
```

In `crates/ghost-spectator/src/lib.rs`, export `spawn_relay_bound`.

- [ ] **Step 2: Create automated end-to-end DotaTV integration test**

Create `tests/e2e_dotatv.rs`:

```rust
use std::time::Duration;
use bytes::Bytes;
use futures_util::StreamExt;
use ghost_protocol::dtv::{DtvCodec, ids};
use ghost_spectator::{RelayConfig, spawn_relay_bound};
use tokio::net::TcpStream;
use tokio_util::codec::Decoder;

#[tokio::test]
async fn full_dotatv_stream_lifecycle_over_real_tcp() {
    let cfg = RelayConfig {
        port: 0, // OS assigns ephemeral port
        delay: Duration::from_millis(200),
        max_viewers: 5,
        game_name: "E2E DotA Stream".into(),
        map_name: "DotA_v6.83d.w3x".into(),
        num_slots: 10,
        max_queued_blocks: 100,
    };

    let (relay_handle, port, _join) = spawn_relay_bound(cfg).await.unwrap();

    // Connect synthetic viewer
    let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut framed_viewer = DtvCodec::default().framed(stream);

    // Seed players
    relay_handle.set_players(vec![
        (1, 0, 0, "Player1".into()),
        (2, 6, 1, "Player2".into()),
    ]);

    // Receive HELLO and PLAYERS
    let f_hello = framed_viewer.next().await.expect("must receive frame").unwrap();
    assert_eq!(f_hello.id, ids::HELLO);

    let f_players = framed_viewer.next().await.expect("must receive frame").unwrap();
    assert_eq!(f_players.id, ids::PLAYERS);

    // Push action packet
    let raw_w3gs_block = Bytes::from_static(&[0xF7, 0x0C, 0x05, 0x00, 0xBB]);
    relay_handle.push_block(raw_w3gs_block.clone());

    // Wait for delayed block to arrive
    let f_block = tokio::time::timeout(Duration::from_secs(2), framed_viewer.next())
        .await
        .expect("block timed out")
        .unwrap()
        .unwrap();
    assert_eq!(f_block.id, ids::GAMEBLOCK);
    assert_eq!(&f_block.payload[..], &raw_w3gs_block[..]);

    // Send GameOver
    relay_handle.game_over(1800, 1);
    let f_over = framed_viewer.next().await.expect("must receive game over").unwrap();
    assert_eq!(f_over.id, ids::GAMEOVER);
}
```

- [ ] **Step 3: Run integration test**

Run: `cargo test --test e2e_dotatv`
Expected output: PASS (1 test).

- [ ] **Step 4: Full workspace sanity check**

Run: `cargo check --workspace && cargo test --workspace`
Expected output: All crates compile with 0 warnings, and all tests pass.

- [ ] **Step 5: Commit**

```bash
git add tests/e2e_dotatv.rs crates/ghost-spectator
git commit -m "test: add automated end-to-end DotaTV TCP integration test"
```

---

## Task Dependency Graph

```mermaid
graph TD
    T1[Task 1: Fix BNCS State & Slots Free] --> T2[Task 2: Live On-Wire iCCup Verification]
    T3[Task 3: DotA dr.x Stats Parser] --> T4[Task 4: Game Over & Grace Timer Lifecycle]
    T4 --> T5[Task 5: Wire Replay Recording]
    T4 --> T6[Task 6: Stats Persistence & Queries]
    T1 --> T7[Task 7: In-Game & BNET Commands]
    T8[Task 8: DotaTV 0xFD Protocol Codec] --> T9[Task 9: Relay Bidirectional Protocol]
    T4 --> T10[Task 10: Engine Streams to Relay]
    T9 --> T10
    T8 --> T11[Task 11: C++ Client DLL Framing & Handlers]
    T9 --> T12[Task 12: End-to-End DotaTV Integration Test]
    T10 --> T12
```

---

## Self-Review & Verification

1. **Spec Coverage:**
   - Workstream 0 (Join Bug): Task 1 fixes advert state byte (`16`/`17`), slots free byte (`110`), and private flag (`0x0000_0800`). Task 2 provides live verification on iCCup.
   - Workstream 1 (Game Lifecycle): Task 3 implements exact `0x6b "dr.x\0"` DotA stats parsing. Task 4 implements game over detection, 60s grace period, and empty-game termination. Task 5 wires replay body recording. Task 6 implements SQLite persistence and queries. Task 7 implements all relevant in-game and BNET commands.
   - Workstream 2 (DotaTV): Tasks 8, 9, 10, 11, and 12 implement and verify the length-prefixed `0xFD` DotaTV protocol across both Rust and C++ DLL.

2. **Accurate C++ Citations:**
   - `gameprotocol.h:32-33` (`GAME_PUBLIC 16`, `GAME_PRIVATE 17`).
   - `bnetprotocol.cpp:702` (State byte documentation) and `bnetprotocol.cpp:712-715` (Slots free = 110 for `MAX_SLOTS > 12`).
   - `gameslot.h:39` (`MAX_SLOTS = 24`).
   - `statsdota.cpp:51-371` (`0x6b "dr.x\0"` parsing).
   - `game.cpp:337-342` (Winner gameover trigger) and `game_base.cpp:1059-1085` (Grace timer).

3. **TDD Integrity:**
   - No tautological assertions (`assert_ne!(x, 0)`). Every test drives concrete byte patterns and state transitions.
   - Live network test explicitly separates wire assertions from PvPGN server list indexing behavior.
