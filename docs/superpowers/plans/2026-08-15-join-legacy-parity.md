# Implementation Plan: Battle.net Custom Game Join Parity (ghostrs)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make a real Warcraft III 1.26a client able to join a ghostrs-hosted game from the Battle.net custom game list, by restoring three divergences from the last code version that was confirmed working live (`499b987~1`).

**Architecture:** The hostbot advertises custom games to Battle.net using `SID_STARTADVEX3` (0x1C) and announces listener endpoints via `SID_NETGAMEPORT` (0x45). This plan restores byte-accurate map game type flags (`MAPGAMETYPE_UNKNOWN0`), normalizes advertised map paths to the Warcraft III client convention `Maps\Download\<filename>`, and ensures operational port configuration alignment with diagnostic observability.

**Tech Stack:** Rust 2024 edition (rustc 1.96+), Tokio 1.45 (`tokio-util`), `ghost-protocol`, `ghost-bnet`, `ghostrs`.

---

## Global Constraints

- Rust 2024 edition workspace; `crates/ghost-bnet/src/lib.rs` carries
  `#![forbid(unsafe_code)]`. No `unsafe` in any Rust crate.
- No new external Rust dependencies; no new entries in any `Cargo.toml` `[dependencies]`.
- No `unwrap()`, `expect()`, or panicking indexing on data that came off the network.
  Parsers use checked accessors and return errors.
- Never block or sleep on an async task; use `tokio::time`.
- Never read, print, modify, or commit `ghost.toml` — it holds live credentials.
  It is also NOT in git; a `git restore`/`git checkout` over it destroys the operator's
  configuration permanently. Plans must never instruct anyone to run a command that
  reverts, checks out, or cleans that file.
- Comments explain WHY and cite the reference `file:line` when transcribing a wire
  format or a behaviour copied from GHost++ or from the `499b987~1` legacy code.
- No tautological tests. A test asserting only `is_ok()`, `!is_empty()`, `len() == N`,
  or `!= 0` is a defect. Every assertion pins an exact expected value.
- Every Rust task ends with: `cargo fmt --all`,
  `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace --all-features`, all passing. Current baseline is
  **157 passing tests, 0 failures** — no task may reduce it.
- C++ tasks target the existing MSVC project `dotatv_client/dotatv_client.vcxproj`,
  Win32 x86 (Warcraft III 1.26a is 32-bit), and must build with no new warnings.
- Each task ends with a git commit; commit messages use Conventional Commits.

---

## Background & Root Cause Analysis

A live Warcraft III 1.26a client sees the hosted game in the Battle.net Custom Games list, but upon attempting to join receives the dialog error: *"The game you attempted to join could not be found."* Simultaneously, the bot's TCP listener on `0.0.0.0:6113` records **zero inbound TCP connections** — the client never even attempts to initiate a TCP handshake to the bot. The server ACKs `SID_STARTADVEX3` with `status = 0` (success), confirming the game is accepted into the server's game list, but joining clients fail to resolve a joinable address and map specification.

Commit `499b987` ("chore: remove the legacy GHost++ transliteration") deleted a Rust transliteration of GHost++ whose commit message states: *"Parity verified on a live PvPGN server: login, hosting, join, start, full game, GProxy reconnect and DotaTV spectating all confirmed."* That deleted code is inspectable at `499b987~1` via:
```bash
git show 499b987~1:src/bnet.rs
git show 499b987~1:src/bnetprotocol.rs
```

Diffing the working baseline against the current codebase revealed three concrete divergences:

### 1. `MAPGAMETYPE_UNKNOWN0` (Bit 0) is never set
`ref/ghostpp/ghost/map.h:70` defines:
```c
#define MAPGAMETYPE_UNKNOWN0			1			// always set except for saved games?
```
`ref/ghostpp/ghost/bnet.cpp:2278-2279` sets:
```cpp
uint32_t MapGameType = map->GetMapGameType( );
MapGameType |= MAPGAMETYPE_UNKNOWN0;
```
The working legacy Rust did the same (`499b987~1:src/bnet.rs`, in `queue_game_refresh`):
```rust
let mut map_game_type = map.get_map_game_type();
map_game_type |= MAPGAMETYPE_UNKNOWN0;

if state == GAME_PRIVATE {
    map_game_type |= MAPGAMETYPE_PRIVATEGAME;
}
```
Current code at `crates/ghost-bnet/src/client.rs:108-114` sets only the private bit:
```rust
const MAPGAMETYPE_PRIVATEGAME: u32 = 0x0000_0800;

fn advert_game_type(map: &MapAdvert, visibility: ghost_protocol::GameVisibility) -> [u8; 4] {
    let mut t = map.game_type;
    if visibility == ghost_protocol::GameVisibility::Private {
        t |= MAPGAMETYPE_PRIVATEGAME;
    }
    t.to_le_bytes()
}
```
The `MAPGAMETYPE_UNKNOWN0` bit is missing. The advertised game type is what the client filters the game list on. The fallback map in `crates/ghostrs/src/supervisor.rs:506` masked this by setting `game_type: 1`, but the real MPQ-parsed map (`maps\iCCup DotA 454.w3x`) supplies its own `game_type` (e.g. `0` or `0x0001_8000`), resulting in bit 0 being cleared on live games.

### 2. Advertised map path is not in `Maps\Download\` format
Working legacy sent (`499b987~1:src/bnet.rs`, `queue_game_refresh`):
```rust
format!("Maps\\Download\\{}", map.get_map_path()),
```
Current `crates/ghostrs/src/supervisor.rs:523` passes `map_info.path.clone()` straight through from the MPQ loader (`maps\iCCup DotA 454.w3x`), pointing to the bot's local folder instead of the client-side `Maps\Download\` convention that the joining Warcraft III client expects. The path is embedded in the stat string via `encode_game_statstring`, so `MapAdvert.path` must be normalized at creation time while leaving `GameConfig.map`'s on-disk path untouched.

### 3. Announced Port vs. Configured Port
Working legacy sent port `6112` in `SID_NETGAMEPORT`. Current `crates/ghost-bnet/src/client.rs:484-485` sends `cfg.host_port`, and `ghost.toml` sets `host_port = 6113`. 6112 is the canonical Warcraft III game port; non-standard ports (like 6113) often fail NAT traversal and router forwarding. This is a configuration requirement: the code must clearly log the announced port and emit startup warnings when `host_port != 6112`.

---

## File Structure

| Action | Path | Single Responsibility |
|---|---|---|
| **Modify** | `crates/ghost-bnet/src/client.rs` | Unconditionally set `MAPGAMETYPE_UNKNOWN0` in `advert_game_type`; log port on `SID_NETGAMEPORT` send; add unit tests. |
| **Modify** | `crates/ghostrs/src/supervisor.rs` | Normalize `advert_map.path` to `Maps\Download\<filename>` when constructing `MapAdvert`; warn at startup if `host_port != 6112`. |

---

## Tasks

### Task 1: Unconditional `MAPGAMETYPE_UNKNOWN0` in Game Type Advertisement (`ghost-bnet`)

**Files:**
- Modify: `crates/ghost-bnet/src/client.rs:104-114`
- Test: Embedded in `crates/ghost-bnet/src/client.rs`

**Interfaces:**
- Consumes: `map: &MapAdvert`, `visibility: ghost_protocol::GameVisibility`
- Produces: `advert_game_type(map: &MapAdvert, visibility: ghost_protocol::GameVisibility) -> [u8; 4]` with bit 0 (`MAPGAMETYPE_UNKNOWN0 = 0x0000_0001`) unconditionally set.

- [ ] **Step 1: Write unit tests in `crates/ghost-bnet/src/client.rs`**

Add unit tests asserting:
1. `map.game_type = 0` with `GameVisibility::Public` produces `0x0000_0001` -> `[0x01, 0x00, 0x00, 0x00]`.
2. `map.game_type = 0` with `GameVisibility::Private` produces `0x0000_0801` -> `[0x01, 0x08, 0x00, 0x00]`.
3. `map.game_type = 0x0001_8000` with `GameVisibility::Public` produces `0x0001_8001` -> `[0x01, 0x80, 0x01, 0x00]`.
4. `map.game_type = 0x0001_8000` with `GameVisibility::Private` produces `0x0001_8801` -> `[0x01, 0x88, 0x01, 0x00]`.

In `crates/ghost-bnet/src/client.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ghost_protocol::GameVisibility;

    fn make_test_map(game_type: u32) -> MapAdvert {
        MapAdvert {
            path: "Maps\\Download\\test.w3x".to_string(),
            size: 1000,
            info: 1,
            crc: 0x1234_5678,
            sha1: [0; 20],
            num_players: 10,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type,
            flags: 0,
        }
    }

    #[test]
    fn advert_game_type_always_sets_unknown0_bit() {
        // Map with game_type 0 in public game: bit 0 must be set.
        let map_zero = make_test_map(0);
        assert_eq!(
            advert_game_type(&map_zero, GameVisibility::Public),
            [0x01, 0x00, 0x00, 0x00],
            "bit 0 (MAPGAMETYPE_UNKNOWN0) must be set for game_type=0 in public game"
        );

        // Map with game_type 0 in private game: bit 0 and bit 11 (0x0800) must be set.
        assert_eq!(
            advert_game_type(&map_zero, GameVisibility::Private),
            [0x01, 0x08, 0x00, 0x00],
            "bit 0 and bit 11 (MAPGAMETYPE_PRIVATEGAME) must be set for private game"
        );

        // Real MPQ map with game_type = 0x0001_8000:
        let map_real = make_test_map(0x0001_8000);
        assert_eq!(
            advert_game_type(&map_real, GameVisibility::Public),
            [0x01, 0x80, 0x01, 0x00],
            "0x0001_8000 | 1 = 0x0001_8001 -> [0x01, 0x80, 0x01, 0x00]"
        );
        assert_eq!(
            advert_game_type(&map_real, GameVisibility::Private),
            [0x01, 0x88, 0x01, 0x00],
            "0x0001_8000 | 0x0800 | 1 = 0x0001_8801 -> [0x01, 0x88, 0x01, 0x00]"
        );
    }
}
```

- [ ] **Step 2: Update `advert_game_type` in `crates/ghost-bnet/src/client.rs`**

Replace `crates/ghost-bnet/src/client.rs:104-114`:
```rust
/// `map.h:70` defines MAPGAMETYPE_UNKNOWN0 = 1 ("always set except for saved games?").
/// `bnet.cpp:2279` unconditionally ORs MAPGAMETYPE_UNKNOWN0 into MapGameType before advertising.
const MAPGAMETYPE_UNKNOWN0: u32 = 0x0000_0001;

/// `bnet.cpp:2284` ORs MAPGAMETYPE_PRIVATEGAME into the game type for a private
/// game, so it stays out of the public game list.
const MAPGAMETYPE_PRIVATEGAME: u32 = 0x0000_0800;

fn advert_game_type(map: &MapAdvert, visibility: ghost_protocol::GameVisibility) -> [u8; 4] {
    let mut t = map.game_type | MAPGAMETYPE_UNKNOWN0;
    if visibility == ghost_protocol::GameVisibility::Private {
        t |= MAPGAMETYPE_PRIVATEGAME;
    }
    t.to_le_bytes()
}
```

- [ ] **Step 3: Run unit tests and clippy**

```bash
cargo test -p ghost-bnet
cargo clippy -p ghost-bnet -- -D warnings
```
Expected: All tests pass, including `advert_game_type_always_sets_unknown0_bit`.

- [ ] **Step 4: Commit task changes**

```bash
git add crates/ghost-bnet/src/client.rs
git commit -m "fix(bnet): unconditionally set MAPGAMETYPE_UNKNOWN0 in advertised game type"
```

---

### Task 2: Advertised Map Path Normalization to `Maps\Download\` Format (`ghostrs`)

**Files:**
- Modify: `crates/ghostrs/src/supervisor.rs:514-537`
- Test: Embedded in `crates/ghostrs/src/supervisor.rs`

**Interfaces:**
- Consumes: `raw_path: &str` (e.g. `maps\iCCup DotA 454.w3x` or `maps/dota.w3x`)
- Produces: `normalize_advert_map_path(raw_path: &str) -> String` producing canonical `Maps\Download\<filename>`
- Leaves: `map_info.path` unchanged for local disk loading, only mutating the advertised `MapAdvert.path`.

- [ ] **Step 1: Write unit tests for `normalize_advert_map_path` in `crates/ghostrs/src/supervisor.rs`**

Add unit tests checking all four normalization cases:
1. `maps\iCCup DotA 454.w3x` -> `Maps\Download\iCCup DotA 454.w3x`
2. `Maps\Download\foo.w3x` -> `Maps\Download\foo.w3x` (idempotent)
3. `foo.w3x` -> `Maps\Download\foo.w3x` (bare filename)
4. `maps/custom/nested/dota.w3x` -> `Maps\Download\dota.w3x` (forward slashes and nested folders)

In `crates/ghostrs/src/supervisor.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_advert_map_path_produces_canonical_download_path() {
        assert_eq!(
            normalize_advert_map_path("maps\\iCCup DotA 454.w3x"),
            "Maps\\Download\\iCCup DotA 454.w3x"
        );
        assert_eq!(
            normalize_advert_map_path("Maps\\Download\\iCCup DotA 454.w3x"),
            "Maps\\Download\\iCCup DotA 454.w3x"
        );
        assert_eq!(
            normalize_advert_map_path("bare_map.w3x"),
            "Maps\\Download\\bare_map.w3x"
        );
        assert_eq!(
            normalize_advert_map_path("maps/custom/nested/dota.w3x"),
            "Maps\\Download\\dota.w3x"
        );
    }
}
```

- [ ] **Step 2: Implement `normalize_advert_map_path` and use in `Supervisor::create_game`**

In `crates/ghostrs/src/supervisor.rs`:
```rust
/// Normalizes an on-disk map path into the canonical client-side `Maps\Download\<filename>` format.
/// Cites legacy `499b987~1:src/bnet.rs` (`format!("Maps\\Download\\{}", map.get_map_path())`).
pub fn normalize_advert_map_path(raw_path: &str) -> String {
    let filename = raw_path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(raw_path);
    format!("Maps\\Download\\{filename}")
}
```

In `Supervisor::create_game` (`crates/ghostrs/src/supervisor.rs:522-535`):
```rust
        let advert_map = MapAdvert {
            path: normalize_advert_map_path(&map_info.path),
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
```
Note: `encode_game_statstring` in `crates/ghost-bnet/src/advert.rs` remains unchanged; it consumes `advert_map.path` directly.

- [ ] **Step 3: Run unit tests and clippy**

```bash
cargo test -p ghostrs
cargo clippy -p ghostrs -- -D warnings
```
Expected: All tests pass, including `normalize_advert_map_path_produces_canonical_download_path`.

- [ ] **Step 4: Commit task changes**

```bash
git add crates/ghostrs/src/supervisor.rs
git commit -m "fix(supervisor): normalize advertised map path to Maps\Download convention"
```

---

### Task 3: `SID_NETGAMEPORT` Port Logging and Startup Configuration Warning

**Files:**
- Modify: `crates/ghost-bnet/src/client.rs:484-486`
- Modify: `crates/ghostrs/src/supervisor.rs:55-80`

**Interfaces:**
- Consumes: `cfg.host_port` in `ghost-bnet`, `cfg.bot.host_port` and `cfg.bnet.host_port` in `supervisor`.
- Produces: `tracing::info!` log at `SID_NETGAMEPORT` transmission; `tracing::warn!` at startup when `host_port != 6112`.

- [ ] **Step 1: Enhance `SID_NETGAMEPORT` logging in `crates/ghost-bnet/src/client.rs`**

Update `crates/ghost-bnet/src/client.rs:484-485`:
```rust
stage = Stage::AwaitEnterChat;
tracing::info!(
    port = cfg.host_port,
    "--> [SEND] SID_NETGAMEPORT (0x45) announcing host port to battle.net"
);
let _ = framed_write.send(outgoing::netgameport(cfg.host_port)).await;
```

- [ ] **Step 2: Add startup configuration warning in `Supervisor::run`**

In `crates/ghostrs/src/supervisor.rs:55-75`, right after reading configuration and before starting the listener:
```rust
        if cfg.bot.host_port != 6112 || cfg.bnet.host_port != 6112 {
            tracing::warn!(
                bot_host_port = cfg.bot.host_port,
                bnet_host_port = cfg.bnet.host_port,
                "host_port is not 6112; war3 clients and most routers expect 6112 — joins may fail"
            );
        }
```

- [ ] **Step 3: Run full workspace test suite and linter**

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-features
```
Expected: All 157+ baseline tests pass with 0 errors.

- [ ] **Step 4: Commit task changes**

```bash
git add crates/ghost-bnet/src/client.rs crates/ghostrs/src/supervisor.rs
git commit -m "feat(bnet): add SID_NETGAMEPORT logging and non-6112 port startup warning"
```

---

### Task 4: Live Battle.net Probe & Joinability Acceptance Verification

**Files:**
- Target binary: `target/release/ghostrs.exe`
- Verification log: stdout / console logger

**Interfaces:**
- Consumes: Live Battle.net connection, `SID_GETADVLISTEX` probe responses.
- Produces: Confirmation of game discoverability and client connection readiness.

- [ ] **Step 1: Build the release binary**

```bash
cargo build --release --bin ghostrs
```
Expected: `target/release/ghostrs.exe` compiles cleanly with 0 warnings.

- [ ] **Step 2: Operator Configuration Checklist**

The operator must verify `ghost.toml` settings before running:
- `[bot] host_port = 6112`
- `[bnet] host_port = 6112`
- Router/Firewall: TCP port `6112` forwarded to the host machine.
- Verify listener bind address and announced port match.

- [ ] **Step 3: Launch live probe host**

```bash
./target/release/ghostrs.exe --host "ghostrs probe8"
```

- [ ] **Step 4: Analyze `SID_GETADVLISTEX` probe output**

Observe the automated 10-second self-probe line in the bot console (`crates/ghost-bnet/src/client.rs:596-618`):

1. **Outcome A (Failure):**
   ```
   WARN ghost_bnet::client: <-- [RECV] SID_GETADVLISTEX (0x09) [games_found=0] — the server does NOT have our game; joins will fail
   ```
   *Diagnosis:* The server rejected or dropped the game advert. Check `SID_STARTADVEX3` status code, map flags, and map path formatting.

2. **Outcome B (Success):**
   ```
   INFO ghost_bnet::client: game="ghostrs probe8" ip="a.b.c.d" port=6112 host_counter=0x0... <-- [RECV] SID_GETADVLISTEX (0x09) — server WILL hand joiners this address
   ```
   *Diagnosis:* The server has registered the game and is returning it to querying clients. Verify `a.b.c.d:port` matches your public IP and forwarded port. If a mismatch exists, the issue is router NAT/port-forwarding, not the protocol.

- [ ] **Step 5: Warcraft III Client Join Verification**

1. Launch Warcraft III 1.26a.
2. Log into the Battle.net server (e.g. iCCup).
3. Open **Custom Games**.
4. Select `ghostrs probe8` and click **Join Game**.
5. Observe the bot log:
   ```
   INFO ghost_net::listener: inbound TCP connection accepted
   INFO ghost_engine::lobby: player joined slot
   ```
   *Acceptance Criteria:* Client successfully enters the lobby screen; slot information and map download/verification complete.
