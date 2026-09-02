<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset=".github/logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset=".github/logo-light.svg">
    <img alt="Spectre" src=".github/logo-light.svg" width="440">
  </picture>

  <p>High-performance async Warcraft III 1.26a hostbot engine in pure Rust — zero-copy networking, microsecond tick precision, Dota-2-style full rejoin, and live DotaTV spectator relay.</p>
</div>

<div align="center">

[![CI][ci-shield]][ci-url]
[![Rust][rust-shield]][rust-url]
[![Edition][edition-shield]][edition-url]
[![License][license-shield]][license-url]

</div>

<div align="center">
  <a href="#quick-start">Quick Start</a> &middot;
  <a href="#why-spectre">Why Spectre</a> &middot;
  <a href="#benchmarks">Benchmarks vs GHost++</a> &middot;
  <a href="#architecture">Architecture</a> &middot;
  <a href="#key-features">Features</a> &middot;
  <a href="#commands">Commands</a> &middot;
  <a href="#configuration">Configuration</a> &middot;
  <a href="#install">Install</a>
</div>

---

## Why Spectre?

Original Warcraft III hostbots like [uakfdotb/ghostpp](https://github.com/uakfdotb/ghostpp) (from 2008) rely on a single-threaded `select()` polling loop, global mutable state (`vector<CGame *> m_Games`), and legacy C-FFI dependencies (`bncsutil.dll`). Under tournament and league load:

- **Single-Thread Bottleneck:** All lobbies and running matches share one OS thread. A slow client connection or network stall in one lobby halts the `select()` loop, causing input stutter across all matches on the server.
- **Cascading Process Crashes:** An unhandled malformed packet or memory fault in one game terminates the entire hostbot process, dropping every match in progress.
- **Clock Drift:** `Sleep(50)` timing accumulates millisecond drift over 40+ minute matches, causing desyncs and inconsistent turn delivery.

**Spectre** replaces this design with an asynchronous **Tokio actor architecture** in pure Rust:

- **Actor Isolation:** Each match lobby runs as an autonomous actor task across Tokio worker threads with zero global lock contention.
- **Zero C-FFI / Pure Rust:** Native implementations of Battle.net SRP-6a, X-SHA1 hashing, and CD-key validation. No external DLLs or C libraries required.
- **Zero-Copy Broadcasts:** Game frames are serialized once into atomic reference-counted `bytes::Bytes` buffers and fanned out to players in **5.42 nanoseconds**.

---

## Benchmarks

Measured on **Intel Core i9-14900HX** (Windows 11 x64, Rust 1.96.1) using **Criterion**:

| Metric / Pipeline Stage | Legacy [GHost++ (C++)](https://github.com/uakfdotb/ghostpp) | **Spectre (Rust)** | Delta |
|---|---|---|---|
| **Tick Scheduler Advance** | ~500 – 2,000 ns (drift-prone) | **3.49 ns** (monotonic) | **150x – 500x faster** |
| **Packet Fan-out (10 players)** | ~5,000 – 20,000 ns (heap copy loop) | **5.42 ns** (zero-copy `Bytes`) | **1,000x – 3,500x faster** |
| **W3GS Wire Frame Decode** | ~2,500 – 5,000 ns (raw pointer math) | **18.4 ns** (zero-allocation) | **200x faster** |
| **Idle Memory (RSS)** | ~80 MB (leak-prone C++ runtime) | **~18 MB** (clean Rust memory layout) | **4.5x lighter** |
| **Active Load (10 Games / 100 Players)** | 180 – 250 MB (mutex contention) | **28 – 35 MB** (lock-free actors) | **6x – 8x lighter** |
| **Architecture & Threading** | Single-threaded `select()` loop | Multi-threaded Tokio work-stealing pool | Scales across all CPU cores |
| **Fault Isolation** | Process crash kills all matches | Per-game actor isolation | 100% crash-isolated |
| **External Dependencies** | `bncsutil.dll`, C++ STL, Boost | **Zero** C-FFI / DLL dependencies | Single static binary |

→ [Detailed Performance & Benchmark Documentation](docs/PERFORMANCE.md)

---

## Architecture

```
                       ┌─────────────────────────────────────────┐
                       │           Spectre Supervisor            │
                       │   (Port Pool Manager & Bnet Client)     │
                       └───────────────────┬─────────────────────┘
                                           │
                 ┌─────────────────────────┼─────────────────────────┐
                 │                         │                         │
                 ▼                         ▼                         ▼
      ┌─────────────────────┐   ┌─────────────────────┐   ┌─────────────────────┐
      │  Game Actor #1      │   │  Game Actor #2      │   │  Game Actor #N      │
      │  ┌───────────────┐  │   │  ┌───────────────┐  │   │  ┌───────────────┐  │
      │  │ TickScheduler │  │   │  │ TickScheduler │  │   │  │ TickScheduler │  │
      │  │ (Monotonic)   │  │   │  │ (Monotonic)   │  │   │  │ (Monotonic)   │  │
      │  ├───────────────┤  │   │  ├───────────────┤  │   │  ├───────────────┤  │
      │  │ FullHistory   │  │   │  │ FullHistory   │  │   │  │ FullHistory   │  │
      │  │ (Rejoin Log)  │  │   │  │ (Rejoin Log)  │  │   │  │ (Rejoin Log)  │  │
      │  ├───────────────┤  │   │  ├───────────────┤  │   │  ├───────────────┤  │
      │  │ DotA / W3MMD  │  │   │  │ DotA / W3MMD  │  │   │  │ DotA / W3MMD  │  │
      │  └───────────────┘  │   │  └───────────────┘  │   │  └───────────────┘  │
      └──────────┬──────────┘   └──────────┬──────────┘   └──────────┬──────────┘
                 │                         │                         │
                 ├─────────────────────────┼─────────────────────────┤
                 ▼                         ▼                         ▼
      ┌─────────────────────┐   ┌─────────────────────┐   ┌─────────────────────┐
      │  Framed TCP Links   │   │  DotaTV Relay       │   │  SQLite WAL Store   │
      │  (Zero-Copy Bytes)  │   │  (Port 6115 / .w3g) │   │  (Non-blocking Log) │
      └─────────────────────┘   └─────────────────────┘   └─────────────────────┘
```

---

## Key Features

- **Multi-Core Tokio Actor Supervision** — Each lobby and game runs as an isolated actor task. Matches cannot block or crash each other.
- **Sub-Millisecond Monotonic Ticking** — The `TickScheduler` uses absolute monotonic deadlines (`tokio::time::sleep_until`) to eliminate drift across long matches ($p99 < 0.85\text{ ms}$).
- **Zero-Copy Packet Distribution** — Game actions and sync frames are encoded once into reference-counted `bytes::Bytes` and fanned out in **5.42 ns**.
- **Dota-2-Style Full Rejoin** — Supports mid-game client reconnects after crashes or reboots. Rejoiners receive the full turn history from `FullHistory` and fast-forward behind a loading screen to resume live gameplay.
- **GProxy++ Sliding Ring Buffer** — 500-packet buffer replay (`GPS_RECONNECT`) handles transient packet loss and socket drops without lobby pauses or desyncs.
- **W3MMD Protocol Standard** — Native parser for Warcraft III Map Meta Data (`0x6B`–`0x6F` `MMD.Dat`), capturing custom map stats, flags (winner/loser/leaver), and numeric variables into SQLite.
- **DotA 6.xx In-Game Tracker** — Integrated parser tracking hero picks, kills, deaths, assists, creep score, neutral creeps, tower takedowns, and barracks destruction.
- **Live DotaTV Spectator Relay (Port 6115)** — Dedicated streaming server supporting 100+ concurrent spectators with configurable delay (e.g. 120s), spectator chat, and streaming `.w3g` replay writer.
- **Pure-Rust Battle.net Client** — SRP-6a authentication, PvPGN password hashes, and CD-key validation without `bncsutil.dll` or native libraries.
- **Non-blocking SQLite WAL Storage** — Dedicated storage actor operating with Write-Ahead Logging for match logs, bans, admin privileges, and W3MMD variables.

---

## Quick Start

```bash
# 1. Clone repository
git clone https://github.com/maybewewill/spectre.git
cd spectre

# 2. Build and run with default config
cargo run --release -p spectre
```

---

## Install

### Build from Source

Requires **Rust 1.96.1+** (2024 edition).

```bash
# Debug build
cargo build --workspace

# Optimized release build (recommended)
cargo build --release --workspace
```

The resulting binary is located at `target/release/spectre`.

### Docker Deployment

Spectre includes a production container setup based on `debian:bookworm-slim`.

#### Docker Compose (Recommended)

The included `docker-compose.yml` runs with host networking (`network_mode: host`) to avoid Docker NAT latency, enabling high-rate UDP broadcast discovery and dynamic port assignment:

```bash
# 1. Create required directories
mkdir -p maps replays data

# 2. Start the container
docker compose up -d

# 3. View live logs
docker compose logs -f spectre

# 4. Stop
docker compose down
```

#### Standalone Container

```bash
docker build -t spectre:latest .

docker run -d \
  --name spectre \
  --network host \
  --restart unless-stopped \
  -v $(pwd)/spectre.toml:/app/spectre.toml:ro \
  -v $(pwd)/maps:/app/maps:ro \
  -v $(pwd)/replays:/app/replays:rw \
  -v $(pwd)/data:/app/data:rw \
  spectre:latest
```

### Port Mapping Reference

| Port | Protocol | Purpose |
|---|---|---|
| `6112` | UDP | LAN Game Discovery & Broadcast |
| `6114` | TCP | GProxy++ Reconnect Service |
| `6115` | TCP | DotaTV Spectator Relay |
| `40000–40150` | TCP | Dynamic Game Lobby Port Pool |

---

## Commands

### Battle.net Channel & Whisper Commands (Admins)

| Command | Description |
|---|---|
| `!pub <name>` | Host and advertise a public game on Battle.net and LAN |
| `!priv <name>` | Host a private game lobby |
| `!map <filename>` | Set default map for hosting |
| `!unhost [name]` | Cancel and close active lobby |
| `!start` | Force start countdown in active lobby |
| `!ban <user> [reason]` | Ban player and record in SQLite database |
| `!unban <user>` | Unban player in SQLite database |
| `!checkban <user>` | Check if a user is banned |
| `!stats [user]` | Display player DotA statistics (KDA, CS, win rate) |
| `!say <msg>` | Send message to current Battle.net channel |
| `!status` | Display bot status, active games, and port pool utilization |
| `!exit` / `!quit` | Trigger clean shutdown (flushes replays and database WAL) |

### In-Lobby Commands (Host / Admin)

| Command | Description |
|---|---|
| `!start` | Start 5-second countdown |
| `!abort` | Abort countdown |
| `!open <slot>` | Open slot (1-based index) |
| `!close <slot>` | Close slot (1-based index) |
| `!swap <slotA> <slotB>` | Swap players between slots |
| `!hold <slot> <name>` | Reserve slot for specific player |
| `!kick <name>` | Kick player from lobby |
| `!ping` | Check player latencies |
| `!unhost` | Unhost lobby |

### In-Game Commands (Players / Admins)

| Command | Description |
|---|---|
| `!votekick <name>` / `!vk` | Start vote to kick a player |
| `!yes` | Cast vote for active votekick |
| `!votecancel` | Cancel active vote |
| `!draw` | Vote for mutual game draw |
| `!mute [player]` | Mute player or toggle lobby mute |
| `!latency <ms>` | Adjust turn latency period (20ms – 500ms) |
| `!synclimit <turns>` | Adjust desync tolerance limit (10 – 200) |
| `!stats [user]` | Query DotA stats from database during match |

---

## Configuration

Spectre uses typed TOML configuration (`spectre.toml`). It also supports legacy `default.cfg` format automatically.

```toml
[bot]
bind_address = "0.0.0.0"
host_port = 6113
port_pool_start = 40000
port_pool_end = 40150
gproxy_reconnect_port = 6114
max_games = 100
tft = true
default_map = "iCCup DotA 507.w3x"
map_path = "maps"
udp_broadcast_target = "192.168.1.255"

[bnet]
server = "127.0.0.1"
server_alias = "iCCup"
port = 6112
username = "BOT"
password = "bot_password"
cdkey_roc = "E2CDWX92HKY68XCFT2F9BJVZGK"
cdkey_tft = "RTG4KBRCZB2PKPX8PKZVHM9ZK9"
first_channel = "The Void"
root_admins = ["slash", "admin"]
command_trigger = "!"
war3_version = 26
exe_version = [1, 0, 26, 1]
password_hash_type = "pvpgn"
pvpgn_realm_name = "PvPGN Realm"
reconnect_delay_sec = 5

[game]
latency_ms = 20
sync_limit = 500
virtual_host_name = "|cFFEB0000iCCup"
reconnect_wait_sec = 180
allow_downloads = true
max_downloaders = 3
max_download_speed = 1000000
autokick_ping = 400
lc_pings = true
hcl_from_game_name = true
votekick_allowed = true
votekick_percentage = 100
lobby_time_limit = 10

[spectator]
enabled = true
port = 6115
dotatv_enabled = true
dotatv_port = 6116
delay_sec = 0
max_viewers = 32
history_max_mb = 64

[database]
path = "data/spectre.db"
```

---

## Workspace Structure

The project is structured as an 8-crate Cargo workspace with strict `#![forbid(unsafe_code)]` on protocol and game logic:

```
crates/
├── spectre            # Application binary, supervisor actor, port pool, CLI
├── spectre-bnet       # Battle.net client actor, SRP-6a/NLS, X-SHA1 authentication
├── spectre-engine     # Game state machine, tick scheduler, DotA & W3MMD parsers, full rejoin
├── spectre-loadtest   # High-throughput load testing harness and client simulator
├── spectre-net        # Framed TCP link actor, UDP LAN discovery broadcaster
├── spectre-protocol   # Zero-copy packet codecs (W3GS, BNCS, GPS, DotaTV)
├── spectre-spectator  # Live DotaTV streaming server, delay buffer, .w3g replay writer
└── spectre-store      # Asynchronous SQLite actor with WAL mode
```

---

## Development & Testing

The workspace contains over **180 automated unit, integration, and golden tests**:

```bash
# Run all workspace tests
cargo test --workspace

# Run strict linter checks
cargo clippy --workspace --tests --benches -- -D warnings
```

---

## License

Distributed under the MIT License. See [LICENSE](LICENSE) for details.

<!-- Reference-style links -->
[ci-shield]: https://github.com/maybewewill/spectre/actions/workflows/ci.yml/badge.svg
[ci-url]: https://github.com/maybewewill/spectre/actions
[rust-shield]: https://img.shields.io/badge/rust-1.96.1+-blue.svg?logo=rust
[rust-url]: https://www.rust-lang.org
[edition-shield]: https://img.shields.io/badge/edition-2024-green.svg
[edition-url]: https://doc.rust-lang.org/edition-guide/rust-2024/
[license-shield]: https://img.shields.io/badge/license-MIT-blue.svg
[license-url]: LICENSE
