# Ghost-RS

[![CI](https://github.com/slash/ghostrs/actions/workflows/ci.yml/badge.svg)](https://github.com/slash/ghostrs/actions/workflows/ci.yml)
[![Rust](https://img.shields.io/badge/rust-1.96.1+-blue.svg)](https://www.rust-lang.org)
[![Edition](https://img.shields.io/badge/edition-2024-green.svg)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue.svg)](LICENSE)

A high-performance, event-driven, pure-Rust Warcraft III 1.26a (The Frozen Throne) hostbot engine with GProxy++ reconnect support and live DotaTV spectator relay.

---

## Features

- **Actor-Based Event-Driven Architecture:** Each game runs as an isolated actor task owning its mutable state. Zero global mutexes or rwlocks (`Arc<Mutex<Game>>` completely eliminated).
- **Drift-Free Deterministic Ticking:** `TickScheduler` uses absolute deadlines (`tokio::time::sleep_until`) preventing cumulative latency drift, maintaining tick jitter $p99 < 1.0\text{ ms}$.
- **Zero-Copy Action Broadcasting:** Game tick action packets are built and CRC-hashed **once**, then broadcast across all connected players using shared `Bytes` refcount bumps.
- **Dedicated I/O Tasks:** Independent per-socket Reader and Writer tasks; slow or unresponsive clients hit bounded non-blocking queues (`try_send`) and never stall the game tick.
- **GProxy++ Reconnect Support:** Per-player ring buffer replay on reconnection (`GPS_RECONNECT`), transparently restoring lost sessions without desyncing the match.
- **Live DotaTV Spectator Relay:** Spectator server with configurable broadcast delay (e.g. 120s) and `.w3g` replay file writer with zlib chunking.
- **Asynchronous PvPGN BNCS Client:** Full Battle.net protocol implementation supporting NLS / SRP-1 authentication, broken SHA-1 password hashing, chat/whisper commands, and live game advertisement.
- **SQLite WAL Storage:** Dedicated blocking storage actor with write-ahead logging (WAL) for persistent bans, admin records, and game history.
- **Synthetic Load Testing Harness:** Bundled `ghost-loadtest` simulating dozens of simultaneous matches and hundreds of concurrent clients.

---

## Workspace Structure

```
ghostrs/
├── crates/
│   ├── ghost-protocol/      # Pure wire codecs for W3GS, GPS and BNCS (no I/O, no async)
│   ├── ghost-net/           # Dual-framing TCP connection actors and UDP broadcaster
│   ├── ghost-engine/        # Core game actor, tick scheduler, slot & player tables, lag screen
│   ├── ghost-bnet/          # PvPGN Battle.net client actor and game advertiser
│   ├── ghost-spectator/     # DotaTV delayed spectator relay and .w3g replay writer
│   ├── ghost-store/         # SQLite storage actor running in WAL mode
│   ├── ghostrs/             # Application entrypoint, typed config, and supervisor
│   ├── ghost-loadtest/      # Multi-game synthetic load test harness
│   └── ghost-legacy-attic/  # Preserved legacy modules for future v2 scope
├── docs/
│   └── PERFORMANCE.md       # Measured microbenchmarks and KPI verification
└── Cargo.toml               # Workspace configuration
```

---

## Requirements

- **Rust:** `1.96.1` or newer (`edition = "2024"`).
- **Target Platform:** Linux, macOS, or Windows (x64 / ARM64).
- **Warcraft III:** Version `1.26a` (The Frozen Throne).
- **Battle.net Server:** PvPGN or standard Battle.net emulation server.

---

## Getting Started

### 1. Build the Workspace

```bash
# Debug build
cargo build --workspace

# Release build (recommended for hosting)
cargo build --release --workspace
```

### 2. Run Tests

```bash
cargo test --workspace
```

### 3. Run Clippy Linter

```bash
cargo clippy --workspace -- -D warnings
```

---

## Running the Hostbot

Copy or edit `default.cfg` in the project root:

```bash
cargo run --release -p ghostrs
```

### Configuration Options (`default.cfg`)

```ini
# Bot Configuration
bot_hostport = 6112
bot_bindaddress = 0.0.0.0
bot_maxgames = 20
bot_latency = 100
bot_synclimit = 50
bot_reconnectwaittime = 180
bot_virtualhostname = |cFF4080C0Ghost

# Battle.net (PvPGN) Configuration
bnet_server = wc3.theabyss.ru
bnet_serverport = 6112
bnet_username = MyBot
bnet_password = mysecretpassword
bnet_firstchannel = The Abyss
bnet_rootadmin = Slash Admin2
bnet_commandtrigger = !
bnet_custom_war3version = 26

# DotaTV Spectator Relay
spectator_enabled = 1
spectator_port = 6114
spectator_delay = 120
spectator_maxviewers = 32

# Storage
db_path = ghost.db
```

---

## In-Game & Battle.net Commands

### Battle.net Channel / Whisper Commands (Root Admins)

| Command | Description |
|---|---|
| `!pub <name>` | Create and advertise a public game in the channel |
| `!priv <name>` | Create a private game |
| `!unhost` | Unhost and cancel the current lobby |
| `!start` | Force start the lobby countdown |
| `!say <msg>` | Send a chat message to the Battle.net channel |

### In-Lobby Commands (Host / Admin)

| Command | Description |
|---|---|
| `!start` | Start the 5-second countdown |
| `!abort` | Cancel the countdown |
| `!open <slot>` | Open slot number (1-based) |
| `!close <slot>` | Close slot number (1-based) |
| `!swap <slotA> <slotB>` | Swap two slots |
| `!kick <name>` | Kick a player from the lobby |
| `!ping` | Display average pings of all seated players |
| `!unhost` | Unhost the current game |

---

## Performance & Benchmarks

Measured baseline on an **Intel Core i9-14900HX** (24 cores / 32 threads, Windows 11 x64):

| Metric | Target | Measured Baseline |
|---|---|---|
| **Tick Jitter ($p99$)** | $< 2.0\text{ ms}$ | **$0.85\text{ ms}$** |
| **Tick Action Encoding (10 actions)** | $< 2.00\text{ \mu s}$ | **$0.082\text{ \mu s}$ ($82\text{ ns}$)** |
| **Tick Broadcast (10 players)** | $< 5.00\text{ \mu s}$ | **$0.240\text{ \mu s}$ ($240\text{ ns}$)** |
| **1000 W3GS Frame Decode** | $< 100\text{ \mu s}$ | **$18.4\text{ \mu s}$** |
| **Memory Usage ($500$ active players)** | $< 200\text{ MB}$ | **$\sim 28\text{ MB}$** |
| **Dropped Clients / Missed Ticks** | $0$ | **$0$** |

### Running Benchmarks

```bash
cargo bench -p ghost-protocol
cargo bench -p ghost-engine
```

### Running the Load Test Harness

Simulate 50 games with 10 synthetic players each ($500$ clients streaming actions and responding to keepalives for 60 seconds):

```bash
cargo run --release -p ghost-loadtest -- --games 50 --players-per-game 10 --duration 60 --addr 127.0.0.1:6112
```

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
