# Implementation Plan: DotaTV Live Spectator Client

> **For agentic workers:** Required superpowers: [`superpowers:subagent-driven-development`](file:///C:/Users/slash/iccwc3_work/ghostrs/.superpowers/skills/subagent-driven-development/SKILL.md)
>
> **Goal:** Let a Warcraft III 1.26a player type `ip:port` into a widget in the top-left of the main menu, press Connect, and end up watching a live ghostrs DotA game in the game client, with the correct simulation and no desync.
>
> **Architecture:** In-game spectator bridge running entirely inside an injected Win32 x86 C++ DLL (`dotatv_client`, deployed as `war3/dotatv.mix`), communicating with `ghostrs` spectator relay over a dedicated `0xFD` framed TCP wire protocol. The relay buffers game start snapshots, player tables, and all historical action blocks, streaming them to `dotatv_client` upon connection. Inside the client, a local W3GS host (`LocalHost`) on `127.0.0.1` emulates a full replay host, presents the exact original match environment, auto-joins the client via reverse-engineered `game.dll` entry points, and feeds the action stream to the Warcraft III engine from tick 0 to catch up to live play while discarding client input.
>
> **Tech Stack:**
> - Host Server (`ghostrs`): Rust 2024 edition, `#![forbid(unsafe_code)]`, Tokio 1.45, `bytes 1.10.1`, `ghost-protocol`, `ghost-spectator`, `ghost-engine`, `ghost-net`.
> - Client DLL (`dotatv_client`): C++20, MSVC v143+ (Win32 x86), MinHook, DirectX 8 (`d3d8.dll` / `d3dx8.lib`), Winsock2, Storm.dll API (`SFileOpenFileEx`/`SFileReadFile`).

---

## 1. Global Constraints

The following constraints are mandatory across all tasks and must be followed without exception:

- **Rust 2024 edition workspace:** `ghost-net`, `ghost-spectator`, `ghost-bnet` carry `#![forbid(unsafe_code)]`. No `unsafe` in any Rust crate.
- **No new external Rust dependencies:** No new `[dependencies]` entries in any `Cargo.toml`.
- **No panicking operations on network data:** No `unwrap()`, `expect()`, or panicking indexing on data that came off the network. Parsers must use checked accessors and return explicit errors (`ProtoError` / `Result`).
- **Never block or sleep on an async task:** Use `tokio::time::sleep`, `tokio::time::interval`, or `tokio::task::spawn_blocking` for CPU-heavy disk/zlib tasks.
- **Protected credentials file:** Never read, print, modify, commit, checkout, restore, or clean `ghost.toml`. No task or command may touch it.
- **Traceable citations:** Comments must explain WHY and cite reference `file:line` when transcribing a wire format or behavior copied from GHost++.
- **C++ compilation target:** C++ targets MSVC v143+, C++20, **Win32 x86** (32-bit PE, matching Warcraft III 1.26a), and must build cleanly with zero warnings (`/W3 /WX` or `/W3`).
- **Warcraft III version support:** Target is Warcraft III 1.26a (`1.26.0.6401`) only. The signature scanner and self-test enforce this at runtime.
- **Cross-repo structure:** `dotatv_client` is located at `C:\Users\slash\iccwc3_work\dotatv_client` (sibling directory, outside the `ghostrs` git index). All file references to the C++ project use absolute paths. Its build artifact is deployed to `C:\Users\slash\iccwc3_work\ghostrs\war3\dotatv.mix`.

---

## 2. Architecture & Wire Protocols

### 2.1 The Core Constraint & Simulation Model

Warcraft III 1.26a does not support joining a game that is already in progress. Therefore, live spectating operates via **full replay-player emulation**:
1. When the spectator clicks **Connect**, the in-DLL `LocalHost` begins listening on an ephemeral port on `127.0.0.1`.
2. The client connects to `ghostrs` spectator relay over TCP (`0xFD` protocol).
3. The relay delivers the `GAME_START_SNAPSHOT` and seated `PLAYER` definitions, followed by the complete history buffer of `INCOMING_ACTION` / `INCOMING_ACTION2` packets recorded since tick 0.
4. `dotatv_client` invokes the reverse-engineered `game.dll` LAN join routine with `127.0.0.1:<LocalHost port>`.
5. The game client joins `LocalHost`, loads the map, sends `GAME_LOADED_SELF` (0x23), and enters the game.
6. `LocalHost` streams the entire recorded history of action packets from tick 0 to the client as fast as TCP allows. The Warcraft III simulation fast-forwards through the action stream until it catches up to the live tail.
7. Once history is exhausted (`HISTORY_END`), `LocalHost` streams live actions delayed by the configured spectator delay (`delay_sec`).

#### Accepted System Costs
1. **Catch-up Duration:** A spectator joining 40 minutes into a DotA match must simulate 40 minutes of game ticks (~24,000 action frames). Catch-up takes several seconds to a few minutes depending on CPU speed. The D3D8 overlay displays an interactive progress state (`Loading N%`).
2. **Deterministic Init Requirement:** The client simulation remains in sync only if map CRC/size/SHA1, slot table, random seed, layout style, player count, and player PIDs/names/colors/teams match the original game initialization byte for byte.

#### Known Divergence Risk & Mitigation
- **Risk:** Map initialization triggers in DotA (e.g. hero pick scripts, gold distribution) inspect player count and slot states. The spectator client occupies an observer slot (PID e.g. 12) not present as an active human during the original match.
- **Mitigation:** The spectator slot is configured with `slot_status = 2` (occupied), `team = 12`, `colour = 12`, `race = 0x20`, `computer = 0`, `download_status = 100`, and `handicap = 100`. This matches GHost++'s referee/observer slot (`CGameSlot(0, 255, SLOTSTATUS_OCCUPIED, 0, 12, 12, SLOTRACE_RANDOM)`). Note `0x20` is `SLOTRACE_RANDOM` — Warcraft III has **no** "observer race"; observer status comes from team 12 plus the map's observer game-type flags, not from the race byte. Do not invent a race constant.
- **Simulation Isolation:** `LocalHost` unconditionally **drops** all `OUTGOING_ACTION` (0x26) and `CHAT_TO_HOST` (0x28) packets sent by the spectator's client. The simulation is driven purely by the recorded server stream.
- **Verification:** End-to-end conformance test verifies that the spectator's saved `.w3g` replay action stream matches the host's `.w3g` replay action stream byte for byte.

---

### 2.2 Relay Wire Protocol v1 (`0xFD`) Specification

The spectator protocol uses `0xFD` framing. All multi-byte integers are **little-endian** (`le`).

#### Frame Header (4 bytes)
| Offset | Field | Type | Description |
|---|---|---|---|
| 0x00 | `header` | `u8` | Always `0xFD` |
| 0x01 | `id` | `u8` | Message identifier (0x01–0x07 server→client, 0x80–0x81 client→server) |
| 0x02 | `length` | `u16 le` | Total frame length including the 4-byte header |

---

#### Server → Client Messages

##### `0x01 HELLO`
Sent immediately upon client TCP connection.
- `version`: `u16 le` (Protocol version, currently `1`)
- `server_name`: `cstring` (Null-terminated ASCII string, e.g. `"ghostrs\0"`)

*Worked Hex Example:* `FD 01 0E 00 01 00 67 68 6F 73 74 72 73 00`
- `FD 01`: Header `0xFD`, ID `0x01`
- `0E 00`: Total length = 14 bytes (`0x000E`)
- `01 00`: Version = 1
- `67 68 6F 73 74 72 73 00`: `"ghostrs\0"`

---

##### `0x02 GAME_START_SNAPSHOT`
Delivers complete match initialization parameters required to start the local simulation.
- `game_name`: `cstring`
- `map_path`: `cstring` (e.g. `"Maps\\Download\\DotA v6.83d.w3x\0"`)
- `map_size`: `u32 le`
- `map_info_crc`: `u32 le`
- `map_crc`: `u32 le`
- `map_sha1`: `[u8; 20]` (20 raw bytes)
- `stat_string`: `cstring` (Null-terminated statstring)
- `random_seed`: `u32 le`
- `layout_style`: `u8`
- `player_slots`: `u8`
- `war3_version`: `u8` (e.g. `26`)
- `is_tft`: `u8` (`1` for TFT, `0` for RoC)
- `num_slots`: `u8`
- `slots`: `num_slots * 9` bytes (Array of 9-byte `SlotInfo` records: `pid`, `download_status`, `slot_status`, `computer`, `team`, `colour`, `race`, `computer_type`, `handicap`)

*Worked Hex Example Structure (Payload snippet):*
`FD 02 [len:2] "DotA Live\0" "Maps\\dota.w3x\0" [size:4] [info:4] [crc:4] [sha1:20] [statstr\0] [seed:4] [layout:1] [playerslots:1] [war3ver:1] [tft:1] [num_slots:1] [slot0:9] ...`

---

##### `0x03 PLAYER`
Delivers one seated player definition. Sent N times after snapshot.
- `pid`: `u8`
- `name`: `cstring`
- `colour`: `u8`
- `team`: `u8`
- `race`: `u8`

*Worked Hex Example:* `FD 03 10 00 01 50 6C 61 79 65 72 31 00 01 00 01`
- `FD 03`: Header `0xFD`, ID `0x03`
- `10 00`: Total length = 16 bytes (`0x0010`)
- `01`: PID = 1
- `50 6C 61 79 65 72 31 00`: `"Player1\0"`
- `01 00 01`: Colour = 1 (Blue), Team = 0 (Sentinel), Race = 1 (Human)

---

##### `0x04 ACTION`
Encapsulates one complete recorded W3GS `INCOMING_ACTION` (0x0C) or `INCOMING_ACTION2` (0x48) packet, verbatim including its original `0xF7` header.
- `payload`: `bytes` (Complete `0xF7` frame)

*Worked Hex Example (Empty 100ms Action Tick):* `FD 04 0A 00 F7 0C 06 00 64 00`
- `FD 04`: Header `0xFD`, ID `0x04`
- `0A 00`: Total length = 10 bytes (`0x000A`)
- `F7 0C 06 00 64 00`: W3GS `INCOMING_ACTION` frame (`0xF7`, ID `0x0C`, length 6, interval 100ms)

---

##### `0x05 CHAT`
Spectator broadcast chat message.
- `sender`: `cstring`
- `text`: `cstring`

*Worked Hex Example:* `FD 05 12 00 48 6F 73 74 00 57 65 6C 63 6F 6D 65 21 00`
- `FD 05`: Header `0xFD`, ID `0x05`
- `12 00`: Total length = 18 bytes (`0x0012`)
- `48 6F 73 74 00`: `"Host\0"`
- `57 65 6C 63 6F 6D 65 21 00`: `"Welcome!\0"`

---

##### `0x06 GAME_OVER`
Announces match conclusion.
- `winner`: `cstring` (e.g. `"Sentinel\0"` or `"Scourge\0"`)

*Worked Hex Example:* `FD 06 0D 00 53 65 6E 74 69 6E 65 6C 00`
- `FD 06`: Header `0xFD`, ID `0x06`
- `0D 00`: Total length = 13 bytes (`0x000D`)
- `53 65 6E 74 69 6E 65 6C 00`: `"Sentinel\0"`

---

##### `0x07 HISTORY_END`
Signals end of historical action replay burst. All subsequent `0x04 ACTION` frames represent live, delayed gameplay.
- `history_packet_count`: `u32 le` (Total count of history action packets delivered)

*Worked Hex Example:* `FD 07 08 00 E8 03 00 00`
- `FD 07`: Header `0xFD`, ID `0x07`
- `08 00`: Total length = 8 bytes (`0x0008`)
- `E8 03 00 00`: 1,000 packets (`0x000003E8`)

---

#### Client → Server Messages

##### `0x80 SUBSCRIBE`
Sent by client upon TCP connection to request stream.
- `client_version`: `u16 le` (Client protocol version, currently `1`)

*Worked Hex Example:* `FD 80 06 00 01 00`
- `FD 80`: Header `0xFD`, ID `0x80`
- `06 00`: Total length = 6 bytes
- `01 00`: Client Version = 1

---

##### `0x81 CHAT`
Spectator sending chat to spectator channel.
- `text`: `cstring`

*Worked Hex Example:* `FD 81 0A 00 47 47 20 57 50 00`
- `FD 81`: Header `0xFD`, ID `0x81`
- `0A 00`: Total length = 10 bytes
- `47 47 20 57 50 00`: `"GG WP\0"`

---

### 2.3 Memory Budget & Relay History Buffer Lifecycle

- **Packet Sizing:** At a standard 100 ms tick interval, a 60-minute game produces 36,000 action ticks. With average packet size between 200 and 800 bytes, total history memory is ~10–30 MB per active game.
- **Configurable Cap:** `[spectator] history_max_mb = 64` (default 64 MB).
- **Buffer Retention & Cap Behavior:** `Relay` stores `history: Vec<Bytes>`. If memory exceeds `history_max_mb`, the oldest non-essential action blocks are not pruned (as pruning tick 0 breaks Warcraft III replay fast-forward); instead, new spectator joins are rejected with an error state `"History buffer limit exceeded"` until the game ends. When the game ends (`RelayCmd::GameOver` / `Shutdown`), the history buffer is dropped immediately.

---

### 2.4 In-DLL Threading Model (`dotatv_client`)

To eliminate race conditions and avoid blocking the Warcraft III main render loop:
1. **D3D8 Render Thread (`IDirect3DDevice8::EndScene` Hook):**
   - Polls the atomic state of `AutoJoin` and `NetClient`.
   - Renders the Connect UI widget, input box, buttons, and progress overlay (`Loading N%`).
   - Uses precomputed viewport-relative screen coordinates for rendering and hit-testing.
2. **WndProc Hook (UI Input Thread):**
   - Intercepts `WM_LBUTTONDOWN`, `WM_LBUTTONUP`, `WM_MOUSEMOVE`, `WM_CHAR`, `WM_KEYDOWN`.
   - When the Connect dialog or editbox is focused, consumes relevant keystrokes and clicks without passing them to Warcraft III's engine.
3. **NetClient Thread (TCP Client):**
   - Runs a dedicated background socket receive loop.
   - Decodes `0xFD` frames and enqueues snapshots and action packets into a thread-safe MPSC queue.
4. **LocalHost Thread (W3GS Server on `127.0.0.1`):**
   - Accepts the single game client connection.
   - Manages the W3GS join/load handshake.
   - Pulls action packets from the shared history queue and writes them to the client socket.

---

## 3. File Structure

| Action | Path | Single Responsibility |
|---|---|---|
| **Delete** | `ghostrs/docs/superpowers/plans/2026-08-15-dotatv-client.md` | Remove superseded and inaccurate draft plan. |
| **Create** | `ghostrs/docs/superpowers/plans/2026-08-15-dotatv-live-spectator.md` | The authoritative implementation plan. |
| **Modify** | `ghostrs/crates/ghost-engine/src/actions.rs` | Fix `incoming_action2` overflow forwarding to relay and replay in tick order. |
| **Create** | `ghostrs/crates/ghost-protocol/src/dotatv.rs` | `0xFD` frame codec, message encoders, decoders, and data structures. |
| **Modify** | `ghostrs/crates/ghost-protocol/src/lib.rs` | Expose `pub mod dotatv;`. |
| **Create** | `ghostrs/crates/ghost-protocol/tests/dotatv_golden.rs` | Unit tests generating golden binary fixtures for cross-language verification. |
| **Modify** | `ghostrs/crates/ghost-spectator/src/relay.rs` | History buffer, `0xFD` framing, `RelayCmd::GameStart`, viewer lifecycle, and chat. |
| **Modify** | `ghostrs/crates/ghost-spectator/src/lib.rs` | Export updated `RelayConfig` and relay commands. |
| **Modify** | `ghostrs/crates/ghostrs/src/config.rs` | Add `history_max_mb` to spectator configuration. |
| **Modify** | `ghostrs/crates/ghostrs/src/supervisor.rs` | Pass spectator config and map info to relay initialization. |
| **Create** | `dotatv_client/include/SigScan.hpp` | Pattern scanning declarations over `game.dll` `.text`. |
| **Create** | `dotatv_client/src/SigScan.cpp` | Pattern scanner, symbol table, and startup self-test logging to `dotatv_client.log`. |
| **Modify** | `dotatv_client/include/GameOffsets.hpp` | Replace hardcoded RVAs with pattern-scanned pointers. |
| **Modify** | `dotatv_client/src/DotaTV.cpp` | Remove blanket exception handlers; install scanner and hooks cleanly. |
| **Create** | `dotatv_client/include/BLPDecoder.hpp` | BLP1 texture decoder (Palettised and JPEG formats). |
| **Create** | `dotatv_client/src/BLPDecoder.cpp` | BLP1 decoder implementation converting MPQ images to D3D8 textures. |
| **Create** | `dotatv_client/include/D3D8Hook.hpp` | MinHook installation for `IDirect3DDevice8::EndScene`. |
| **Create** | `dotatv_client/src/D3D8Hook.cpp` | DirectX 8 hook capturing real device pointer and rendering UI. |
| **Create** | `dotatv_client/include/OverlayUI.hpp` | Main menu widget geometry, font rendering, state machine, and hit testing. |
| **Create** | `dotatv_client/src/OverlayUI.cpp` | Connect widget implementation with glue textures and real-time status display. |
| **Modify** | `dotatv_client/include/NetClient.hpp` | Extend NetClient with `0xFD` protocol frame decoding and queue management. |
| **Modify** | `dotatv_client/src/NetClient.cpp` | Winsock2 `0xFD` protocol implementation. |
| **Create** | `dotatv_client/include/LocalHost.hpp` | In-DLL W3GS host on `127.0.0.1` for replay emulation. |
| **Create** | `dotatv_client/src/LocalHost.cpp` | W3GS host handshake, packet filtering (dropping 0x26/0x28), and history streaming. |
| **Create** | `dotatv_client/include/AutoJoin.hpp` | Orchestrates Connect click → NetClient → Snapshot → LocalHost → LAN Join hook. |
| **Create** | `dotatv_client/src/AutoJoin.cpp` | Auto-join state machine and error handling. |
| **Create** | `dotatv_client/tests/dotatv_tests.vcxproj` | Win32 x86 test runner project for C++ unit and conformance tests. |
| **Create** | `dotatv_client/tests/test_main.cpp` | C++ test harness entry point and runner. |
| **Create** | `dotatv_client/tests/test_conformance.cpp` | Compares C++ packet builders against Rust golden `.bin` fixtures. |
| **Create** | `ghostrs/crates/ghost-engine/tests/spectator_relay_e2e.rs` | Rust integration test verifying spectator join, snapshot, and action streaming. |

---

## 4. Implementation Tasks

### Task 0: Baseline Measurement & Stale Plan Removal

- **Files:**
  - Delete: `docs/superpowers/plans/2026-08-15-dotatv-client.md`
- **Interfaces:** None (Housekeeping & baseline measurement).

- [ ] **Step 1: Measure and record baseline workspace test results**

  Run `cargo test --workspace --all-features` in `ghostrs`. Verify every test passes, then write the exact totals into this plan by replacing the line below (edit this file):

  > **Measured baseline (recorded 2026-08-15, commit `c5ef2b6`, branch `feat/dotatv-live-spectator`):** **157 passing, 0 failed, across 16 test binaries**; `cargo test --workspace --all-features` exits 0. No later task may reduce this number.

  Do not trust any test count quoted in an older plan document — measure it.

- [ ] **Step 2: Remove the stale and inaccurate draft plan**

  `docs/superpowers/plans/2026-08-15-dotatv-client.md` is **untracked** (it has never been committed), so `git rm` fails on it with `fatal: pathspec ... did not match any files`. Delete it from the working tree instead:

  ```bash
  rm docs/superpowers/plans/2026-08-15-dotatv-client.md
  ```

  Verify with `git status --short docs/` that the file no longer appears.

- [ ] **Step 3: Commit this plan**

  Deleting an untracked file stages nothing, so there is no deletion to commit — the commit exists to put *this* plan under version control:

  ```bash
  git add docs/superpowers/plans/2026-08-15-dotatv-live-spectator.md
  git commit -m "docs: add dotatv live spectator plan and record test baseline"
  ```

---

### Task 1: Fix Action Overflow Forwarding in `ghost-engine`

- **Files:**
  - Modify: `crates/ghost-engine/src/actions.rs`
- **Interfaces:**
  - Consumes: `ghost_protocol::w3gs::outgoing::incoming_action2`, `ghost_spectator::RelayHandle::push`, `ghost_spectator::ReplayBody::add_timeslot`.
  - Produces: Correct forwarding of `INCOMING_ACTION2` (0x48) packets to spectator relay and replay recorder in tick order.

- [ ] **Step 1: Write failing test in `crates/ghost-engine/src/actions.rs`**
  Add a unit test `oversized_action_batches_are_relayed_and_recorded_in_order` verifying that when queued actions exceed `MAX_ACTION_PAYLOAD`:
  1. An `INCOMING_ACTION2` (0x48) packet is pushed to `self.relay` before the main `INCOMING_ACTION` (0x0C) packet.
  2. `self.replay` records a timeslot with `time_increment = 0` and the overflow action payload.
  3. The test fails prior to implementation.
- [ ] **Step 2: Implement the fix in `crates/ghost-engine/src/actions.rs`**
  Update `send_all_actions` at lines 206–216:
  ```rust
  let len = action.wire_len();
  if batch_len + len > MAX_ACTION_PAYLOAD && !batch.is_empty() {
      match outgoing::incoming_action2(&batch) {
          Ok(b) => {
              if let Some(r) = &self.relay {
                  r.push(b.clone());
              }
              if let Some(rep) = self.replay.as_mut()
                  && b.len() >= 6
              {
                  rep.add_timeslot(0, &b[6..]);
              }
              self.broadcast(b);
          }
          Err(e) => tracing::warn!(error = %e, "failed to build overflow packet"),
      }
      batch.clear();
      batch_len = 0;
  }
  ```
- [ ] **Step 3: Run unit tests and verify green**
  Run `cargo test -p ghost-engine actions::tests::oversized_action_batches_are_relayed_and_recorded_in_order`.
- [ ] **Step 4: Format and lint**
  Run `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 5: Commit**
  ```bash
  git commit -m "fix(engine): forward overflow incoming_action2 packets to relay and replay"
  ```

---

### Task 2: DotaTV `0xFD` Wire Protocol Module & Golden Fixtures Generator

- **Files:**
  - Create: `crates/ghost-protocol/src/dotatv.rs`
  - Modify: `crates/ghost-protocol/src/lib.rs`
  - Create: `crates/ghost-protocol/tests/dotatv_golden.rs`
- **Interfaces:**
  - Produces:
    - `pub const DOTATV_HEADER: u8 = 0xFD;`
    - `pub struct DotaTvCodec;` — **new code.** There is no generic `HeaderCodec<N>` in this workspace; do not reference one. Write `DotaTvCodec` as a `tokio_util::codec::Decoder` + `Encoder<Bytes>` pair modelled directly on the existing `W3gsCodec` in `crates/ghost-protocol/src/w3gs/codec.rs` (same 4-byte header shape, `0xFD` instead of `0xF7`), and reuse that file's `Frame` struct for `{ id, payload }`.
    - `pub struct GameStartSnapshot { pub game_name: String, pub map_path: String, pub map_size: u32, pub map_info_crc: u32, pub map_crc: u32, pub map_sha1: [u8; 20], pub stat_string: Vec<u8>, pub random_seed: u32, pub layout_style: u8, pub player_slots: u8, pub war3_version: u8, pub is_tft: bool, pub slots: Vec<SlotInfo> }`
    - `pub fn encode_hello(version: u16, server_name: &str) -> Result<Bytes, ProtoError>`
    - `pub fn encode_snapshot(snap: &GameStartSnapshot) -> Result<Bytes, ProtoError>`
    - `pub fn decode_snapshot(payload: &[u8]) -> Result<GameStartSnapshot, ProtoError>`
    - `pub fn encode_player(pid: u8, name: &str, colour: u8, team: u8, race: u8) -> Result<Bytes, ProtoError>`
    - `pub fn encode_action(w3gs_raw_frame: &[u8]) -> Result<Bytes, ProtoError>`
    - `pub fn encode_chat(sender: &str, text: &str) -> Result<Bytes, ProtoError>`
    - `pub fn encode_game_over(winner: &str) -> Result<Bytes, ProtoError>`
    - `pub fn encode_history_end(count: u32) -> Result<Bytes, ProtoError>`

- [ ] **Step 1: Write failing unit tests for `0xFD` protocol encoders/decoders**
  In `crates/ghost-protocol/src/dotatv.rs`, test encoding each message type and assert the exact byte array matches the hex examples in §2.2.
- [ ] **Step 2: Implement `crates/ghost-protocol/src/dotatv.rs`**
  Implement the encoders, decoders, and checked parsers (returning `ProtoError::BadValue` on truncated or malformed buffers).
- [ ] **Step 3: Expose module in `crates/ghost-protocol/src/lib.rs`**
  Add `pub mod dotatv;`.
- [ ] **Step 4: Create golden binary fixtures test `crates/ghost-protocol/tests/dotatv_golden.rs`**
  Write test emitting golden `.bin` files into `crates/ghost-protocol/tests/fixtures/dotatv/` (`hello.bin`, `snapshot.bin`, `player.bin`, `action.bin`, `history_end.bin`) used for C++ cross-language testing.
- [ ] **Step 5: Run tests, fmt, and clippy**
  Run `cargo test -p ghost-protocol`, `cargo fmt --all`, and `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 6: Commit**
  ```bash
  git commit -m "feat(protocol): implement 0xFD DotaTV spectator wire protocol and golden fixtures"
  ```

---

### Task 3: Spectator Relay History Buffer, Snapshot & `0xFD` Protocol

- **Files:**
  - Modify: `crates/ghost-spectator/src/relay.rs`
  - Modify: `crates/ghost-spectator/src/lib.rs`
  - Modify: `crates/ghostrs/src/config.rs`
  - Modify: `crates/ghostrs/src/supervisor.rs`
  - Modify: `crates/ghost-engine/src/actions.rs`
- **Interfaces:**
  - Consumes: `ghost_protocol::dotatv::{GameStartSnapshot, encode_hello, encode_snapshot, encode_player, encode_action, encode_chat, encode_game_over, encode_history_end}`.
  - Produces: `RelayCmd::GameStart(GameStartSnapshot)`, `RelayConfig::history_max_mb`.

- [ ] **Step 1: Write failing unit tests in `crates/ghost-spectator/src/relay.rs`**
  Write tests asserting:
  1. `Relay` stores snapshot and player records upon `RelayCmd::GameStart` and `RelayCmd::PlayerInfo`.
  2. Joining viewer receives `HELLO` (0x01), `GAME_START_SNAPSHOT` (0x02), `PLAYER` (0x03) for all seated players, all history `ACTION` packets (0x04), and `HISTORY_END` (0x07).
  3. Action history is capped at `history_max_mb`; excess memory rejects new viewers.
  4. Inbound viewer `Closed` event cleans up viewer list immediately.
- [ ] **Step 2: Update `RelayCmd` and `Relay` struct in `relay.rs`**
  Add `RelayCmd::GameStart(GameStartSnapshot)`. Add `snapshot: Option<GameStartSnapshot>`, `players: Vec<(u8, String, u8, u8, u8)>`, `history: Vec<Bytes>`, `history_bytes: usize` to `Relay`.
- [ ] **Step 3: Implement `run_relay` sequence and `0xFD` stream dispatch**
  On `ViewerJoined`:
  - Send `encode_hello(1, "ghostrs")`.
  - If snapshot is present, send `encode_snapshot(&snap)`.
  - For each player, send `encode_player(pid, name, colour, team, race)`.
  - Burst all `history` packets wrapped in `0x04 ACTION`.
  - Send `encode_history_end(history.len() as u32)`.
  - Register viewer for live delayed broadcasts.
- [ ] **Step 4: Wire `conn_rx` event processing**
  Handle `ConnEventKind::Closed` to unregister viewers, and handle incoming `0x81 CHAT` to broadcast spectator chat.
- [ ] **Step 5: Wire `GameState::begin_playing` to send `RelayCmd::GameStart`**
  In `crates/ghost-engine/src/actions.rs:begin_playing`, construct `GameStartSnapshot` from `self.slots`, `self.random_seed`, and `self.cfg.map`, and dispatch to `self.relay`.
- [ ] **Step 6: Update configuration in `crates/ghostrs/src/config.rs` and `supervisor.rs`**
  Add `history_max_mb: usize` (default `64`) to `SpectatorConfig` and pass to `spawn_relay`.
- [ ] **Step 7: Run tests, fmt, and clippy**
  Run `cargo test --workspace --all-features`, `cargo fmt --all`, and `cargo clippy --workspace --all-targets -- -D warnings`.
- [ ] **Step 8: Commit**
  ```bash
  git commit -m "feat(spectator): add snapshot retention, action history buffer, and 0xFD protocol support"
  ```

---

### Task 4: Pattern Scanner (`SigScan`) & Self-Test in `dotatv_client`

- **Files:**
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\include\SigScan.hpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\src\SigScan.cpp`
  - Modify: `C:\Users\slash\iccwc3_work\dotatv_client\include\GameOffsets.hpp`
  - Modify: `C:\Users\slash\iccwc3_work\dotatv_client\src\DotaTV.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\tests\test_sigscan.cpp`
- **Interfaces:**
  - Produces:
    - `bool SigScan::Initialize(uintptr_t gameDllBase);`
    - `uintptr_t SigScan::FindPattern(const char* pattern, const char* mask);`
    - `bool SigScan::ResolveAll();`
    - Replaces all hardcoded RVAs in `GameOffsets.hpp`:
      - `GlobalClassPtr` (0xAB4F80), `ViewMatrixOffset` (0xAD1640), `UnitVTableAddr` (0x931934), `LocalPlayerPtr` (0xAB65F4)
      - UI functions: `fnGetGameUI` (0x2FA440), `fnCreateSimpleFrame` (0x2FA8E0), `fnGetSimpleFrameByName` (0x2FA9C0), `fnSetFrameAbsolutePos` (0x6056B0), `fnSetFramePoint` (0x606770), `fnSetFrameSize` (0x605D40), `fnSetSimpleFrameTexture` (0x60C2C0), `fnSetSimpleFrameText` (0x614830), `fnShowFrame` (0x605DC0), `fnSetFrameAlpha` (0x605E80), `fnSetStatusBarValue` (0x60D1E0)
      - JASS/Game APIs: `fnGetLocalPlayer` (0x3BBB60), `fnGetPlayerName` (0x3C0F60), `fnGetPlayerColor` (0x3C1240), `fnGetPlayerState` (0x3C9B00), `fnDisplayTextToPlayer` (0x3CB900), `fnCreateTextTag` (0x3BC580), `fnSetTextTagText` (0x3BC5D0), `fnSetTextTagPos` (0x3BC610), `fnSetTextTagColor` (0x3BC6A0), `fnDestroyTextTag` (0x3BC5A0)
      - Camera/Fog/Speed: `fnSetCameraField` (0x3B48B0), `fnSetCameraTargetController` (0x3CD760), `fnFogEnable` (0x3BC630), `fnFogMaskEnable` (0x3BC650), `fnSetGameSpeedScale` (0x3DB270), `fnQuitGame` (0x39F240), `fnEndGame` (0x3BBBB0), `fnGameMainLoop` (0x3B3D90), `sub_6F736FA6` (0x736FA6).

- [ ] **Step 1: Implement `SigScan` scanner and unit test**
  In `include/SigScan.hpp` and `src/SigScan.cpp`, implement fast IDA-style pattern scanning (`\x55\x8B\xEC...` with mask `xxxx...`) over PE `.text` sections. In `tests/test_sigscan.cpp`, verify pattern matching against synthetic memory buffers with wildcards.
- [ ] **Step 2: Delete the symbols that are known-bad or made obsolete by the overlay**

  Do **not** attempt to build a signature for these — a signature derived from a wrong address matches the wrong code. Delete each one from `GameOffsets.hpp` and delete its call sites:

  | Symbol | Listed RVA | Why it is deleted |
  |---|---|---|
  | `fnCreateSimpleFrame` | `0x2FA8E0` | Bytes at that RVA (`85 ff 74 a6`) are a backward jump inside a loop, not a function prologue. Also unnecessary: the connect widget is a D3D8 overlay (Task 6), not a SimpleFrame. |
  | `fnShowFrame` | `0x605DC0` | Bytes (`fc fe ff ff cc cc cc cc`) are a rel32 displacement tail followed by `int3` padding — past the end of a function. |
  | `fnGetGameUI` | `0x2FA440` | Mid-function; consumes a live `ebx`. Calling it from outside its frame is undefined. |
  | `sub_6F736FA6` | `0x736FA6` | `68 60 07 98 6f` is mid-instruction. This was the old `LoadMatch` call — the whole in-memory match-load approach is replaced by `LocalHost`. |
  | `fnGetSimpleFrameByName`, `fnSetFrameAbsolutePos`, `fnSetFramePoint`, `fnSetFrameSize`, `fnSetSimpleFrameTexture`, `fnSetSimpleFrameText`, `fnSetFrameAlpha`, `fnSetStatusBarValue` | various | SimpleFrame UI API. Unused after Task 6: SimpleFrames do not exist on the glue screen, which is where this widget lives. |

  If a deleted symbol still has call sites in `MainMenuUI.cpp` or `SpectatorHUD.cpp`, remove or port those call sites in this step — the project must still compile at the end of the task.

- [ ] **Step 3: Define the signature table for the symbols that are kept**

  Build byte signatures and masks only for the symbols that survive Step 2: `fnGetLocalPlayer` (`0x3BBB60`), `LocalPlayerPtr` (`0xAB65F4`), `fnGetPlayerName` (`0x3C0F60`), `fnGetPlayerColor` (`0x3C1240`), `fnGetPlayerState` (`0x3C9B00`), `fnDisplayTextToPlayer` (`0x3CB900`), `fnCreateTextTag` (`0x3BC580`), `fnSetTextTagText` (`0x3BC5D0`), `fnSetTextTagPos` (`0x3BC610`), `fnSetTextTagColor` (`0x3BC6A0`), `fnDestroyTextTag` (`0x3BC5A0`), `fnSetCameraField` (`0x3B48B0`), `fnSetCameraTargetController` (`0x3CD760`), `fnFogEnable` (`0x3BC630`), `fnFogMaskEnable` (`0x3BC650`), `fnSetGameSpeedScale` (`0x3DB270`), `fnQuitGame` (`0x39F240`), `fnEndGame` (`0x3BBBB0`), `fnGameMainLoop` (`0x3B3D90`), `GlobalClassPtr` (`0xAB4F80`), `ViewMatrixOffset` (`0xAD1640`), `UnitVTableAddr` (`0x931934`).

  For each: read the bytes at that RVA in `game.dll.i64`, confirm it is a real function start (or, for the data pointers, a real reference), take 16–32 bytes, and wildcard every byte that is part of a relocated absolute address or a rel32 displacement. Acceptance for each signature is the same as Task 5 Step 4: it must match exactly one address in `.text`. Any listed symbol that fails that check is deleted like the Step 2 symbols rather than shipped with a loose pattern.
- [ ] **Step 4: Implement `SigScan::ResolveAll()` self-test and logging**
  `ResolveAll()` verifies that every symbol resolves to a non-zero address in `Game.dll`. If any symbol fails to resolve:
  - Write `[ERROR] Failed to resolve symbol: <name>` to `dotatv_client.log`.
  - Abort hook installation and return `false`.
- [ ] **Step 5: Eliminate blanket `__try / __except` blocks**
  Remove all `__try { ... } __except (EXCEPTION_EXECUTE_HANDLER) {}` constructs across `DotaTV.cpp`, `CameraManager.cpp`, `SpectatorHUD.cpp`, and `MainMenuUI.cpp`. Replace with explicit null checks against resolved function pointers.
- [ ] **Step 6: Build C++ tests and verify green**
  Build `dotatv_client/tests/dotatv_tests.vcxproj` and run test executable.
- [ ] **Step 7: Commit**
  ```bash
  git commit -m "feat(client): implement SigScan pattern scanner and eliminate hardcoded RVAs and blind exception handlers"
  ```

---

### Task 5: Reverse Engineer Warcraft III LAN Join Routine (`game.dll.i64`)

- **Files:**
  - Reference: `C:\Users\slash\iccwc3_work\game.dll.i64`
  - Modify: `C:\Users\slash\iccwc3_work\dotatv_client\include\SigScan.hpp`
  - Modify: `C:\Users\slash\iccwc3_work\dotatv_client\src\SigScan.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\tests\test_join_sig.cpp`
- **Interfaces:**
  - Produces:
    - Signature, mask, RVA, calling convention, and argument list for the LAN join routine.
    - Resolved pointer: `extern fnJoinLanGame_t g_fnJoinLanGame;`

  > ## RESOLVED 2026-08-16: null result. Auto-join is deferred; the LAN-advert fallback ships instead.
  >
  > Investigation against the real `war3/game.dll` (12 MB, image base `0x6F000000` — note the
  > `iccwc3_work/game.dll.i64` this plan originally cited is the IDB of a 28 KB stub, not this binary):
  >
  > - `game.dll` imports sockets from **WSOCK32**, not WS2_32. `connect` is at `0x6F86D8C4`.
  > - `connect` has exactly **one** code call site, inside `sub_6F6E1A00` (`0x6F6E1A00`–`0x6F6E1B29`),
  >   which is the low-level TCP primitive: `socket` → `htons` → `connect` → `ioctlsocket`
  >   (non-blocking) → `SetEvent`, under a critical section.
  > - `sub_6F6E1A00` has no vtable entry (searching for its absolute address `00 1A 6E 6F` returns
  >   zero hits), so it is reached only by direct `rel32` calls. Enumerating those requires full
  >   auto-analysis of the 12 MB image.
  > - `w3lh.dll`, a candidate prior-art tool, is packed/stubbed: no imports, nothing to learn from.
  >
  > No cheaply-identifiable `JoinLanGame(ip, port)` entry point exists. Finding one is open-ended,
  > and a wrong guess executes garbage inside the game process.
  >
  > **Operator decision:** ship the LAN-advert path now. `LocalHost` broadcasts `W3GS_GAMEINFO`
  > (0x30) to `127.0.0.1:6112`; the stream appears in the stock Local Area Network tab and the user
  > clicks Join. **Zero `game.dll` calls on the join path**, so nothing on it can crash the game.
  > Auto-join returns as separate follow-up work if the routine is ever pinned down.
  >
  > Task 9 is rewritten accordingly: it orchestrates connect → snapshot → LocalHost → UDP advert,
  > and does NOT call `g_fnJoinLanGame`. The typedef below is retained only as a record of the
  > hypothesis that was tested and not confirmed.
  >
  > **This task is research; its output is not known in advance.** The following typedef is a *hypothesis to test*, not a specification to code against:
  > `typedef void (__thiscall *fnJoinLanGame_t)(void* pLanManager, const char* hostIp, uint16_t port, uint32_t entryKey);`
  > The real calling convention, parameter count, and parameter types are whatever IDA shows. Record the actual signature in this plan (edit this block) before Task 9 consumes it. If the routine turns out to take a packed `sockaddr_in` or a game-list entry struct rather than `(ip, port)`, Task 9's step 4 changes accordingly.
  >
  > **If no single callable join-by-address routine exists**, stop and report rather than forcing it. The fallback — bridge advertises via `W3GS_GAMEINFO` UDP broadcast to `127.0.0.1:6112` and the user clicks the game in the LAN tab — reaches a working spectator with no `game.dll` call at all, at the cost of two extra clicks. That trade is the operator's call, not the implementer's.

- [ ] **Step 1: Analyze `game.dll.i64` in IDA Pro**
  Trace the execution flow from `W3GS_GAMEINFO` (0x30) packet processing and the LAN game list "Join" button handler to locate the internal function that establishes a connection to a host address (`ip:port`).
- [ ] **Step 2: Document deliverable parameters**
  Document the function RVA (relative to `0x6F000000`), calling convention (`__thiscall` with `pLanManager` context), parameter types, and surrounding instruction byte sequence.
- [ ] **Step 3: Construct unique byte pattern and mask**
  Construct a pattern of 16–32 bytes with wildcards for relocations.
- [ ] **Step 4: Write signature uniqueness verification test**
  In `tests/test_join_sig.cpp`, load `Game.dll` (or memory dump) and assert that the pattern matches **exactly one** location in `.text`.
- [ ] **Step 5: Add symbol to `SigScan.cpp`**
  Register `fnJoinLanGame` in `SigScan::ResolveAll()`.
- [ ] **Step 6: Commit**
  ```bash
  git commit -m "feat(client): reverse engineer and register Warcraft III LAN join routine signature"
  ```

---

### Task 6: D3D8 Overlay, WndProc Hook & BLP1 Texture Decoder

- **Files:**
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\include\BLPDecoder.hpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\src\BLPDecoder.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\include\D3D8Hook.hpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\src\D3D8Hook.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\include\OverlayUI.hpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\src\OverlayUI.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\tests\test_blp.cpp`
- **Interfaces:**
  - Consumes: MinHook, Storm.dll (`SFileOpenFileEx`/`SFileReadFile`), `IDirect3DDevice8`.
  - Produces:
    - `bool BLPDecoder::DecodeBLP(const uint8_t* data, size_t size, std::vector<uint32_t>& outPixels, uint32_t& outWidth, uint32_t& outHeight);`
    - `bool D3D8Hook::Install();`
    - `void OverlayUI::Render(IDirect3DDevice8* pDevice);`
    - `bool OverlayUI::OnWndProc(HWND hWnd, UINT uMsg, WPARAM wParam, LPARAM lParam);`

- [ ] **Step 1: Write failing BLP1 decoder unit test**
  In `tests/test_blp.cpp`, test decoding known palettised (BLP1 type 1) and JPEG (BLP1 type 0) files, verifying dimensions and pixel color values.
- [ ] **Step 2: Implement `BLPDecoder`**
  Implement uncompressed / palettised BLP1 parsing (extracting 256-color RGBA palette + pixel indices + alpha bit planes) and JPEG BLP1 decoding into 32-bit ARGB arrays.
- [ ] **Step 3: Implement `D3D8Hook`**
  Locate `IDirect3DDevice8` vtable pointer from Warcraft III's initialized graphics manager (not a dummy device). Install MinHook on `EndScene` (vtable index 35).
- [ ] **Step 4: Implement `OverlayUI` widget**
  Render the Connect widget at top-left of the main menu:
  - Editbox prefilled with last used `ip:port`.
  - Connect button styled with `UI\Widgets\Glues\GlueScreen-Button-Background.blp` and `...-Highlight.blp`.
  - Font text rendered via `ID3DXFont` / `FRIZQT__.TTF`.
  - States: `Disconnected`, `Connecting...`, `Loading N%` (catch-up progress), `Watching - <game name>`, `Error: <reason>`.
- [ ] **Step 5: Implement unified hit-testing and `WndProc` hook**
  In `OverlayUI::OnWndProc`, calculate bounding rectangles using the exact same screen-space transform used in `Render()`. Route mouse clicks and text input to the editbox; block keystrokes from reaching game engine when editbox is active.
- [ ] **Step 6: Run tests and verify green**
  Build and run `test_blp`.
- [ ] **Step 7: Commit**
  ```bash
  git commit -m "feat(client): implement D3D8 EndScene hook, BLP1 texture decoder, and main menu connect widget"
  ```

---

### Task 7: NetClient `0xFD` Wire Protocol Implementation

- **Files:**
  - Modify: `C:\Users\slash\iccwc3_work\dotatv_client\include\NetClient.hpp`
  - Modify: `C:\Users\slash\iccwc3_work\dotatv_client\src\NetClient.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\tests\test_netclient.cpp`
- **Interfaces:**
  - Consumes: `0xFD` protocol definitions, golden binary fixtures (`crates/ghost-protocol/tests/fixtures/dotatv/*.bin`).
  - Produces:
    - `bool NetClient::Connect(const std::string& host, uint16_t port);`
    - `void NetClient::SendSubscribe(uint16_t clientVersion);`
    - `void NetClient::SendSpectatorChat(const std::string& text);`
    - Callback hooks: `SetSnapshotCallback`, `SetPlayerCallback`, `SetActionCallback`, `SetHistoryEndCallback`, `SetGameOverCallback`.

- [ ] **Step 1: Write failing C++ test `test_netclient.cpp` against golden fixtures**
  Load the `.bin` fixtures generated in Task 2 and verify that `NetClient` packet decoder correctly unpacks `HELLO`, `GAME_START_SNAPSHOT`, `PLAYER`, `ACTION`, and `HISTORY_END` with matching fields.
- [ ] **Step 2: Extend `NetClient` with `0xFD` frame buffer and state machine**
  In `src/NetClient.cpp`, implement stream buffering: consume 4-byte header (`0xFD`, `id`, `u16 length`), assemble complete payload, and dispatch to typed callbacks.
- [ ] **Step 3: Implement outgoing encoders**
  Implement `SendSubscribe` (`0x80`) and `SendSpectatorChat` (`0x81`).
- [ ] **Step 4: Run test suite**
  Build and run `test_netclient`.
- [ ] **Step 5: Commit**
  ```bash
  git commit -m "feat(client): implement 0xFD wire protocol framing and golden fixture conformance in NetClient"
  ```

---

### Task 8: `LocalHost` In-DLL W3GS Server Implementation

- **Files:**
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\include\LocalHost.hpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\src\LocalHost.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\tests\test_localhost.cpp`
- **Interfaces:**
  - Consumes: `GameStartSnapshot`, W3GS packet definitions.
  - Produces:
    - `bool LocalHost::Start(uint16_t& outBoundPort);`
    - `void LocalHost::Stop();`
    - `void LocalHost::SetSnapshot(const GameStartSnapshot& snap);`
    - `void LocalHost::AddPlayer(uint8_t pid, const std::string& name, uint8_t colour, uint8_t team, uint8_t race);`
    - `void LocalHost::EnqueueAction(const std::vector<uint8_t>& w3gsActionFrame);`
    - `void LocalHost::MarkHistoryEnd(uint32_t count);`

- [ ] **Step 1: Write failing unit test `test_localhost.cpp` for W3GS packet builders**
  Write tests verifying byte-exact output for:
  - `SLOT_INFO_JOIN` (0x04)
  - `PLAYER_INFO` (0x06)
  - `MAP_CHECK` (0x3D)
  - `COUNTDOWN_START` (0x0A) / `COUNTDOWN_END` (0x0B)
  - `GAME_LOADED_OTHERS` (0x08)
  - Assert exact bytes against `ghost-protocol` output.
- [ ] **Step 2: Implement `LocalHost` TCP server and handshake engine**
  In `LocalHost.cpp`, bind a TCP socket to `127.0.0.1:0` (ephemeral port). Handle client connection sequence:
  1. Receive `REQ_JOIN` (0x1E) → reply with `SLOT_INFO_JOIN` (0x04) with observer slot for client.
  2. Send `PLAYER_INFO` (0x06) for all players.
  3. Send `MAP_CHECK` (0x3D) with map CRC/SHA1.
  4. Receive `MAP_SIZE` (0x42) → send `COUNTDOWN_START` (0x0A) and `COUNTDOWN_END` (0x0B).
  5. Receive `GAME_LOADED_SELF` (0x23) → send `GAME_LOADED_OTHERS` (0x08) for all players.
- [ ] **Step 3: Implement history burst and action streaming**
  Once `GAME_LOADED_SELF` is received, flush all queued historical `INCOMING_ACTION` / `INCOMING_ACTION2` packets into the TCP socket as fast as possible. Transition to streaming live actions.
- [ ] **Step 4: Implement packet dropping for client simulation integrity**
  Unconditionally discard incoming `OUTGOING_ACTION` (0x26) and `CHAT_TO_HOST` (0x28). Answer `PONG_TO_HOST` (0x46) and `OUTGOING_KEEPALIVE` (0x27) locally.
- [ ] **Step 5: Run tests and verify green**
  Build and run `test_localhost`.
- [ ] **Step 6: Commit**
  ```bash
  git commit -m "feat(client): implement in-DLL LocalHost W3GS replay host and packet streamer"
  ```

---

### Task 9: Auto-Join Orchestration & Error State Handling

- **Files:**
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\include\AutoJoin.hpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\src\AutoJoin.cpp`
  - Modify: `C:\Users\slash\iccwc3_work\dotatv_client\src\OverlayUI.cpp`
  - Create: `C:\Users\slash\iccwc3_work\dotatv_client\tests\test_autojoin.cpp`
- **Interfaces:**
  - Consumes: `NetClient`, `LocalHost`, `SigScan`, `fnJoinLanGame`.
  - Produces:
    - `void AutoJoin::StartConnect(const std::string& host, uint16_t port);`
    - `AutoJoinStatus AutoJoin::GetStatus() const;`
    - `float AutoJoin::GetCatchupProgress() const;`

- [ ] **Step 1: Write failing unit test `test_autojoin.cpp` for state transitions**
  Test the state machine through: `Idle` → `ConnectingNet` → `WaitingSnapshot` → `StartingLocalHost` → `InvokingJoin` → `CatchingUp` → `Watching` / `Error`.
- [ ] **Step 2: Implement `AutoJoin` controller**

  Per the Task 5 null result, step 4 is a UDP advert, NOT a `game.dll` call. Nothing in this
  sequence calls into the game, so no step here can fault inside the game process.

  Upon Connect click:
  1. Update UI state to `Connecting...`.
  2. Connect `NetClient` to `host:port`.
  3. On receipt of `GAME_START_SNAPSHOT` and player list, start `LocalHost` on `127.0.0.1:<port>`.
  4. Begin broadcasting `W3GS_GAMEINFO` (0x30) by UDP to `127.0.0.1:6112` every 1500 ms, carrying
     the game name from the snapshot and the `LocalHost` TCP port, so the stream appears in the
     stock Local Area Network tab. Keep broadcasting until the client connects to `LocalHost`.
  5. Set UI state to `Ready - open Local Area Network and join "<game name>"`, so the user knows
     the one manual step that remains.
  6. Calculate catch-up progress `Loading N%` as `received_actions / target_history_count * 100.0f`.
  7. Transition to `Watching - <game_name>` once `HISTORY_END` is reached.
- [ ] **Step 3: Implement comprehensive error handling**
  Define clear failure messages shown on the overlay:
  - Relay connection timeout / refused: `"Error: Cannot reach spectator server"`.
  - Protocol version mismatch: `"Error: Incompatible spectator protocol"`.
  - Map file missing locally: `"Error: Map not found locally"`.
  - Signature resolution failure: `"Error: Game.dll hook failed"`.
- [ ] **Step 4: Connect `AutoJoin` to `OverlayUI`**
  Wire Connect button click to `AutoJoin::StartConnect`, and bind render loop to status/progress getters.
- [ ] **Step 5: Run tests**
  Build and run `test_autojoin`.
- [ ] **Step 6: Commit**
  ```bash
  git commit -m "feat(client): implement auto-join orchestration, catch-up tracking, and UI error handling"
  ```

---

### Task 10: Cross-Language Conformance, Simulation Desync Check & E2E Validation

- **Files:**
  - Create: `ghostrs/crates/ghost-engine/tests/spectator_relay_e2e.rs`
  - Modify: `dotatv_client/tests/test_conformance.cpp`
- **Interfaces:**
  - Validates end-to-end integration across `ghost-engine`, `ghost-spectator`, and `dotatv_client`.

- [ ] **Step 1: Implement Rust-side end-to-end integration test**
  In `crates/ghost-engine/tests/spectator_relay_e2e.rs`:
  1. Spawn a seated game with spectator relay enabled.
  2. Transition game to playing, generate regular and overflow (`incoming_action2`) action ticks.
  3. Connect a mock TCP spectator client speaking `0xFD`.
  4. Assert reception of `HELLO`, `GAME_START_SNAPSHOT`, `PLAYER` records, history `ACTION` packets, and `HISTORY_END`.
- [ ] **Step 2: Run C++ cross-language conformance test suite**
  Run `test_conformance.exe` comparing C++ builders against all generated Rust `.bin` fixtures. Assert 0 byte discrepancies.
- [ ] **Step 3: Implement Replay Action Stream Desync Check**
  Write an automated fixture-driven desync verification:
  - Capture the stream of `INCOMING_ACTION` packets produced by `LocalHost` during a simulated game.
  - Parse the action blocks into `ghost_spectator::ReplayBody` and save as `spectator.w3g`.
  - Compare `spectator.w3g` timeslots against the host's recorded `host.w3g`. Assert 100% timeslot and payload identity.
- [ ] **Step 4: Run full workspace test suite**
  Run `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace --all-features`. Verify zero regressions from baseline.
- [ ] **Step 5: Commit**
  ```bash
  git commit -m "test: add cross-language conformance suite, e2e spectator relay test, and replay desync validation"
  ```

---

## 5. Cross-Language Conformance & End-to-End Acceptance Testing

### 5.1 Golden Fixture Conformance Mechanism

Because the C++ W3GS and `0xFD` builders are compiled separately from Cargo, conformance is verified using binary fixtures:
1. **Rust Generator:** `crates/ghost-protocol/tests/dotatv_golden.rs` instantiates every packet builder (`slot_info_join`, `player_info`, `map_check`, `countdown_start`, `countdown_end`, `incoming_action`, `incoming_action2`, and `0xFD` messages) with deterministic static inputs and writes `.bin` files to `crates/ghost-protocol/tests/fixtures/dotatv/`.
2. **C++ Validator:** `dotatv_client/tests/test_conformance.cpp` loads each `.bin` file, generates the corresponding packet using C++ classes (`LocalHost`, `NetClient`), and performs a byte-by-byte `memcmp` assertion.

### 5.2 Deterministic Simulation Desync Check

**What this test proves, and what it does not.** Comparing the two `.w3g` action streams proves *transport fidelity*: that every action packet the host produced reached the spectator, in order, unmodified — which is exactly what the `actions.rs` overflow bug (Task 1) broke. It does **not** prove simulation determinism. Both replays are recordings of the same input stream; they stay identical even if the spectator's Warcraft III client diverges internally, because a diverging client still records the packets it was sent. The extra-occupied-slot divergence risk in §2.1 is therefore **invisible to this test** and is confirmed only by step 6 of the manual checklist in §6 (watching whether the spectator's view matches the real match). State that limitation in the test's doc comment so no one later mistakes a green run for a desync guarantee.

Desync verification is performed deterministically:
- An end-to-end test executes a 1,000-tick DotA game session with simulated player movements, spell casts, item purchases, and chat messages.
- The ghostrs engine saves `host_match.w3g` using `ghost_spectator::save_replay`.
- The spectator mock client records all timeslots received through `0x04 ACTION` packets and packs `spectator_match.w3g`.
- The acceptance test unpacks both `.w3g` files and asserts:
  1. `host.timeslot_count == spectator.timeslot_count`
  2. For every timeslot `i`: `host.timeslot[i].time_increment == spectator.timeslot[i].time_increment` and `host.timeslot[i].action_bytes == spectator.timeslot[i].action_bytes`.

---

## 6. End-to-End Manual Verification Checklist

Follow these steps on a Windows workstation to verify live spectating end to end:

1. **Prerequisites & Configuration:**
   - Ensure `ghostrs` configuration has spectator relay enabled:
     ```toml
     [spectator]
     enabled = true
     port = 6114
     delay_sec = 120
     max_viewers = 32
     history_max_mb = 64
     ```
2. **Build and Deploy DLL:**
   - Open `C:\Users\slash\iccwc3_work\dotatv_client\dotatv_client.vcxproj` in Visual Studio 2022.
   - Build configuration `Release | Win32`.
   - Copy the output `bin\Release\dotatv_client.dll` to `C:\Users\slash\iccwc3_work\ghostrs\war3\dotatv.mix`.
3. **Start Host Server:**
   - Launch `ghostrs` and host a DotA match. Join players and start the game so it enters `GamePhase::Playing`.
4. **Launch Warcraft III Client:**
   - Launch Warcraft III 1.26a (with `dotatv.mix` in the game root directory).
   - Verify the main menu loads and the **DotaTV Connect** widget appears in the top-left corner with an editbox prefilled with `127.0.0.1:6114`.
5. **Connect to Live Game:**
   - Type `127.0.0.1:6114` into the widget and click **Connect**.
   - Observe the widget transition from `Connecting...` to `Loading N%` as historical game ticks are buffered.
   - Verify Warcraft III automatically transitions through map loading into the in-game spectator view without manual LAN tab navigation.
6. **In-Game Verification:**
   - Verify that all 10 heroes, buildings, creeps, and scoreboard elements match the active match.
   - Verify that hero health/mana bars and spectator UI update accurately in real time behind the configured 120-second delay.
   - Verify that mouse clicks and keypresses do not issue commands or cause simulation desynchronization.
   - Check `dotatv_client.log` in the game directory to ensure zero unhandled exceptions or pattern scan errors occurred.
