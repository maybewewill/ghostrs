# Ghost-RS: полный переписывание GHost++ на Rust — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Заменить построчный C++→Rust порт GHost++ на event-driven actor-архитектуру в cargo workspace, дающую детерминированный игровой тик (jitter p99 < 2 мс) и zero-copy рассылку пакетов на 10–50 одновременных игр.

**Architecture:** Cargo workspace из 8 крейтов. `ghost-protocol` — чистые кодеки без I/O и без async. `ghost-net` — по две таски на соединение (reader → `mpsc<Frame>`, writer ← `mpsc<Bytes>`), игровой цикл никогда не ждёт сокет. `ghost-engine` — один актор на игру, владеющий своим состоянием единолично; команды приходят через `mpsc`, тик планируется по абсолютным дедлайнам (`sleep_until`), без накопления дрейфа. Глобальных `Lazy<RwLock<...>>` и `Arc<Mutex<Game>>` нет ни одного. Легаси-крейт остаётся в workspace и собирается, пока его модули не будут вытеснены по одному (strangler).

**Tech Stack:** Rust 1.96.1 / edition 2024, tokio 1.45 (multi-thread), tokio-util 0.7 (`codec::Framed`), bytes 1.10, rusqlite 0.33 (bundled, WAL), tracing 0.1 + tracing-subscriber 0.3, thiserror 2.0, proptest 1.5 (roundtrip-тесты кодеков), criterion 0.5 (бенчмарки).

## Global Constraints

Требования ниже неявно входят в каждую задачу.

- Toolchain: `rustc 1.96.1`, `edition = "2024"`. Не поднимать и не понижать.
- Целевая игра: Warcraft III 1.26a, The Frozen Throne. Сервер: PvPGN (`wc3.theabyss.ru`). Официальный Battle.net и Reforged — вне скоупа.
- Масштаб: 10–50 одновременных игр (~500 игроков) на ноду. Рантайм — обычный tokio multi-thread. Никакого thread-per-core / io_uring / monoio.
- Хранилище: SQLite в режиме WAL через `rusqlite` (bundled). Postgres не добавлять.
- Скоуп v1: ядро хостинга + GProxy++ реконнект + DotaTV спектатор-релей. **Вне v1:** DotA-статистика, W3MMD, веб-API, matchmaking, savegame/`!load`.
- Константы протокола: W3GS header `0xF7`, GPS header `0xF8`, BNCS header `0xFF`. Длина пакета — `u16` little-endian и включает 4-байтовый заголовок.
- Дефолты игры: `latency = 100` мс, `sync_limit = 50` тиков, максимум payload экшенов в пакете — 1400 байт.
- Никаких `println!`/`eprintln!` в библиотечных крейтах — только `tracing`. `unwrap()`/`expect()` запрещены везде, кроме тестов и `main.rs` при старте.
- Никакого `.await` внутри тела игрового тика. Отправка игрокам — только `try_send`.
- Каждый крейт обязан проходить `cargo clippy -p <crate> -- -D warnings`.
- Легаси-крейт `ghostrs-legacy` должен собираться (`cargo check --workspace`) до Task 20.

---

## Структура файлов

Новое (создаётся):

```
Cargo.toml                              # [workspace] + [package] ghostrs-legacy
crates/ghost-protocol/
  src/lib.rs                            # реэкспорты, ProtoError
  src/error.rs                          # ProtoError
  src/bytes_ext.rs                      # чтение/запись C-строк, статстрок
  src/w3gs/mod.rs
  src/w3gs/ids.rs                       # const-ы id пакетов
  src/w3gs/codec.rs                     # Frame + W3gsCodec (Decoder/Encoder)
  src/w3gs/incoming.rs                  # типизированные декодеры
  src/w3gs/outgoing.rs                  # билдеры -> Bytes
  src/w3gs/slot.rs                      # SlotInfo (проводной формат слота)
  src/gps/mod.rs                        # GPS кодек
  src/bncs/mod.rs                       # BNCS кодек
  src/bncs/ids.rs
  src/bncs/incoming.rs
  src/bncs/outgoing.rs
  benches/codec.rs
crates/ghost-net/
  src/lib.rs
  src/conn.rs                           # reader/writer таски, PlayerLink
  src/listener.rs                       # accept-луп
  src/udp.rs                            # LAN broadcast 6112
crates/ghost-engine/
  src/lib.rs
  src/tick.rs                           # TickScheduler (без дрейфа)
  src/slots.rs                          # SlotTable
  src/players.rs                        # PlayerTable, Player
  src/state.rs                          # GameState, GamePhase
  src/handle.rs                         # GameHandle, GameCmd, GameConfig
  src/actor.rs                          # select!-луп
  src/lobby.rs                          # join/leave/slot-логика
  src/actions.rs                        # батчинг экшенов + CRC
  src/lagcheck.rs                       # sync_limit, лаг-скрин
  src/mapxfer.rs                        # раздача карты
  src/chat.rs                           # команды !xxx
  src/lang.rs                           # шаблоны сообщений
  src/gproxy.rs                         # GProxy++ реконнект
crates/ghost-bnet/
  src/lib.rs
  src/client.rs                         # актор BNCS-клиента
  src/auth.rs                           # NLS/SRP + old logon
  src/advert.rs                         # STARTADVEX3 / STOPADV
crates/ghost-store/
  src/lib.rs                            # Store (handle), StoreCmd
  src/schema.rs                         # DDL + миграции
  src/writer.rs                         # blocking-таска-писатель
crates/ghost-spectator/
  src/lib.rs
  src/relay.rs                          # DotaTV релей
  src/replay.rs                         # w3g-writer
crates/ghostrs/
  src/main.rs
  src/config.rs                         # типизированный конфиг из default.cfg
  src/telemetry.rs                      # tracing + метрики
  src/supervisor.rs                     # владелец BNET/игр
crates/ghost-loadtest/
  src/main.rs                           # N синтетических W3GS-клиентов
```

Переезжает из легаси (не переписывается с нуля):

| Откуда | Куда | Задача |
|---|---|---|
| `src/protocol/w3gs.rs` | `crates/ghost-protocol/src/w3gs/` | 3 |
| `src/protocol/gps.rs` | `crates/ghost-protocol/src/gps/mod.rs` | 6 |
| `src/protocol/bncs.rs` | `crates/ghost-protocol/src/bncs/` | 6 |
| `src/engine/slot.rs` | `crates/ghost-engine/src/slots.rs` | 9 |
| `src/engine/sync.rs` | `crates/ghost-engine/src/actions.rs` | 11 |
| `src/engine/gproxy.rs` | `crates/ghost-engine/src/gproxy.rs` | 15 |
| `src/spectator_relay.rs` | `crates/ghost-spectator/src/relay.rs` | 16 |
| `src/ghostdb.rs` | `crates/ghost-store/src/` | 17 |
| `src/lang.rs` | `crates/ghost-engine/src/lang.rs` | 13 |

Удаляются в Task 20: `src/game_base.rs`, `src/game.rs`, `src/ghost.rs`, `src/gameplayer.rs`, `src/gameprotocol.rs`, `src/bnet.rs`, `src/bnetprotocol.rs`, `src/socket.rs`, `src/gameslot.rs`, `src/logger.rs`, `src/engine/`, `src/protocol/`, `src/stats/`, `src/storage/`, `src/spectator/`.

Остаются в легаси-крейте до v2 (не в скоупе): `src/stats_dota.rs`, `src/stats_w3mmd.rs`, `src/savegame.rs`.

---

## Task 1: Workspace, телеметрия, зелёный CI

**Files:**
- Modify: `Cargo.toml` (весь файл)
- Create: `crates/ghost-protocol/Cargo.toml`, `crates/ghost-protocol/src/lib.rs`
- Create: `crates/ghostrs/Cargo.toml`, `crates/ghostrs/src/main.rs`, `crates/ghostrs/src/telemetry.rs`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: ничего.
- Produces: `ghost_protocol` (пустой крейт-цель), `ghostrs::telemetry::init(default_level: &str) -> anyhow::Result<()>`, workspace-зависимости в `[workspace.dependencies]`.

- [ ] **Step 1: Превратить корень в workspace, легаси переименовать**

Заменить `Cargo.toml` целиком:

```toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
edition = "2024"
rust-version = "1.96.1"
authors = ["Ghost-RS Team"]

[workspace.dependencies]
bytes = "1.10.1"
tokio = { version = "1.45.0", features = ["full"] }
tokio-util = { version = "0.7", features = ["codec"] }
thiserror = "2.0"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }
anyhow = "1.0"
rusqlite = { version = "0.33", features = ["bundled"] }
crc32fast = "1.4"
sha1 = "0.10.6"
rand = "0.9.1"
flate2 = "1.1.1"
proptest = "1.5"
criterion = "0.5"

[package]
name = "ghostrs-legacy"
version = "0.2.0"
edition = "2024"
description = "Legacy GHost++ transliteration, retired module-by-module"

[[bin]]
name = "ghostrs-legacy"
path = "src/main.rs"

[dependencies]
byteorder = "1.5.0"
bytes = "1.10.1"
config = "0.15.11"
crc32fast = "1.4"
flate2 = "1.1.1"
libloading = "0.8.7"
log = "0.4.27"
mpq = "0.8"
once_cell = "1.21.3"
paris = { version = "1.5.15", features = ["timestamps", "macros"] }
parking_lot = "0.12"
rand = "0.9.1"
rusqlite = { version = "0.33", features = ["bundled"] }
serde = { version = "1.0.219", features = ["derive"] }
serde_json = "1.0"
sha1 = "0.10.6"
socket2 = "0.5.9"
thiserror = "2.0"
tokio = { version = "1.45.0", features = ["full"] }
uuid = { version = "1.17.0", features = ["v4"] }
```

- [ ] **Step 2: Создать пустой `ghost-protocol`**

`crates/ghost-protocol/Cargo.toml`:

```toml
[package]
name = "ghost-protocol"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
bytes.workspace = true
thiserror.workspace = true
tokio-util.workspace = true
tracing.workspace = true
crc32fast.workspace = true
sha1.workspace = true

[dev-dependencies]
proptest.workspace = true
criterion.workspace = true
```

`crates/ghost-protocol/src/lib.rs`:

```rust
//! Pure wire-format codecs for W3GS, GPS and BNCS. No I/O, no async.
#![forbid(unsafe_code)]
```

- [ ] **Step 3: Создать бинарь `ghostrs` с телеметрией**

`crates/ghostrs/Cargo.toml`:

```toml
[package]
name = "ghostrs"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[[bin]]
name = "ghostrs"
path = "src/main.rs"

[dependencies]
ghost-protocol = { path = "../ghost-protocol" }
tokio.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
anyhow.workspace = true
```

`crates/ghostrs/src/telemetry.rs`:

```rust
use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt};

/// Installs the global tracing subscriber. `default_level` is used when
/// RUST_LOG is unset, e.g. "info" or "ghost_engine=debug,info".
pub fn init(default_level: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level));
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))
}
```

`crates/ghostrs/src/main.rs`:

```rust
mod telemetry;

fn main() -> anyhow::Result<()> {
    telemetry::init("info")?;
    tracing::info!("ghostrs starting");
    Ok(())
}
```

- [ ] **Step 4: Проверить, что собирается весь workspace**

Run: `cargo check --workspace`
Expected: PASS. Легаси-крейт компилируется (с предупреждениями), новые два крейта — тоже.

- [ ] **Step 5: Проверить, что новый бинарь запускается**

Run: `cargo run -p ghostrs`
Expected: одна строка лога `ghostrs starting`, код возврата 0.

- [ ] **Step 6: Добавить CI**

`.github/workflows/ci.yml`:

```yaml
name: ci
on: [push, pull_request]
jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.96.1
        with:
          components: clippy, rustfmt
      - run: cargo check --workspace
      - run: cargo test --workspace --exclude ghostrs-legacy
      - run: cargo clippy -p ghost-protocol -- -D warnings
```

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates .github
git commit -m "build: convert repo to cargo workspace, add ghost-protocol and ghostrs crates"
```

---

## Task 2: Байтовые хелперы и типы ошибок протокола

Warcraft III гоняет по проводу C-строки (null-terminated) и «статстроки» (кодирование, где каждый 8-й байт — маска младших битов). Без надёжных примитивов чтения все декодеры будут паниковать на обрезанном вводе.

**Files:**
- Create: `crates/ghost-protocol/src/error.rs`
- Create: `crates/ghost-protocol/src/bytes_ext.rs`
- Modify: `crates/ghost-protocol/src/lib.rs`

**Interfaces:**
- Consumes: ничего.
- Produces:
  - `ProtoError` (варианты `Truncated { need: usize, have: usize }`, `UnterminatedString`, `BadValue(&'static str)`, `TooLarge(usize)`)
  - `trait BufExt: Buf` → `try_get_u8() -> Result<u8, ProtoError>`, `try_get_u16_le`, `try_get_u32_le`, `try_get_cstring() -> Result<String, ProtoError>`, `try_get_bytes(n: usize) -> Result<Bytes, ProtoError>`
  - `fn put_cstring(buf: &mut BytesMut, s: &str)`
  - `fn encode_statstring(raw: &[u8]) -> Vec<u8>`, `fn decode_statstring(enc: &[u8]) -> Vec<u8>`

- [ ] **Step 1: Написать падающие тесты**

`crates/ghost-protocol/src/bytes_ext.rs` (пока только тесты):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{Bytes, BytesMut};

    #[test]
    fn cstring_roundtrip() {
        let mut buf = BytesMut::new();
        put_cstring(&mut buf, "PlayerOne");
        put_cstring(&mut buf, "");
        assert_eq!(buf.len(), 10 + 1);

        let mut b = buf.freeze();
        assert_eq!(b.try_get_cstring().unwrap(), "PlayerOne");
        assert_eq!(b.try_get_cstring().unwrap(), "");
        assert!(b.try_get_cstring().is_err());
    }

    #[test]
    fn cstring_without_terminator_errors_and_does_not_panic() {
        let mut b = Bytes::from_static(b"nope");
        assert!(matches!(b.try_get_cstring(), Err(ProtoError::UnterminatedString)));
    }

    #[test]
    fn try_get_u32_on_short_buffer_errors() {
        let mut b = Bytes::from_static(&[1, 2]);
        assert!(matches!(
            b.try_get_u32_le(),
            Err(ProtoError::Truncated { need: 4, have: 2 })
        ));
    }

    #[test]
    fn statstring_roundtrip() {
        let raw: Vec<u8> = vec![0x00, 0x01, 0x7F, 0x80, 0xFF, 0x10, 0x00, 0x2A, 0x03];
        let enc = encode_statstring(&raw);
        assert!(!enc.contains(&0u8), "encoded statstring must not contain NUL");
        assert_eq!(decode_statstring(&enc), raw);
    }
}
```

- [ ] **Step 2: Запустить, убедиться что не собирается**

Run: `cargo test -p ghost-protocol`
Expected: FAIL — `cannot find function put_cstring in this scope`.

- [ ] **Step 3: Реализовать `error.rs`**

```rust
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq, Eq)]
pub enum ProtoError {
    #[error("truncated: need {need} bytes, have {have}")]
    Truncated { need: usize, have: usize },
    #[error("string is not NUL-terminated")]
    UnterminatedString,
    #[error("bad value: {0}")]
    BadValue(&'static str),
    #[error("payload too large for u16 length field: {0} bytes")]
    TooLarge(usize),
}
```

- [ ] **Step 4: Реализовать `bytes_ext.rs`**

Вставить над блоком `mod tests`:

```rust
use bytes::{Buf, BufMut, Bytes, BytesMut};
use crate::error::ProtoError;

pub trait BufExt: Buf {
    fn try_get_u8(&mut self) -> Result<u8, ProtoError> {
        if self.remaining() < 1 {
            return Err(ProtoError::Truncated { need: 1, have: self.remaining() });
        }
        Ok(self.get_u8())
    }

    fn try_get_u16_le(&mut self) -> Result<u16, ProtoError> {
        if self.remaining() < 2 {
            return Err(ProtoError::Truncated { need: 2, have: self.remaining() });
        }
        Ok(self.get_u16_le())
    }

    fn try_get_u32_le(&mut self) -> Result<u32, ProtoError> {
        if self.remaining() < 4 {
            return Err(ProtoError::Truncated { need: 4, have: self.remaining() });
        }
        Ok(self.get_u32_le())
    }

    fn try_get_bytes(&mut self, n: usize) -> Result<Bytes, ProtoError> {
        if self.remaining() < n {
            return Err(ProtoError::Truncated { need: n, have: self.remaining() });
        }
        Ok(self.copy_to_bytes(n))
    }

    /// Reads a NUL-terminated string. Non-UTF8 bytes are replaced, never panics.
    fn try_get_cstring(&mut self) -> Result<String, ProtoError> {
        let mut out = Vec::new();
        loop {
            if self.remaining() == 0 {
                return Err(ProtoError::UnterminatedString);
            }
            let b = self.get_u8();
            if b == 0 {
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            out.push(b);
        }
    }
}

impl<T: Buf + ?Sized> BufExt for T {}

pub fn put_cstring(buf: &mut BytesMut, s: &str) {
    buf.put_slice(s.as_bytes());
    buf.put_u8(0);
}

/// Battle.net statstring encoding: each group of 7 bytes is prefixed by a mask
/// byte whose bit (i+1) is set when payload byte i was even; every payload byte
/// then gets its low bit forced to 1 so no NUL can appear inside the string.
pub fn encode_statstring(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len() + raw.len() / 7 + 1);
    for chunk in raw.chunks(7) {
        let mut mask: u8 = 1;
        for (i, &b) in chunk.iter().enumerate() {
            if b % 2 == 0 {
                mask |= 1 << (i + 1);
            }
        }
        out.push(mask);
        for &b in chunk {
            out.push(b | 1);
        }
    }
    out
}

pub fn decode_statstring(enc: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(enc.len());
    let mut i = 0usize;
    while i < enc.len() {
        let mask = enc[i];
        i += 1;
        for j in 0..7usize {
            if i >= enc.len() {
                break;
            }
            let mut b = enc[i];
            if mask & (1 << (j + 1)) != 0 {
                b &= 0xFE;
            }
            out.push(b);
            i += 1;
        }
    }
    out
}
```

- [ ] **Step 5: Подключить модули**

`crates/ghost-protocol/src/lib.rs`:

```rust
//! Pure wire-format codecs for W3GS, GPS and BNCS. No I/O, no async.
#![forbid(unsafe_code)]

pub mod bytes_ext;
pub mod error;

pub use bytes_ext::{BufExt, decode_statstring, encode_statstring, put_cstring};
pub use error::ProtoError;
```

- [ ] **Step 6: Запустить тесты**

Run: `cargo test -p ghost-protocol`
Expected: PASS, 4 теста.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-protocol
git commit -m "feat(protocol): add fallible byte readers, cstring and statstring codecs"
```

---

## Task 3: W3GS фрейм-кодек

Заменяет `src/protocol/w3gs.rs`. Две вещи чиним против него: неизвестный packet id больше не рассинхронизирует поток (в легаси `Err` возвращался **до** `src.advance(4)`), и слишком длинный payload возвращает ошибку вместо тихого обрезания до `u16`.

**Files:**
- Create: `crates/ghost-protocol/src/w3gs/mod.rs`, `ids.rs`, `codec.rs`
- Modify: `crates/ghost-protocol/src/lib.rs`

**Interfaces:**
- Consumes: `ProtoError` (Task 2).
- Produces:
  - `pub const W3GS_HEADER: u8 = 0xF7;`
  - `pub struct Frame { pub id: u8, pub payload: Bytes }`, `Frame::new(id: u8, payload: Bytes) -> Frame`, `Frame::encode(&self) -> Result<Bytes, ProtoError>`
  - `pub struct W3gsCodec;` реализующий `Decoder<Item = Frame, Error = ProtoError>` и `Encoder<Bytes, Error = ProtoError>`
  - модуль `w3gs::ids` со всеми 36 константами

- [ ] **Step 1: Написать падающие тесты**

`crates/ghost-protocol/src/w3gs/codec.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::codec::Decoder;

    #[test]
    fn decodes_one_frame_and_leaves_the_rest() {
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0xF7, 0x1E, 0x06, 0x00, 0xAA, 0xBB]);
        buf.extend_from_slice(&[0xF7, 0x27, 0x04, 0x00]);

        let mut codec = W3gsCodec;
        let f = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, ids::REQ_JOIN);
        assert_eq!(&f.payload[..], &[0xAA, 0xBB]);
        assert_eq!(buf.len(), 4, "second frame must stay in the buffer");
    }

    #[test]
    fn returns_none_until_the_whole_frame_arrives() {
        let mut buf = BytesMut::from(&[0xF7, 0x1E, 0x08, 0x00, 0x01][..]);
        let mut codec = W3gsCodec;
        assert!(codec.decode(&mut buf).unwrap().is_none());
        assert_eq!(buf.len(), 5, "partial frame must not be consumed");
    }

    #[test]
    fn unknown_packet_id_is_consumed_not_desynced() {
        // Regression: legacy src/protocol/w3gs.rs:160 errored before advancing,
        // leaving the byte stream permanently misaligned.
        let mut buf = BytesMut::new();
        buf.extend_from_slice(&[0xF7, 0xEE, 0x05, 0x00, 0x99]);
        buf.extend_from_slice(&[0xF7, 0x27, 0x04, 0x00]);

        let mut codec = W3gsCodec;
        let unknown = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(unknown.id, 0xEE, "unknown ids are forwarded verbatim");
        let next = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(next.id, ids::OUTGOING_KEEPALIVE);
    }

    #[test]
    fn resyncs_after_garbage_prefix() {
        let mut buf = BytesMut::from(&[0x00, 0x11, 0xF7, 0x27, 0x04, 0x00][..]);
        let mut codec = W3gsCodec;
        let f = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, ids::OUTGOING_KEEPALIVE);
    }

    #[test]
    fn length_below_header_size_is_rejected_and_byte_skipped() {
        let mut buf = BytesMut::from(&[0xF7, 0x27, 0x02, 0x00, 0xF7, 0x27, 0x04, 0x00][..]);
        let mut codec = W3gsCodec;
        assert!(codec.decode(&mut buf).is_err());
        let f = codec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, ids::OUTGOING_KEEPALIVE);
    }

    #[test]
    fn oversized_payload_errors_instead_of_truncating() {
        // Regression: legacy encode cast total_len to u16 unchecked.
        let payload = Bytes::from(vec![0u8; 70_000]);
        let frame = Frame::new(ids::MAP_PART, payload);
        assert!(matches!(frame.encode(), Err(ProtoError::TooLarge(70_004))));
    }

    #[test]
    fn encode_decode_roundtrip() {
        let frame = Frame::new(ids::PING_FROM_HOST, Bytes::from_static(&[1, 2, 3, 4]));
        let mut buf = BytesMut::from(&frame.encode().unwrap()[..]);
        let back = W3gsCodec.decode(&mut buf).unwrap().expect("frame");
        assert_eq!(back, frame);
        assert!(buf.is_empty());
    }

    proptest::proptest! {
        #[test]
        fn decoder_never_panics_on_arbitrary_input(data: Vec<u8>) {
            let mut buf = BytesMut::from(&data[..]);
            let mut codec = W3gsCodec;
            for _ in 0..data.len() + 1 {
                let before = buf.len();
                match codec.decode(&mut buf) {
                    Ok(Some(_)) => {}
                    Ok(None) => break,
                    Err(_) => {}
                }
                if buf.len() == before {
                    break;
                }
            }
        }
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-protocol w3gs`
Expected: FAIL — модуля `w3gs` не существует.

- [ ] **Step 3: Реализовать `ids.rs`**

Значения перенесены один в один из `src/protocol/w3gs.rs:20-57`:

```rust
//! W3GS packet identifiers.
pub const PING_FROM_HOST: u8 = 0x01;
pub const SLOT_INFO_JOIN: u8 = 0x04;
pub const REJECT_JOIN: u8 = 0x05;
pub const PLAYER_INFO: u8 = 0x06;
pub const PLAYER_LEAVE_OTHERS: u8 = 0x07;
pub const GAME_LOADED_OTHERS: u8 = 0x08;
pub const SLOT_INFO: u8 = 0x09;
pub const COUNTDOWN_START: u8 = 0x0A;
pub const COUNTDOWN_END: u8 = 0x0B;
pub const INCOMING_ACTION: u8 = 0x0C;
pub const CHAT_FROM_HOST: u8 = 0x0F;
pub const START_LAG: u8 = 0x10;
pub const STOP_LAG: u8 = 0x11;
pub const HOST_KICK_PLAYER: u8 = 0x1C;
pub const REQ_JOIN: u8 = 0x1E;
pub const LEAVE_GAME: u8 = 0x21;
pub const GAME_LOADED_SELF: u8 = 0x23;
pub const OUTGOING_ACTION: u8 = 0x26;
pub const OUTGOING_KEEPALIVE: u8 = 0x27;
pub const CHAT_TO_HOST: u8 = 0x28;
pub const DROP_REQ: u8 = 0x29;
pub const SEARCH_GAME: u8 = 0x2F;
pub const GAME_INFO: u8 = 0x30;
pub const CREATE_GAME: u8 = 0x31;
pub const REFRESH_GAME: u8 = 0x32;
pub const DECREATE_GAME: u8 = 0x33;
pub const CHAT_OTHERS: u8 = 0x34;
pub const PING_FROM_OTHERS: u8 = 0x35;
pub const PONG_TO_OTHERS: u8 = 0x36;
pub const MAP_CHECK: u8 = 0x3D;
pub const START_DOWNLOAD: u8 = 0x3F;
pub const MAP_SIZE: u8 = 0x42;
pub const MAP_PART: u8 = 0x43;
pub const MAP_PART_OK: u8 = 0x44;
pub const PONG_TO_HOST: u8 = 0x46;
pub const INCOMING_ACTION2: u8 = 0x48;
```

- [ ] **Step 4: Реализовать `codec.rs`**

Вставить над блоком `mod tests`:

```rust
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use super::ids;
use crate::error::ProtoError;

pub const W3GS_HEADER: u8 = 0xF7;
const HEADER_LEN: usize = 4;

/// A framed W3GS packet. `payload` excludes the 4-byte header and shares memory
/// with the read buffer, so cloning it is a refcount bump, not a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: u8,
    pub payload: Bytes,
}

impl Frame {
    pub fn new(id: u8, payload: Bytes) -> Self {
        Self { id, payload }
    }

    pub fn encode(&self) -> Result<Bytes, ProtoError> {
        let total = HEADER_LEN + self.payload.len();
        if total > u16::MAX as usize {
            return Err(ProtoError::TooLarge(total));
        }
        let mut buf = BytesMut::with_capacity(total);
        buf.put_u8(W3GS_HEADER);
        buf.put_u8(self.id);
        buf.put_u16_le(total as u16);
        buf.put_slice(&self.payload);
        Ok(buf.freeze())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct W3gsCodec;

impl Decoder for W3gsCodec {
    type Item = Frame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, ProtoError> {
        // Resync: drop anything before the next header byte.
        if !src.is_empty() && src[0] != W3GS_HEADER {
            match src.iter().position(|&b| b == W3GS_HEADER) {
                Some(pos) => src.advance(pos),
                None => {
                    src.clear();
                    return Ok(None);
                }
            }
        }
        if src.len() < HEADER_LEN {
            return Ok(None);
        }

        let id = src[1];
        let total = u16::from_le_bytes([src[2], src[3]]) as usize;

        if total < HEADER_LEN {
            src.advance(1); // skip the bogus header byte so the next call resyncs
            return Err(ProtoError::BadValue("w3gs length below header size"));
        }
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }

        src.advance(HEADER_LEN);
        let payload = src.split_to(total - HEADER_LEN).freeze();
        Ok(Some(Frame { id, payload }))
    }
}

impl Encoder<Bytes> for W3gsCodec {
    type Error = ProtoError;

    /// Frames are pre-encoded once by the engine and broadcast as shared `Bytes`,
    /// so the encoder only appends already-framed data.
    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), ProtoError> {
        dst.reserve(item.len());
        dst.put_slice(&item);
        Ok(())
    }
}

/// True for ids the engine acts on. Unknown ids are still framed and forwarded
/// so the stream never desyncs; the engine decides whether to ignore them.
pub fn is_known_id(id: u8) -> bool {
    matches!(
        id,
        ids::REQ_JOIN
            | ids::LEAVE_GAME
            | ids::GAME_LOADED_SELF
            | ids::OUTGOING_ACTION
            | ids::OUTGOING_KEEPALIVE
            | ids::CHAT_TO_HOST
            | ids::DROP_REQ
            | ids::SEARCH_GAME
            | ids::MAP_SIZE
            | ids::MAP_PART_OK
            | ids::PONG_TO_HOST
    )
}
```

`crates/ghost-protocol/src/w3gs/mod.rs`:

```rust
pub mod codec;
pub mod ids;

pub use codec::{Frame, W3GS_HEADER, W3gsCodec, is_known_id};
```

Добавить `pub mod w3gs;` в `crates/ghost-protocol/src/lib.rs`.

- [ ] **Step 5: Запустить тесты**

Run: `cargo test -p ghost-protocol w3gs`
Expected: PASS, 8 тестов.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-protocol
git commit -m "feat(protocol): add W3GS frame codec with resync and desync-safe unknown ids"
```

---

## Task 4: Типизированные декодеры входящих W3GS-пакетов

Фрейм даёт `id` + `payload`. Здесь появляются структуры для тех пакетов, которые движок реально разбирает. Горячий путь (`OUTGOING_ACTION`, id `0x26`) намеренно оставляет тело экшена как `Bytes` — оно ретранслируется без копирования и без парсинга.

**Files:**
- Create: `crates/ghost-protocol/src/w3gs/incoming.rs`
- Modify: `crates/ghost-protocol/src/w3gs/mod.rs`

**Interfaces:**
- Consumes: `BufExt`, `ProtoError` (Task 2), `Frame`, `ids` (Task 3).
- Produces:
  - `struct ReqJoin { pub host_counter: u32, pub entry_key: u32, pub listen_port: u16, pub peer_key: u32, pub name: String, pub internal_ip: [u8; 4] }` + `ReqJoin::decode(payload: &Bytes) -> Result<ReqJoin, ProtoError>`
  - `struct OutgoingAction { pub crc: u32, pub data: Bytes }` + `OutgoingAction::decode(payload: &Bytes) -> Result<OutgoingAction, ProtoError>`
  - `struct ChatToHost { pub to_pids: Vec<u8>, pub from_pid: u8, pub flag: u8, pub extra: Bytes, pub message: String }` + `ChatToHost::decode`
  - `struct MapSizeReport { pub size_flag: u8, pub map_size: u32 }` + `MapSizeReport::decode`
  - `fn decode_leave_game(payload: &Bytes) -> Result<u32, ProtoError>`
  - `fn decode_keepalive(payload: &Bytes) -> Result<u32, ProtoError>` (возвращает checksum)
  - `fn decode_pong_to_host(payload: &Bytes) -> Result<u32, ProtoError>`
  - `fn decode_map_part_ok(payload: &Bytes) -> Result<u32, ProtoError>`

Проводные раскладки (payload — без 4-байтового заголовка):

| Пакет | Раскладка payload | Легаси-источник |
|---|---|---|
| `REQ_JOIN` 0x1E | `u32` host_counter, `u32` entry_key, `u8` unknown, `u16` listen_port, `u32` peer_key, cstring name, 4 байта unknown, `[u8;4]` internal_ip | `src/gameprotocol.rs:92-104` |
| `OUTGOING_ACTION` 0x26 | `u32` crc, остаток — тело экшена | `src/gameprotocol.rs:117-140` |
| `OUTGOING_KEEPALIVE` 0x27 | `u8` unknown, `u32` checksum | `src/gameprotocol.rs:141-147` |
| `LEAVE_GAME` 0x21 | `u32` reason | `src/gameprotocol.rs:105-111` |
| `CHAT_TO_HOST` 0x28 | `u8` count, `count` байт PID-получателей, `u8` from_pid, `u8` flag, далее по flag: `0x10` → cstring message; `0x11..=0x14` → `u8` byte; `0x20` → `u32` extra + cstring message | `src/gameprotocol.rs:148-173` |
| `MAP_SIZE` 0x42 | 4 байта unknown, `u8` size_flag, `u32` map_size | `src/gameprotocol.rs:189-195` |
| `MAP_PART_OK` 0x44 | `u8` to_pid, `u8` from_pid, 4 байта unknown, `u32` map_size | `src/gameprotocol.rs:196-202` |
| `PONG_TO_HOST` 0x46 | `u32` pong | `src/gameprotocol.rs:203-209` |

- [ ] **Step 1: Сверить раскладки с легаси**

Прочитать `src/gameprotocol.rs:92-209`. Если реальные смещения расходятся с таблицей выше — **легаси авторитетнее** (он играет с настоящими клиентами 1.26a): поправить и таблицу, и тесты, и реализацию, и записать расхождение в сообщение коммита.

- [ ] **Step 2: Написать падающие тесты**

`crates/ghost-protocol/src/w3gs/incoming.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{BufMut, BytesMut};

    fn reqjoin_payload(name: &str) -> Bytes {
        let mut b = BytesMut::new();
        b.put_u32_le(7);           // host counter
        b.put_u32_le(0xDEAD_BEEF); // entry key
        b.put_u8(0);               // unknown
        b.put_u16_le(6112);        // listen port
        b.put_u32_le(0x1234_5678); // peer key
        b.put_slice(name.as_bytes());
        b.put_u8(0);
        b.put_slice(&[0, 0, 0, 0]); // unknown
        b.put_slice(&[192, 168, 1, 50]);
        b.freeze()
    }

    #[test]
    fn decodes_req_join() {
        let p = ReqJoin::decode(&reqjoin_payload("Slash")).unwrap();
        assert_eq!(p.host_counter, 7);
        assert_eq!(p.entry_key, 0xDEAD_BEEF);
        assert_eq!(p.listen_port, 6112);
        assert_eq!(p.peer_key, 0x1234_5678);
        assert_eq!(p.name, "Slash");
        assert_eq!(p.internal_ip, [192, 168, 1, 50]);
    }

    #[test]
    fn req_join_truncated_errors_without_panicking() {
        let full = reqjoin_payload("Slash");
        for cut in 0..full.len() {
            let short = full.slice(0..cut);
            assert!(ReqJoin::decode(&short).is_err(), "cut at {cut} must error");
        }
    }

    #[test]
    fn outgoing_action_keeps_body_zero_copy() {
        let mut b = BytesMut::new();
        b.put_u32_le(0xAABB_CCDD);
        b.put_slice(&[0x10, 0x20, 0x30]);
        let payload = b.freeze();

        let a = OutgoingAction::decode(&payload).unwrap();
        assert_eq!(a.crc, 0xAABB_CCDD);
        assert_eq!(&a.data[..], &[0x10, 0x20, 0x30]);
        // The action body must be a slice of the original buffer, not a copy.
        assert_eq!(a.data.as_ptr(), payload[4..].as_ptr());
    }

    #[test]
    fn decodes_chat_message_flag_0x10() {
        let mut b = BytesMut::new();
        b.put_u8(2);
        b.put_slice(&[3, 4]); // to pids
        b.put_u8(1);          // from pid
        b.put_u8(0x10);       // flag: plain message
        b.put_slice(b"gl hf");
        b.put_u8(0);
        let c = ChatToHost::decode(&b.freeze()).unwrap();
        assert_eq!(c.to_pids, vec![3, 4]);
        assert_eq!(c.from_pid, 1);
        assert_eq!(c.message, "gl hf");
    }

    #[test]
    fn decodes_chat_extra_flag_0x20() {
        let mut b = BytesMut::new();
        b.put_u8(1);
        b.put_slice(&[2]);
        b.put_u8(1);
        b.put_u8(0x20);
        b.put_u32_le(0);      // extra flags (chat scope)
        b.put_slice(b"ally");
        b.put_u8(0);
        let c = ChatToHost::decode(&b.freeze()).unwrap();
        assert_eq!(c.message, "ally");
        assert_eq!(c.extra.len(), 4);
    }

    #[test]
    fn decodes_keepalive_checksum() {
        let mut b = BytesMut::new();
        b.put_u8(0);
        b.put_u32_le(0x0BAD_F00D);
        assert_eq!(decode_keepalive(&b.freeze()).unwrap(), 0x0BAD_F00D);
    }

    #[test]
    fn decodes_map_size_report() {
        let mut b = BytesMut::new();
        b.put_slice(&[0, 0, 0, 0]);
        b.put_u8(1);
        b.put_u32_le(1_234_567);
        let m = MapSizeReport::decode(&b.freeze()).unwrap();
        assert_eq!(m.size_flag, 1);
        assert_eq!(m.map_size, 1_234_567);
    }
}
```

- [ ] **Step 3: Запустить, убедиться что падает**

Run: `cargo test -p ghost-protocol incoming`
Expected: FAIL — `cannot find struct ReqJoin`.

- [ ] **Step 4: Реализовать декодеры**

Вставить над блоком `mod tests`:

```rust
use bytes::Bytes;

use crate::bytes_ext::BufExt;
use crate::error::ProtoError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReqJoin {
    pub host_counter: u32,
    pub entry_key: u32,
    pub listen_port: u16,
    pub peer_key: u32,
    pub name: String,
    pub internal_ip: [u8; 4],
}

impl ReqJoin {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let host_counter = b.try_get_u32_le()?;
        let entry_key = b.try_get_u32_le()?;
        let _unknown = b.try_get_u8()?;
        let listen_port = b.try_get_u16_le()?;
        let peer_key = b.try_get_u32_le()?;
        let name = b.try_get_cstring()?;
        if name.is_empty() {
            return Err(ProtoError::BadValue("empty player name"));
        }
        let _unknown2 = b.try_get_bytes(4)?;
        let ip = b.try_get_bytes(4)?;
        Ok(Self {
            host_counter,
            entry_key,
            listen_port,
            peer_key,
            name,
            internal_ip: [ip[0], ip[1], ip[2], ip[3]],
        })
    }
}

/// A player action. `data` aliases the read buffer: relaying it costs a
/// refcount bump, and the engine never parses the body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingAction {
    pub crc: u32,
    pub data: Bytes,
}

impl OutgoingAction {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        if payload.len() < 4 {
            return Err(ProtoError::Truncated { need: 4, have: payload.len() });
        }
        let crc = u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]);
        Ok(Self { crc, data: payload.slice(4..) })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatToHost {
    pub to_pids: Vec<u8>,
    pub from_pid: u8,
    pub flag: u8,
    /// Extra flags for flag 0x20 (chat scope: all/allies/observers/private).
    pub extra: Bytes,
    pub message: String,
    /// Set for flags 0x11..=0x14 (team/colour/race/handicap change requests).
    pub byte: u8,
}

impl ChatToHost {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let count = b.try_get_u8()? as usize;
        let to_pids = b.try_get_bytes(count)?.to_vec();
        let from_pid = b.try_get_u8()?;
        let flag = b.try_get_u8()?;

        let mut extra = Bytes::new();
        let mut message = String::new();
        let mut byte = 0u8;

        match flag {
            0x10 => message = b.try_get_cstring()?,
            0x11..=0x14 => byte = b.try_get_u8()?,
            0x20 => {
                extra = b.try_get_bytes(4)?;
                message = b.try_get_cstring()?;
            }
            _ => return Err(ProtoError::BadValue("unknown chat-to-host flag")),
        }

        Ok(Self { to_pids, from_pid, flag, extra, message, byte })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapSizeReport {
    pub size_flag: u8,
    pub map_size: u32,
}

impl MapSizeReport {
    pub fn decode(payload: &Bytes) -> Result<Self, ProtoError> {
        let mut b = payload.clone();
        let _unknown = b.try_get_bytes(4)?;
        Ok(Self { size_flag: b.try_get_u8()?, map_size: b.try_get_u32_le()? })
    }
}

pub fn decode_leave_game(payload: &Bytes) -> Result<u32, ProtoError> {
    payload.clone().try_get_u32_le()
}

pub fn decode_keepalive(payload: &Bytes) -> Result<u32, ProtoError> {
    let mut b = payload.clone();
    let _unknown = b.try_get_u8()?;
    b.try_get_u32_le()
}

pub fn decode_pong_to_host(payload: &Bytes) -> Result<u32, ProtoError> {
    payload.clone().try_get_u32_le()
}

pub fn decode_map_part_ok(payload: &Bytes) -> Result<u32, ProtoError> {
    let mut b = payload.clone();
    let _to_pid = b.try_get_u8()?;
    let _from_pid = b.try_get_u8()?;
    let _unknown = b.try_get_bytes(4)?;
    b.try_get_u32_le()
}
```

Добавить `pub mod incoming;` в `crates/ghost-protocol/src/w3gs/mod.rs`.

- [ ] **Step 5: Запустить тесты**

Run: `cargo test -p ghost-protocol incoming`
Expected: PASS, 7 тестов.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-protocol
git commit -m "feat(protocol): decode W3GS reqjoin, action, chat, keepalive and map packets"
```

---

## Task 5: Билдеры исходящих W3GS-пакетов и CRC экшенов

Самая ответственная часть протокола. `INCOMING_ACTION` (0x0C) несёт 2-байтовую CRC от склеенных блоков экшенов — `src/engine/sync.rs:53` её не считает вообще, из-за чего клиенты уходят в десинк. Здесь это чинится и покрывается тестом.

**Files:**
- Create: `crates/ghost-protocol/src/w3gs/slot.rs`
- Create: `crates/ghost-protocol/src/w3gs/outgoing.rs`
- Modify: `crates/ghost-protocol/src/w3gs/mod.rs`

**Interfaces:**
- Consumes: `put_cstring`, `ProtoError`, `Frame`, `ids`.
- Produces:
  - `struct SlotInfo { pub pid: u8, pub download_status: u8, pub slot_status: u8, pub computer: u8, pub team: u8, pub colour: u8, pub race: u8, pub computer_type: u8, pub handicap: u8 }` + `SlotInfo::encode(&self, buf: &mut BytesMut)` (9 байт на слот)
  - `struct ActionBlock { pub pid: u8, pub data: Bytes }`, `ActionBlock::wire_len(&self) -> usize` (= `data.len() + 3`)
  - `fn incoming_action(actions: &[ActionBlock], send_interval: u16) -> Result<Bytes, ProtoError>`
  - `fn incoming_action2(actions: &[ActionBlock]) -> Result<Bytes, ProtoError>`
  - `fn ping_from_host(ticks: u32) -> Bytes`
  - `fn slot_info(slots: &[SlotInfo], random_seed: u32, layout_style: u8, player_slots: u8) -> Result<Bytes, ProtoError>`
  - `fn slot_info_join(pid: u8, port: u16, external_ip: [u8;4], slots: &[SlotInfo], random_seed: u32, layout_style: u8, player_slots: u8) -> Result<Bytes, ProtoError>`
  - `fn reject_join(reason: u32) -> Bytes`
  - `fn player_info(pid: u8, name: &str, external_ip: [u8;4], internal_ip: [u8;4]) -> Result<Bytes, ProtoError>`
  - `fn player_leave_others(pid: u8, left_code: u32) -> Bytes`
  - `fn game_loaded_others(pid: u8) -> Bytes`
  - `fn countdown_start() -> Bytes`, `fn countdown_end() -> Bytes`
  - `fn chat_from_host(from_pid: u8, to_pids: &[u8], flag: u8, extra: &[u8], message: &str) -> Result<Bytes, ProtoError>`
  - `fn start_lag(laggers: &[(u8, u32)]) -> Result<Bytes, ProtoError>` (pid + сколько мс лагает)
  - `fn stop_lag(pid: u8, lag_ms: u32) -> Bytes`
  - `fn map_check(map_path: &str, map_size: u32, map_info: u32, map_crc: u32, map_sha1: [u8;20]) -> Result<Bytes, ProtoError>`
  - `fn start_download(from_pid: u8) -> Bytes`
  - `fn map_part(from_pid: u8, to_pid: u8, start: u32, chunk: &[u8]) -> Result<Bytes, ProtoError>`

Все функции возвращают **полностью обрамлённые** байты (через `Frame::encode`), готовые к отправке как есть.

- [ ] **Step 1: Написать падающий тест на CRC экшенов**

`crates/ghost-protocol/src/w3gs/outgoing.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incoming_action_layout_and_crc() {
        let actions = vec![
            ActionBlock { pid: 1, data: Bytes::from_static(&[0x10, 0x20]) },
            ActionBlock { pid: 2, data: Bytes::from_static(&[0x30]) },
        ];
        let framed = incoming_action(&actions, 100).unwrap();

        // Frame header
        assert_eq!(framed[0], 0xF7);
        assert_eq!(framed[1], ids::INCOMING_ACTION);
        assert_eq!(u16::from_le_bytes([framed[2], framed[3]]) as usize, framed.len());

        // send interval, then 2-byte CRC, then the action blocks
        assert_eq!(u16::from_le_bytes([framed[4], framed[5]]), 100);

        let mut subpacket = BytesMut::new();
        subpacket.put_u8(1);
        subpacket.put_u16_le(2);
        subpacket.put_slice(&[0x10, 0x20]);
        subpacket.put_u8(2);
        subpacket.put_u16_le(1);
        subpacket.put_slice(&[0x30]);

        let full = crc32fast::hash(&subpacket);
        assert_eq!(framed[6], (full & 0xFF) as u8);
        assert_eq!(framed[7], ((full >> 8) & 0xFF) as u8);
        assert_eq!(&framed[8..], &subpacket[..]);
    }

    #[test]
    fn empty_action_tick_still_carries_send_interval() {
        let framed = incoming_action(&[], 100).unwrap();
        assert_eq!(framed.len(), 4 + 2 + 2);
        assert_eq!(u16::from_le_bytes([framed[4], framed[5]]), 100);
    }

    #[test]
    fn incoming_action2_uses_zero_send_interval() {
        let actions = vec![ActionBlock { pid: 1, data: Bytes::from_static(&[9]) }];
        let framed = incoming_action2(&actions).unwrap();
        assert_eq!(framed[1], ids::INCOMING_ACTION2);
        assert_eq!(u16::from_le_bytes([framed[4], framed[5]]), 0);
    }

    #[test]
    fn action_block_wire_len_matches_encoded_size() {
        let a = ActionBlock { pid: 1, data: Bytes::from_static(&[0; 17]) };
        assert_eq!(a.wire_len(), 20);
        let framed = incoming_action(std::slice::from_ref(&a), 100).unwrap();
        assert_eq!(framed.len(), 4 + 2 + 2 + a.wire_len());
    }

    #[test]
    fn slot_info_encodes_nine_bytes_per_slot() {
        let slots = vec![SlotInfo::default(); 12];
        let framed = slot_info(&slots, 42, 0, 12).unwrap();
        // header 4 + u16 blocklen + u8 numslots + 12*9 + u32 seed + u8 layout + u8 playerslots
        assert_eq!(framed.len(), 4 + 2 + 1 + 12 * 9 + 4 + 1 + 1);
        assert_eq!(framed[1], ids::SLOT_INFO);
    }

    #[test]
    fn map_part_over_u16_is_rejected() {
        let chunk = vec![0u8; 70_000];
        assert!(matches!(
            map_part(1, 2, 0, &chunk),
            Err(ProtoError::TooLarge(_))
        ));
    }

    #[test]
    fn player_info_contains_name_and_addresses() {
        let framed = player_info(3, "Slash", [1, 2, 3, 4], [192, 168, 0, 5]).unwrap();
        assert_eq!(framed[1], ids::PLAYER_INFO);
        assert!(framed.windows(5).any(|w| w == b"Slash"));
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-protocol outgoing`
Expected: FAIL — `cannot find function incoming_action`.

- [ ] **Step 3: Реализовать `slot.rs`**

```rust
use bytes::{BufMut, BytesMut};

/// One entry of the W3GS slot table: 9 bytes on the wire.
/// Ported from src/gameslot.rs and src/engine/slot.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotInfo {
    pub pid: u8,
    /// 0..100, or 255 when unknown.
    pub download_status: u8,
    /// 0 = open, 1 = closed, 2 = occupied.
    pub slot_status: u8,
    /// 1 when the slot holds a computer player.
    pub computer: u8,
    pub team: u8,
    pub colour: u8,
    pub race: u8,
    /// 0 = easy, 1 = normal, 2 = insane.
    pub computer_type: u8,
    /// Percentage, normally 100.
    pub handicap: u8,
}

impl SlotInfo {
    pub const WIRE_LEN: usize = 9;

    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u8(self.pid);
        buf.put_u8(self.download_status);
        buf.put_u8(self.slot_status);
        buf.put_u8(self.computer);
        buf.put_u8(self.team);
        buf.put_u8(self.colour);
        buf.put_u8(self.race);
        buf.put_u8(self.computer_type);
        buf.put_u8(self.handicap);
    }
}
```

- [ ] **Step 4: Реализовать `outgoing.rs`**

Вставить над блоком `mod tests`:

```rust
use bytes::{BufMut, Bytes, BytesMut};

use super::codec::Frame;
use super::ids;
use super::slot::SlotInfo;
use crate::bytes_ext::put_cstring;
use crate::error::ProtoError;

/// One player action as it appears inside INCOMING_ACTION:
/// pid (1) + length (2, LE) + body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionBlock {
    pub pid: u8,
    pub data: Bytes,
}

impl ActionBlock {
    pub fn wire_len(&self) -> usize {
        self.data.len() + 3
    }

    fn put(&self, buf: &mut BytesMut) {
        buf.put_u8(self.pid);
        buf.put_u16_le(self.data.len() as u16);
        buf.put_slice(&self.data);
    }
}

fn action_payload(actions: &[ActionBlock], send_interval: u16) -> Result<Bytes, ProtoError> {
    let body_len: usize = actions.iter().map(ActionBlock::wire_len).sum();
    for a in actions {
        if a.data.len() > u16::MAX as usize {
            return Err(ProtoError::TooLarge(a.data.len()));
        }
    }

    let mut sub = BytesMut::with_capacity(body_len);
    for a in actions {
        a.put(&mut sub);
    }

    let mut payload = BytesMut::with_capacity(4 + body_len);
    payload.put_u16_le(send_interval);
    if actions.is_empty() {
        // An empty tick carries no CRC field, matching src/gameprotocol.rs:358.
        return Ok(payload.freeze());
    }
    let crc = crc32fast::hash(&sub);
    payload.put_u8((crc & 0xFF) as u8);
    payload.put_u8(((crc >> 8) & 0xFF) as u8);
    payload.put_slice(&sub);
    Ok(payload.freeze())
}

/// W3GS_INCOMING_ACTION (0x0C): the per-tick action broadcast.
pub fn incoming_action(actions: &[ActionBlock], send_interval: u16) -> Result<Bytes, ProtoError> {
    Frame::new(ids::INCOMING_ACTION, action_payload(actions, send_interval)?).encode()
}

/// W3GS_INCOMING_ACTION2 (0x48): overflow packet, always send_interval 0.
pub fn incoming_action2(actions: &[ActionBlock]) -> Result<Bytes, ProtoError> {
    Frame::new(ids::INCOMING_ACTION2, action_payload(actions, 0)?).encode()
}

pub fn ping_from_host(ticks: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(ticks);
    Frame::new(ids::PING_FROM_HOST, p.freeze())
        .encode()
        .expect("4-byte ping always fits")
}

fn slot_block(slots: &[SlotInfo], random_seed: u32, layout_style: u8, player_slots: u8) -> BytesMut {
    let mut p = BytesMut::with_capacity(3 + slots.len() * SlotInfo::WIRE_LEN + 6);
    let block_len = 1 + slots.len() * SlotInfo::WIRE_LEN + 4 + 1 + 1;
    p.put_u16_le(block_len as u16);
    p.put_u8(slots.len() as u8);
    for s in slots {
        s.encode(&mut p);
    }
    p.put_u32_le(random_seed);
    p.put_u8(layout_style);
    p.put_u8(player_slots);
    p
}

/// W3GS_SLOTINFO (0x09).
pub fn slot_info(
    slots: &[SlotInfo],
    random_seed: u32,
    layout_style: u8,
    player_slots: u8,
) -> Result<Bytes, ProtoError> {
    let p = slot_block(slots, random_seed, layout_style, player_slots);
    Frame::new(ids::SLOT_INFO, p.freeze()).encode()
}

/// W3GS_SLOTINFOJOIN (0x04): slot table plus the joiner's own identity.
pub fn slot_info_join(
    pid: u8,
    port: u16,
    external_ip: [u8; 4],
    slots: &[SlotInfo],
    random_seed: u32,
    layout_style: u8,
    player_slots: u8,
) -> Result<Bytes, ProtoError> {
    let mut p = slot_block(slots, random_seed, layout_style, player_slots);
    p.put_u8(pid);
    p.put_u16_le(2); // AF_INET
    p.put_u16_be(port);
    p.put_slice(&external_ip);
    p.put_slice(&[0; 8]); // sockaddr padding
    Frame::new(ids::SLOT_INFO_JOIN, p.freeze()).encode()
}

pub fn reject_join(reason: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(reason);
    Frame::new(ids::REJECT_JOIN, p.freeze())
        .encode()
        .expect("4-byte reject always fits")
}

/// W3GS_PLAYERINFO (0x06).
pub fn player_info(
    pid: u8,
    name: &str,
    external_ip: [u8; 4],
    internal_ip: [u8; 4],
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(32 + name.len());
    p.put_u32_le(2); // player join counter
    p.put_u8(pid);
    put_cstring(&mut p, name);
    p.put_u8(1); // size of following unknown block
    p.put_u8(0);
    // external sockaddr
    p.put_u16_le(2);
    p.put_u16_be(6112);
    p.put_slice(&external_ip);
    p.put_slice(&[0; 8]);
    // internal sockaddr
    p.put_u16_le(2);
    p.put_u16_be(6112);
    p.put_slice(&internal_ip);
    p.put_slice(&[0; 8]);
    Frame::new(ids::PLAYER_INFO, p.freeze()).encode()
}

pub fn player_leave_others(pid: u8, left_code: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u8(pid);
    p.put_u32_le(left_code);
    Frame::new(ids::PLAYER_LEAVE_OTHERS, p.freeze())
        .encode()
        .expect("5-byte leave always fits")
}

pub fn game_loaded_others(pid: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(1);
    p.put_u8(pid);
    Frame::new(ids::GAME_LOADED_OTHERS, p.freeze())
        .encode()
        .expect("1-byte loaded always fits")
}

pub fn countdown_start() -> Bytes {
    Frame::new(ids::COUNTDOWN_START, Bytes::new())
        .encode()
        .expect("empty frame always fits")
}

pub fn countdown_end() -> Bytes {
    Frame::new(ids::COUNTDOWN_END, Bytes::new())
        .encode()
        .expect("empty frame always fits")
}

/// W3GS_CHAT_FROM_HOST (0x0F).
pub fn chat_from_host(
    from_pid: u8,
    to_pids: &[u8],
    flag: u8,
    extra: &[u8],
    message: &str,
) -> Result<Bytes, ProtoError> {
    if to_pids.is_empty() {
        return Err(ProtoError::BadValue("chat_from_host needs at least one recipient"));
    }
    let mut p = BytesMut::with_capacity(4 + to_pids.len() + extra.len() + message.len());
    p.put_u8(to_pids.len() as u8);
    p.put_slice(to_pids);
    p.put_u8(from_pid);
    p.put_u8(flag);
    p.put_slice(extra);
    put_cstring(&mut p, message);
    Frame::new(ids::CHAT_FROM_HOST, p.freeze()).encode()
}

/// W3GS_START_LAG (0x10): pid plus how long that player has been lagging.
pub fn start_lag(laggers: &[(u8, u32)]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(1 + laggers.len() * 5);
    p.put_u8(laggers.len() as u8);
    for &(pid, lag_ms) in laggers {
        p.put_u8(pid);
        p.put_u32_le(lag_ms);
    }
    Frame::new(ids::START_LAG, p.freeze()).encode()
}

pub fn stop_lag(pid: u8, lag_ms: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u8(pid);
    p.put_u32_le(lag_ms);
    Frame::new(ids::STOP_LAG, p.freeze())
        .encode()
        .expect("5-byte stoplag always fits")
}

/// W3GS_MAPCHECK (0x3D).
pub fn map_check(
    map_path: &str,
    map_size: u32,
    map_info: u32,
    map_crc: u32,
    map_sha1: [u8; 20],
) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(40 + map_path.len());
    p.put_u32_le(1);
    put_cstring(&mut p, map_path);
    p.put_u32_le(map_size);
    p.put_u32_le(map_info);
    p.put_u32_le(map_crc);
    p.put_slice(&map_sha1);
    Frame::new(ids::MAP_CHECK, p.freeze()).encode()
}

pub fn start_download(from_pid: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u32_le(1);
    p.put_u8(from_pid);
    Frame::new(ids::START_DOWNLOAD, p.freeze())
        .encode()
        .expect("5-byte startdownload always fits")
}

/// W3GS_MAPPART (0x43). `chunk` must be at most 1442 bytes; the CRC covers it.
pub fn map_part(from_pid: u8, to_pid: u8, start: u32, chunk: &[u8]) -> Result<Bytes, ProtoError> {
    let mut p = BytesMut::with_capacity(14 + chunk.len());
    p.put_u8(to_pid);
    p.put_u8(from_pid);
    p.put_u32_le(1);
    p.put_u32_le(start);
    p.put_u32_le(crc32fast::hash(chunk));
    p.put_slice(chunk);
    Frame::new(ids::MAP_PART, p.freeze()).encode()
}
```

Добавить в `crates/ghost-protocol/src/w3gs/mod.rs`:

```rust
pub mod incoming;
pub mod outgoing;
pub mod slot;

pub use outgoing::ActionBlock;
pub use slot::SlotInfo;
```

- [ ] **Step 5: Сверить с легаси**

Прочитать `src/gameprotocol.rs:210-660` и сверить порядок полей для `SLOTINFOJOIN`, `PLAYERINFO`, `MAPCHECK`, `MAPPART`, `START_LAG`. Расхождения — править по легаси (он проверен живыми клиентами), тесты обновить.

- [ ] **Step 6: Запустить тесты**

Run: `cargo test -p ghost-protocol outgoing`
Expected: PASS, 7 тестов.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-protocol
git commit -m "feat(protocol): build outgoing W3GS packets, fix missing action CRC"
```

---

## Task 6: Обобщённый фрейминг, GPS- и BNCS-кодеки

W3GS, GPS и BNCS используют один и тот же формат обрамления — `[header_byte, id, u16 LE длина-включая-заголовок, payload]`, отличается только байт заголовка (`0xF7` / `0xF8` / `0xFF`). Вместо трёх копий декодера делаем один параметризованный константой и три псевдонима. Тесты из Task 3 обязаны продолжать проходить без изменений — это и есть проверка рефакторинга.

**Files:**
- Create: `crates/ghost-protocol/src/frame.rs`
- Create: `crates/ghost-protocol/src/gps/mod.rs`
- Create: `crates/ghost-protocol/src/bncs/mod.rs`, `crates/ghost-protocol/src/bncs/ids.rs`
- Modify: `crates/ghost-protocol/src/w3gs/codec.rs`, `crates/ghost-protocol/src/lib.rs`

**Interfaces:**
- Consumes: `ProtoError`, `BufExt`.
- Produces:
  - `struct Frame { pub id: u8, pub payload: Bytes }` (переезжает из `w3gs::codec`), `Frame::encode_with(header: u8) -> Result<Bytes, ProtoError>`
  - `struct HeaderCodec<const H: u8>;` реализующий `Decoder<Item = Frame, Error = ProtoError>` и `Encoder<Bytes, Error = ProtoError>`
  - `pub type W3gsCodec = HeaderCodec<0xF7>;`, `pub type GpsCodec = HeaderCodec<0xF8>;`, `pub type BncsCodec = HeaderCodec<0xFF>;`
  - `gps::{GPS_HEADER, ids as gps_ids, init, reconnect, ack, reject, decode_reconnect, ReconnectReq}`
  - `bncs::ids` — константы SID-пакетов

- [ ] **Step 1: Написать падающие тесты на общий кодек**

`crates/ghost-protocol/src/frame.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_util::codec::Decoder;

    type Gps = HeaderCodec<0xF8>;
    type Bncs = HeaderCodec<0xFF>;

    #[test]
    fn gps_and_bncs_frame_independently() {
        let mut buf = BytesMut::from(&[0xF8, 0x02, 0x05, 0x00, 0x7B][..]);
        let f = Gps::default().decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, 0x02);
        assert_eq!(&f.payload[..], &[0x7B]);

        let mut buf = BytesMut::from(&[0xFF, 0x50, 0x04, 0x00][..]);
        let f = Bncs::default().decode(&mut buf).unwrap().expect("frame");
        assert_eq!(f.id, 0x50);
        assert!(f.payload.is_empty());
    }

    #[test]
    fn a_bncs_frame_is_not_mistaken_for_a_gps_frame() {
        // 0xFF is not the GPS header, so the GPS codec must resync past it and
        // then find nothing rather than decoding a bogus frame.
        let mut buf = BytesMut::from(&[0xFF, 0x50, 0x04, 0x00][..]);
        assert!(Gps::default().decode(&mut buf).unwrap().is_none());
        assert!(buf.is_empty(), "unusable bytes must be discarded");
    }

    #[test]
    fn encode_with_uses_the_requested_header() {
        let f = Frame::new(0x02, Bytes::from_static(&[1]));
        assert_eq!(&f.encode_with(0xF8).unwrap()[..], &[0xF8, 0x02, 0x05, 0x00, 0x01]);
        assert_eq!(&f.encode_with(0xFF).unwrap()[..], &[0xFF, 0x02, 0x05, 0x00, 0x01]);
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-protocol frame`
Expected: FAIL — модуля `frame` нет.

- [ ] **Step 3: Реализовать `frame.rs`**

Перенести тело `Frame` и декодера из `w3gs/codec.rs`, заменив константу заголовка на параметр:

```rust
use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use crate::error::ProtoError;

pub const HEADER_LEN: usize = 4;

/// A framed packet. `payload` excludes the 4-byte header and shares memory with
/// the read buffer, so cloning it is a refcount bump, not a copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub id: u8,
    pub payload: Bytes,
}

impl Frame {
    pub fn new(id: u8, payload: Bytes) -> Self {
        Self { id, payload }
    }

    pub fn encode_with(&self, header: u8) -> Result<Bytes, ProtoError> {
        let total = HEADER_LEN + self.payload.len();
        if total > u16::MAX as usize {
            return Err(ProtoError::TooLarge(total));
        }
        let mut buf = BytesMut::with_capacity(total);
        buf.put_u8(header);
        buf.put_u8(self.id);
        buf.put_u16_le(total as u16);
        buf.put_slice(&self.payload);
        Ok(buf.freeze())
    }
}

/// Length-prefixed framing shared by W3GS (0xF7), GPS (0xF8) and BNCS (0xFF).
#[derive(Debug, Default, Clone, Copy)]
pub struct HeaderCodec<const H: u8>;

impl<const H: u8> Decoder for HeaderCodec<H> {
    type Item = Frame;
    type Error = ProtoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Frame>, ProtoError> {
        if !src.is_empty() && src[0] != H {
            match src.iter().position(|&b| b == H) {
                Some(pos) => src.advance(pos),
                None => {
                    src.clear();
                    return Ok(None);
                }
            }
        }
        if src.len() < HEADER_LEN {
            return Ok(None);
        }

        let id = src[1];
        let total = u16::from_le_bytes([src[2], src[3]]) as usize;

        if total < HEADER_LEN {
            src.advance(1);
            return Err(ProtoError::BadValue("frame length below header size"));
        }
        if src.len() < total {
            src.reserve(total - src.len());
            return Ok(None);
        }

        src.advance(HEADER_LEN);
        let payload = src.split_to(total - HEADER_LEN).freeze();
        Ok(Some(Frame { id, payload }))
    }
}

impl<const H: u8> Encoder<Bytes> for HeaderCodec<H> {
    type Error = ProtoError;

    /// Packets are pre-encoded once and broadcast as shared `Bytes`, so the
    /// encoder only appends already-framed data.
    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), ProtoError> {
        dst.reserve(item.len());
        dst.put_slice(&item);
        Ok(())
    }
}
```

- [ ] **Step 4: Переписать `w3gs/codec.rs` на общий кодек**

Заменить всё, кроме `mod tests` и `is_known_id`, на:

```rust
use bytes::Bytes;

use crate::error::ProtoError;
use crate::frame::{Frame as RawFrame, HeaderCodec};
use super::ids;

pub const W3GS_HEADER: u8 = 0xF7;
pub type W3gsCodec = HeaderCodec<W3GS_HEADER>;

/// W3GS-flavoured frame: same shape as the shared one, header fixed to 0xF7.
pub type Frame = RawFrame;

pub trait W3gsFrameExt {
    fn encode(&self) -> Result<Bytes, ProtoError>;
}

impl W3gsFrameExt for RawFrame {
    fn encode(&self) -> Result<Bytes, ProtoError> {
        self.encode_with(W3GS_HEADER)
    }
}
```

В `mod tests` файла `w3gs/codec.rs` добавить `use crate::w3gs::codec::W3gsFrameExt;`, а `W3gsCodec` в тестах заменить на `W3gsCodec::default()` там, где он использовался как unit-структура (`W3gsCodec.decode(...)` → `W3gsCodec::default().decode(...)`). Импорт `W3gsFrameExt` также добавить в `w3gs/outgoing.rs`.

- [ ] **Step 5: Запустить тесты Task 3 и Task 5 — регрессия**

Run: `cargo test -p ghost-protocol`
Expected: PASS, все тесты из задач 2–5 плюс 3 новых из `frame`. Ни один тест не изменён по смыслу.

- [ ] **Step 6: Реализовать `gps/mod.rs`**

Раскладки перенести из `src/gpsprotocol.rs` и `src/protocol/gps.rs`:

```rust
use bytes::{BufMut, Bytes, BytesMut};

use crate::bytes_ext::BufExt;
use crate::error::ProtoError;
use crate::frame::{Frame, HeaderCodec};

pub const GPS_HEADER: u8 = 0xF8;
pub type GpsCodec = HeaderCodec<GPS_HEADER>;

pub mod ids {
    pub const INIT: u8 = 0x01;
    pub const RECONNECT: u8 = 0x02;
    pub const ACK: u8 = 0x03;
    pub const REJECT: u8 = 0x04;
}

pub mod reject_reason {
    /// The game the client tried to rejoin is gone.
    pub const NOT_FOUND: u32 = 0x01;
    /// The reconnect key did not match.
    pub const INVALID_KEY: u32 = 0x02;
}

/// Sent by the bot to advertise GProxy support and the reconnect parameters.
pub fn init(version: u32, pid: u8, reconnect_key: u32, num_empty_actions: u8) -> Bytes {
    let mut p = BytesMut::with_capacity(10);
    p.put_u32_le(version);
    p.put_u8(pid);
    p.put_u32_le(reconnect_key);
    p.put_u8(num_empty_actions);
    Frame::new(ids::INIT, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("10-byte gps init always fits")
}

/// Acknowledges how many packets the bot has received from this client.
pub fn ack(last_packet: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(last_packet);
    Frame::new(ids::ACK, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("4-byte gps ack always fits")
}

pub fn reject(reason: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(4);
    p.put_u32_le(reason);
    Frame::new(ids::REJECT, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("4-byte gps reject always fits")
}

/// A client asking to resume a dropped session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectReq {
    pub pid: u8,
    pub reconnect_key: u32,
    /// How many packets the client has already received from the bot.
    pub last_packet: u32,
}

pub fn decode_reconnect(payload: &Bytes) -> Result<ReconnectReq, ProtoError> {
    let mut b = payload.clone();
    Ok(ReconnectReq {
        pid: b.try_get_u8()?,
        reconnect_key: b.try_get_u32_le()?,
        last_packet: b.try_get_u32_le()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_roundtrip() {
        let mut p = BytesMut::new();
        p.put_u8(3);
        p.put_u32_le(0xCAFE_BABE);
        p.put_u32_le(1234);
        let r = decode_reconnect(&p.freeze()).unwrap();
        assert_eq!(r.pid, 3);
        assert_eq!(r.reconnect_key, 0xCAFE_BABE);
        assert_eq!(r.last_packet, 1234);
    }

    #[test]
    fn init_is_framed_with_the_gps_header() {
        let b = init(1, 3, 0xCAFE_BABE, 0);
        assert_eq!(b[0], GPS_HEADER);
        assert_eq!(b[1], ids::INIT);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
    }

    #[test]
    fn truncated_reconnect_errors() {
        assert!(decode_reconnect(&Bytes::from_static(&[3, 0, 0])).is_err());
    }
}
```

- [ ] **Step 7: Реализовать `bncs/ids.rs` и `bncs/mod.rs`**

Идентификаторы перенести из `src/bnetprotocol.rs` (константы `SID_*`). Минимум, нужный для логина и рекламы игр:

```rust
//! Battle.net (BNCS) packet identifiers, ported from src/bnetprotocol.rs.
pub const SID_NULL: u8 = 0x00;
pub const SID_STOPADV: u8 = 0x02;
pub const SID_GETADVLISTEX: u8 = 0x09;
pub const SID_ENTERCHAT: u8 = 0x0A;
pub const SID_JOINCHANNEL: u8 = 0x0C;
pub const SID_CHATCOMMAND: u8 = 0x0E;
pub const SID_CHATEVENT: u8 = 0x0F;
pub const SID_CHECKAD: u8 = 0x15;
pub const SID_STARTADVEX3: u8 = 0x1C;
pub const SID_NOTIFYJOIN: u8 = 0x22;
pub const SID_PING: u8 = 0x25;
pub const SID_LOGONRESPONSE: u8 = 0x29;
pub const SID_NETGAMEPORT: u8 = 0x45;
pub const SID_AUTH_INFO: u8 = 0x50;
pub const SID_AUTH_CHECK: u8 = 0x51;
pub const SID_AUTH_ACCOUNTLOGON: u8 = 0x53;
pub const SID_AUTH_ACCOUNTLOGONPROOF: u8 = 0x54;
pub const SID_WARDEN: u8 = 0x5E;
pub const SID_FRIENDSLIST: u8 = 0x65;
pub const SID_CLANMEMBERLIST: u8 = 0x7D;
pub const SID_CLANMEMBERSTATUSCHANGE: u8 = 0x7F;
```

`crates/ghost-protocol/src/bncs/mod.rs`:

```rust
pub mod ids;

use crate::frame::HeaderCodec;

pub const BNCS_HEADER: u8 = 0xFF;
pub type BncsCodec = HeaderCodec<BNCS_HEADER>;
```

Тела BNCS-пакетов (auth, chat, реклама игры) строятся в Task 14, где живёт вся логика логина.

- [ ] **Step 8: Обновить `lib.rs`**

```rust
//! Pure wire-format codecs for W3GS, GPS and BNCS. No I/O, no async.
#![forbid(unsafe_code)]

pub mod bncs;
pub mod bytes_ext;
pub mod error;
pub mod frame;
pub mod gps;
pub mod w3gs;

pub use bytes_ext::{BufExt, decode_statstring, encode_statstring, put_cstring};
pub use error::ProtoError;
pub use frame::{Frame, HeaderCodec};
```

- [ ] **Step 9: Запустить весь набор тестов крейта**

Run: `cargo test -p ghost-protocol && cargo clippy -p ghost-protocol -- -D warnings`
Expected: PASS. Всего ~21 тест, clippy без предупреждений.

- [ ] **Step 10: Commit**

```bash
git add crates/ghost-protocol
git commit -m "refactor(protocol): unify framing behind HeaderCodec, add GPS and BNCS codecs"
```

---

## Task 7: Сетевой слой — по две таски на соединение

Ключевое отличие от легаси. В `src/game_base.rs:1040` (`send_bytes_all`) игровой цикл последовательно `await`-ит запись в сокет каждого игрока: один тормозящий клиент задерживает тик для всех остальных. Здесь writer вынесен в отдельную таску, а игровой цикл только кладёт `Bytes` в неблокирующую очередь через `try_send`.

**Files:**
- Create: `crates/ghost-net/Cargo.toml`
- Create: `crates/ghost-net/src/lib.rs`, `src/conn.rs`, `src/listener.rs`, `src/udp.rs`

**Interfaces:**
- Consumes: `ghost_protocol::{Frame, w3gs::W3gsCodec, ProtoError}`.
- Produces:
  - `struct PlayerLink { tx: mpsc::Sender<Bytes> }` + `PlayerLink::try_send(&self, bytes: Bytes) -> Result<(), LinkError>`, `PlayerLink::is_closed(&self) -> bool`
  - `enum LinkError { Backpressure, Closed }`
  - `struct ConnEvent { pub conn_id: u64, pub kind: ConnEventKind }`
  - `enum ConnEventKind { Frame(Frame), Closed(CloseReason) }`
  - `enum CloseReason { PeerClosed, Protocol(ProtoError), Io(String), WriterBackpressure }`
  - `fn spawn_conn(conn_id: u64, stream: TcpStream, events: mpsc::Sender<ConnEvent>, write_capacity: usize) -> PlayerLink`
  - `fn spawn_listener(addr: SocketAddr, out: mpsc::Sender<(u64, TcpStream, SocketAddr)>) -> JoinHandle<io::Result<()>>`
  - `async fn spawn_udp_broadcaster(port: u16) -> io::Result<UdpBroadcaster>` + `UdpBroadcaster::send(&self, packet: &Bytes) -> io::Result<()>`

- [ ] **Step 1: Создать крейт**

`crates/ghost-net/Cargo.toml`:

```toml
[package]
name = "ghost-net"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
ghost-protocol = { path = "../ghost-protocol" }
bytes.workspace = true
tokio.workspace = true
tokio-util.workspace = true
tracing.workspace = true
thiserror.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }
```

- [ ] **Step 2: Написать падающие тесты**

`crates/ghost-net/src/conn.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ghost_protocol::w3gs::ids;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    async fn connected_pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let client = TcpStream::connect(addr);
        let server = l.accept();
        let (client, (server, _)) = tokio::join!(client, server);
        (client.unwrap(), server)
    }

    #[tokio::test]
    async fn inbound_frames_reach_the_event_channel() {
        let (mut client, server) = connected_pair().await;
        let (tx, mut rx) = mpsc::channel(16);
        let _link = spawn_conn(1, server, tx, 8);

        client.write_all(&[0xF7, 0x27, 0x09, 0x00, 0, 1, 2, 3, 4]).await.unwrap();

        let ev = rx.recv().await.expect("event");
        assert_eq!(ev.conn_id, 1);
        match ev.kind {
            ConnEventKind::Frame(f) => {
                assert_eq!(f.id, ids::OUTGOING_KEEPALIVE);
                assert_eq!(f.payload.len(), 5);
            }
            other => panic!("expected frame, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn outbound_bytes_reach_the_socket() {
        let (mut client, server) = connected_pair().await;
        let (tx, _rx) = mpsc::channel(16);
        let link = spawn_conn(1, server, tx, 8);

        link.try_send(Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00])).unwrap();

        let mut buf = [0u8; 4];
        client.read_exact(&mut buf).await.unwrap();
        assert_eq!(buf, [0xF7, 0x0B, 0x04, 0x00]);
    }

    #[tokio::test]
    async fn peer_disconnect_produces_a_close_event() {
        let (client, server) = connected_pair().await;
        let (tx, mut rx) = mpsc::channel(16);
        let _link = spawn_conn(7, server, tx, 8);

        drop(client);

        let ev = rx.recv().await.expect("event");
        assert_eq!(ev.conn_id, 7);
        assert!(matches!(ev.kind, ConnEventKind::Closed(CloseReason::PeerClosed)));
    }

    #[tokio::test]
    async fn a_full_write_queue_reports_backpressure_instead_of_blocking() {
        // The game loop must never await a slow client. Once the queue is full,
        // try_send fails immediately and the engine drops the player.
        let (_client, server) = connected_pair().await;
        let (tx, _rx) = mpsc::channel(16);
        let link = spawn_conn(1, server, tx, 1);

        let big = Bytes::from(vec![0u8; 256 * 1024]);
        let mut hit_backpressure = false;
        for _ in 0..10_000 {
            if matches!(link.try_send(big.clone()), Err(LinkError::Backpressure)) {
                hit_backpressure = true;
                break;
            }
        }
        assert!(hit_backpressure, "a never-reading peer must trigger backpressure");
    }

    #[tokio::test]
    async fn link_reports_closed_after_the_connection_dies() {
        let (client, server) = connected_pair().await;
        let (tx, mut rx) = mpsc::channel(16);
        let link = spawn_conn(1, server, tx, 8);
        drop(client);
        let _ = rx.recv().await;

        // Give the writer task a moment to observe the closed socket.
        for _ in 0..100 {
            if link.is_closed() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("link never reported closed");
    }
}
```

- [ ] **Step 3: Запустить, убедиться что падает**

Run: `cargo test -p ghost-net`
Expected: FAIL — `cannot find function spawn_conn`.

- [ ] **Step 4: Реализовать `conn.rs`**

```rust
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use ghost_protocol::ProtoError;
use ghost_protocol::frame::Frame;
use ghost_protocol::w3gs::W3gsCodec;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_util::codec::{FramedRead, FramedWrite};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinkError {
    #[error("write queue is full; peer is not draining")]
    Backpressure,
    #[error("connection is closed")]
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    PeerClosed,
    Protocol(ProtoError),
    Io(String),
    WriterBackpressure,
}

#[derive(Debug)]
pub enum ConnEventKind {
    Frame(Frame),
    Closed(CloseReason),
}

#[derive(Debug)]
pub struct ConnEvent {
    pub conn_id: u64,
    pub kind: ConnEventKind,
}

/// The engine's handle on one player's socket. Sending never blocks and never
/// awaits: the game tick hands off bytes and moves on.
#[derive(Debug, Clone)]
pub struct PlayerLink {
    tx: mpsc::Sender<Bytes>,
}

impl PlayerLink {
    pub fn try_send(&self, bytes: Bytes) -> Result<(), LinkError> {
        match self.tx.try_send(bytes) {
            Ok(()) => Ok(()),
            Err(mpsc::error::TrySendError::Full(_)) => Err(LinkError::Backpressure),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(LinkError::Closed),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}

/// Spawns the reader and writer tasks for one connection.
///
/// `write_capacity` bounds how far a client may fall behind. At the default
/// 100 ms tick, 1024 queued packets is roughly 100 seconds of game time: a peer
/// that far behind is dead, not slow.
pub fn spawn_conn(
    conn_id: u64,
    stream: TcpStream,
    events: mpsc::Sender<ConnEvent>,
    write_capacity: usize,
) -> PlayerLink {
    // Nagle would batch our latency-critical action packets. Never enable it.
    if let Err(e) = stream.set_nodelay(true) {
        tracing::warn!(conn_id, error = %e, "failed to set TCP_NODELAY");
    }

    let (read_half, write_half) = stream.into_split();
    let (out_tx, mut out_rx) = mpsc::channel::<Bytes>(write_capacity);

    // Reader: socket -> engine
    let reader_events = events.clone();
    tokio::spawn(async move {
        let mut framed = FramedRead::new(read_half, W3gsCodec::default());
        let reason = loop {
            match framed.next().await {
                Some(Ok(frame)) => {
                    if reader_events
                        .send(ConnEvent { conn_id, kind: ConnEventKind::Frame(frame) })
                        .await
                        .is_err()
                    {
                        return; // engine is gone; nothing to report to
                    }
                }
                Some(Err(ProtoError::BadValue(_))) => continue, // resync and keep reading
                Some(Err(e)) => break CloseReason::Protocol(e),
                None => break CloseReason::PeerClosed,
            }
        };
        let _ = reader_events
            .send(ConnEvent { conn_id, kind: ConnEventKind::Closed(reason) })
            .await;
    });

    // Writer: engine -> socket
    tokio::spawn(async move {
        let mut framed = FramedWrite::new(write_half, W3gsCodec::default());
        while let Some(bytes) = out_rx.recv().await {
            if let Err(e) = framed.send(bytes).await {
                tracing::debug!(conn_id, error = %e, "write failed, closing connection");
                break;
            }
        }
        let _ = framed.close().await;
    });

    PlayerLink { tx: out_tx }
}
```

Добавить в `crates/ghost-net/Cargo.toml`: `futures-util = { version = "0.3", default-features = false }`.

- [ ] **Step 5: Запустить тесты**

Run: `cargo test -p ghost-net`
Expected: PASS, 5 тестов.

- [ ] **Step 6: Реализовать `listener.rs` и `udp.rs`**

```rust
// listener.rs
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_conn_id() -> u64 {
    NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed)
}

/// Accepts connections and forwards them, tagged with a fresh id, to `out`.
pub fn spawn_listener(
    addr: SocketAddr,
    out: mpsc::Sender<(u64, TcpStream, SocketAddr)>,
) -> JoinHandle<std::io::Result<()>> {
    tokio::spawn(async move {
        let listener = TcpListener::bind(addr).await?;
        tracing::info!(%addr, "listening for players");
        loop {
            let (stream, peer) = match listener.accept().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "accept failed");
                    continue;
                }
            };
            if out.send((next_conn_id(), stream, peer)).await.is_err() {
                return Ok(()); // owner shut down
            }
        }
    })
}
```

```rust
// udp.rs
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};

use bytes::Bytes;
use tokio::net::UdpSocket;

/// Broadcasts W3GS_GAMEINFO to the LAN so the game appears in Local Area Games.
pub struct UdpBroadcaster {
    socket: UdpSocket,
    target: SocketAddrV4,
}

impl UdpBroadcaster {
    pub async fn bind(target_port: u16) -> io::Result<Self> {
        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await?;
        socket.set_broadcast(true)?;
        Ok(Self { socket, target: SocketAddrV4::new(Ipv4Addr::BROADCAST, target_port) })
    }

    pub async fn send(&self, packet: &Bytes) -> io::Result<()> {
        self.socket.send_to(packet, self.target).await.map(|_| ())
    }
}
```

`crates/ghost-net/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod conn;
pub mod listener;
pub mod udp;

pub use conn::{CloseReason, ConnEvent, ConnEventKind, LinkError, PlayerLink, spawn_conn};
pub use listener::{next_conn_id, spawn_listener};
pub use udp::UdpBroadcaster;
```

- [ ] **Step 7: Проверить сборку и линт**

Run: `cargo test -p ghost-net && cargo clippy -p ghost-net -- -D warnings`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/ghost-net Cargo.toml Cargo.lock
git commit -m "feat(net): per-connection reader/writer tasks with non-blocking broadcast"
```

---

## Task 8: Планировщик тика без дрейфа

Сердце перформанса. Легаси считает следующий тик от момента фактической отправки (`m_last_action_sent_ticks = get_ticks()` в `src/game_base.rs:1030`) и опрашивает состояние раз в 15 мс (`src/main.rs:63`). Ошибка каждого тика накапливается, а гранулярность опроса добавляет ±15 мс дрожания к 100-мс бюджету. Здесь дедлайны абсолютные: `next += period` независимо от того, когда тик реально выполнился.

**Files:**
- Create: `crates/ghost-engine/Cargo.toml`
- Create: `crates/ghost-engine/src/lib.rs`, `crates/ghost-engine/src/tick.rs`

**Interfaces:**
- Consumes: ничего.
- Produces:
  - `struct TickScheduler { period: Duration, next: Instant }`
  - `TickScheduler::new(period: Duration) -> Self`
  - `TickScheduler::deadline(&self) -> Instant`
  - `TickScheduler::advance(&mut self, now: Instant) -> u32` — двигает дедлайн на один период и далее, пока он не окажется в будущем; возвращает число **пропущенных** периодов (0 в норме)
  - `TickScheduler::set_period(&mut self, period: Duration)` — применяется со следующего тика
  - `TickScheduler::period(&self) -> Duration`

- [ ] **Step 1: Создать крейт**

`crates/ghost-engine/Cargo.toml`:

```toml
[package]
name = "ghost-engine"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true

[dependencies]
ghost-protocol = { path = "../ghost-protocol" }
ghost-net = { path = "../ghost-net" }
bytes.workspace = true
tokio.workspace = true
tracing.workspace = true
thiserror.workspace = true
rand.workspace = true

[dev-dependencies]
tokio = { workspace = true, features = ["test-util"] }
```

`crates/ghost-engine/src/lib.rs`:

```rust
#![forbid(unsafe_code)]

pub mod tick;

pub use tick::TickScheduler;
```

- [ ] **Step 2: Написать падающие тесты**

`crates/ghost-engine/src/tick.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlines_do_not_drift_when_ticks_run_late() {
        let start = Instant::now();
        let mut t = TickScheduler::new(Duration::from_millis(100));
        let first = t.deadline();
        assert_eq!(first, start + Duration::from_millis(100));

        // The tick body took 30 ms; the next deadline is still on the 200 ms grid,
        // not 230 ms. This is the whole point: error must not accumulate.
        let skipped = t.advance(first + Duration::from_millis(30));
        assert_eq!(skipped, 0);
        assert_eq!(t.deadline(), start + Duration::from_millis(200));
    }

    #[test]
    fn drift_stays_bounded_over_many_late_ticks() {
        let start = Instant::now();
        let mut t = TickScheduler::new(Duration::from_millis(100));
        let mut now = start;
        for _ in 0..1000 {
            now = t.deadline() + Duration::from_millis(5); // always 5 ms late
            t.advance(now);
        }
        // 1000 ticks x 100 ms = 100 s after start, regardless of per-tick lateness.
        assert_eq!(t.deadline(), start + Duration::from_millis(100 * 1001));
    }

    #[test]
    fn reports_skipped_periods_after_a_long_stall() {
        let start = Instant::now();
        let mut t = TickScheduler::new(Duration::from_millis(100));
        // The process stalled for 350 ms: three whole periods were missed.
        let skipped = t.advance(start + Duration::from_millis(450));
        assert_eq!(skipped, 3);
        assert!(t.deadline() > start + Duration::from_millis(450));
    }

    #[test]
    fn changing_latency_takes_effect_from_the_next_tick() {
        let start = Instant::now();
        let mut t = TickScheduler::new(Duration::from_millis(100));
        t.set_period(Duration::from_millis(50));
        assert_eq!(t.period(), Duration::from_millis(50));
        t.advance(start + Duration::from_millis(100));
        assert_eq!(t.deadline(), start + Duration::from_millis(150));
    }
}
```

- [ ] **Step 3: Запустить, убедиться что падает**

Run: `cargo test -p ghost-engine tick`
Expected: FAIL — `cannot find struct TickScheduler`.

- [ ] **Step 4: Реализовать**

Вставить над блоком `mod tests`:

```rust
use std::time::{Duration, Instant};

/// Schedules game ticks on an absolute grid so per-tick lateness never
/// accumulates. Replaces the legacy "sleep 15 ms and compare timestamps" loop.
#[derive(Debug, Clone)]
pub struct TickScheduler {
    period: Duration,
    next: Instant,
}

impl TickScheduler {
    pub fn new(period: Duration) -> Self {
        Self { period, next: Instant::now() + period }
    }

    pub fn deadline(&self) -> Instant {
        self.next
    }

    pub fn period(&self) -> Duration {
        self.period
    }

    /// Applied from the next tick onwards; the pending deadline is untouched.
    pub fn set_period(&mut self, period: Duration) {
        self.period = period;
    }

    /// Moves to the next deadline strictly after `now`. Returns how many whole
    /// periods were skipped, which is non-zero only when the process stalled.
    pub fn advance(&mut self, now: Instant) -> u32 {
        self.next += self.period;
        let mut skipped = 0u32;
        while self.next <= now {
            self.next += self.period;
            skipped += 1;
        }
        skipped
    }
}
```

- [ ] **Step 5: Запустить тесты**

Run: `cargo test -p ghost-engine tick`
Expected: PASS, 4 теста.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-engine Cargo.toml Cargo.lock
git commit -m "feat(engine): add drift-free absolute-deadline tick scheduler"
```

---

## Task 9: Таблицы слотов и игроков

Состояние игры принадлежит одному актору, поэтому здесь нет ни `Arc`, ни `Mutex`, ни `async`. Это обычные структуры данных с обычными методами — их легко тестировать и невозможно заблокировать.

**Files:**
- Create: `crates/ghost-engine/src/slots.rs` (переносит `src/engine/slot.rs`)
- Create: `crates/ghost-engine/src/players.rs`
- Modify: `crates/ghost-engine/src/lib.rs`

**Interfaces:**
- Consumes: `ghost_protocol::w3gs::SlotInfo`, `ghost_net::PlayerLink`.
- Produces:
  - `enum SlotStatus { Open = 0, Closed = 1, Occupied = 2 }`
  - `struct SlotTable { slots: Vec<SlotInfo> }` c методами: `new(num_slots: usize) -> Self`, `as_wire(&self) -> &[SlotInfo]`, `open(&mut self, sid: u8) -> bool`, `close(&mut self, sid: u8) -> bool`, `swap(&mut self, a: u8, b: u8) -> bool`, `first_open(&self) -> Option<u8>`, `sid_of_pid(&self, pid: u8) -> Option<u8>`, `occupy(&mut self, sid: u8, pid: u8, team: u8, colour: u8) -> bool`, `release(&mut self, pid: u8) -> Option<u8>`, `count_open(&self) -> u32`, `count_occupied(&self) -> u32`
  - `struct Player { pub pid: u8, pub name: String, pub link: PlayerLink, pub conn_id: u64, pub external_ip: [u8;4], pub internal_ip: [u8;4], pub sync_counter: u32, pub lagging: bool, pub started_lagging: Option<Instant>, pub loaded: bool, pub download_status: u8, pub ping_history: VecDeque<u32>, pub reconnect_key: u32, pub gproxy: bool, pub left: Option<String> }`
  - `impl Player { fn average_ping(&self) -> Option<u32> }`
  - `struct PlayerTable { players: Vec<Player> }` c методами: `new() -> Self`, `insert(&mut self, p: Player)`, `remove_pid(&mut self, pid: u8) -> Option<Player>`, `by_pid(&self, pid: u8) -> Option<&Player>`, `by_pid_mut(&mut self, pid: u8) -> Option<&mut Player>`, `by_conn(&self, conn_id: u64) -> Option<&Player>`, `by_name_partial(&self, needle: &str) -> Result<&Player, NameMatch>`, `iter(&self)`, `iter_mut(&mut self)`, `len(&self)`, `next_free_pid(&self) -> Option<u8>`, `next_free_colour(&self, slots: &SlotTable) -> u8`
  - `enum NameMatch { None, Ambiguous(usize) }`

- [ ] **Step 1: Написать падающие тесты**

`crates/ghost-engine/src/slots.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_table_is_all_open() {
        let t = SlotTable::new(12);
        assert_eq!(t.as_wire().len(), 12);
        assert_eq!(t.count_open(), 12);
        assert_eq!(t.count_occupied(), 0);
        assert_eq!(t.first_open(), Some(0));
    }

    #[test]
    fn occupy_and_release_track_pids() {
        let mut t = SlotTable::new(4);
        assert!(t.occupy(1, 7, 0, 3));
        assert_eq!(t.sid_of_pid(7), Some(1));
        assert_eq!(t.count_occupied(), 1);
        assert_eq!(t.count_open(), 3);
        assert_eq!(t.first_open(), Some(0));

        assert_eq!(t.release(7), Some(1));
        assert_eq!(t.sid_of_pid(7), None);
        assert_eq!(t.count_open(), 4);
    }

    #[test]
    fn closed_slots_are_neither_open_nor_occupied() {
        let mut t = SlotTable::new(3);
        assert!(t.close(0));
        assert_eq!(t.count_open(), 2);
        assert_eq!(t.count_occupied(), 0);
        assert_eq!(t.first_open(), Some(1));
        assert!(t.open(0));
        assert_eq!(t.first_open(), Some(0));
    }

    #[test]
    fn swap_moves_occupants() {
        let mut t = SlotTable::new(4);
        t.occupy(0, 5, 0, 1);
        assert!(t.swap(0, 3));
        assert_eq!(t.sid_of_pid(5), Some(3));
    }

    #[test]
    fn out_of_range_operations_are_rejected_not_panicking() {
        let mut t = SlotTable::new(2);
        assert!(!t.close(9));
        assert!(!t.open(9));
        assert!(!t.swap(0, 9));
        assert!(!t.occupy(9, 1, 0, 0));
    }
}
```

`crates/ghost-engine/src/players.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn test_player(pid: u8, name: &str) -> Player {
        let (tx, _rx) = mpsc::channel(8);
        Player::new(pid, name.to_string(), 0, ghost_net::PlayerLink::for_test(tx))
    }

    #[test]
    fn pids_are_allocated_from_the_lowest_free_value() {
        let mut t = PlayerTable::new();
        assert_eq!(t.next_free_pid(), Some(1));
        t.insert(test_player(1, "a"));
        t.insert(test_player(3, "b"));
        assert_eq!(t.next_free_pid(), Some(2));
    }

    #[test]
    fn partial_name_lookup_reports_ambiguity() {
        let mut t = PlayerTable::new();
        t.insert(test_player(1, "Slash"));
        t.insert(test_player(2, "Slasher"));
        t.insert(test_player(3, "Other"));

        assert_eq!(t.by_name_partial("Oth").unwrap().pid, 3);
        assert!(matches!(t.by_name_partial("Sla"), Err(NameMatch::Ambiguous(2))));
        assert!(matches!(t.by_name_partial("zzz"), Err(NameMatch::None)));
        // An exact match wins even when it is a prefix of another name.
        assert_eq!(t.by_name_partial("Slash").unwrap().pid, 1);
    }

    #[test]
    fn average_ping_ignores_an_empty_history() {
        let mut p = test_player(1, "a");
        assert_eq!(p.average_ping(), None);
        p.ping_history.push_back(40);
        p.ping_history.push_back(60);
        assert_eq!(p.average_ping(), Some(50));
    }

    #[test]
    fn removing_a_player_frees_the_pid() {
        let mut t = PlayerTable::new();
        t.insert(test_player(1, "a"));
        assert_eq!(t.next_free_pid(), Some(2));
        assert!(t.remove_pid(1).is_some());
        assert_eq!(t.next_free_pid(), Some(1));
        assert!(t.remove_pid(1).is_none());
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-engine`
Expected: FAIL — `cannot find struct SlotTable`.

- [ ] **Step 3: Добавить тестовый конструктор `PlayerLink`**

В `crates/ghost-net/src/conn.rs` добавить в `impl PlayerLink`:

```rust
    /// Builds a link over a caller-supplied channel. For tests and for the
    /// virtual host player, which has no socket behind it.
    pub fn for_test(tx: mpsc::Sender<Bytes>) -> Self {
        Self { tx }
    }
```

- [ ] **Step 4: Реализовать `slots.rs`**

```rust
use ghost_protocol::w3gs::SlotInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SlotStatus {
    Open = 0,
    Closed = 1,
    Occupied = 2,
}

/// The authoritative slot table. Indices are slot ids (SIDs); the wire format
/// is produced verbatim from this vector.
#[derive(Debug, Clone)]
pub struct SlotTable {
    slots: Vec<SlotInfo>,
}

impl SlotTable {
    pub fn new(num_slots: usize) -> Self {
        let slots = (0..num_slots)
            .map(|i| SlotInfo {
                pid: 0,
                download_status: 255,
                slot_status: SlotStatus::Open as u8,
                computer: 0,
                team: (i / 6) as u8,
                colour: i as u8,
                race: 0x20, // random
                computer_type: 1,
                handicap: 100,
            })
            .collect();
        Self { slots }
    }

    pub fn as_wire(&self) -> &[SlotInfo] {
        &self.slots
    }

    fn get_mut(&mut self, sid: u8) -> Option<&mut SlotInfo> {
        self.slots.get_mut(sid as usize)
    }

    pub fn open(&mut self, sid: u8) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                s.slot_status = SlotStatus::Open as u8;
                s.pid = 0;
                true
            }
            None => false,
        }
    }

    pub fn close(&mut self, sid: u8) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                s.slot_status = SlotStatus::Closed as u8;
                s.pid = 0;
                true
            }
            None => false,
        }
    }

    pub fn swap(&mut self, a: u8, b: u8) -> bool {
        let (a, b) = (a as usize, b as usize);
        if a >= self.slots.len() || b >= self.slots.len() {
            return false;
        }
        self.slots.swap(a, b);
        true
    }

    pub fn occupy(&mut self, sid: u8, pid: u8, team: u8, colour: u8) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                s.slot_status = SlotStatus::Occupied as u8;
                s.pid = pid;
                s.team = team;
                s.colour = colour;
                s.computer = 0;
                true
            }
            None => false,
        }
    }

    /// Frees whichever slot holds `pid`, returning its SID.
    pub fn release(&mut self, pid: u8) -> Option<u8> {
        let sid = self.sid_of_pid(pid)?;
        self.open(sid);
        Some(sid)
    }

    pub fn sid_of_pid(&self, pid: u8) -> Option<u8> {
        self.slots.iter().position(|s| {
            s.slot_status == SlotStatus::Occupied as u8 && s.pid == pid
        }).map(|i| i as u8)
    }

    pub fn first_open(&self) -> Option<u8> {
        self.slots.iter().position(|s| s.slot_status == SlotStatus::Open as u8).map(|i| i as u8)
    }

    pub fn count_open(&self) -> u32 {
        self.slots.iter().filter(|s| s.slot_status == SlotStatus::Open as u8).count() as u32
    }

    pub fn count_occupied(&self) -> u32 {
        self.slots.iter().filter(|s| s.slot_status == SlotStatus::Occupied as u8).count() as u32
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}
```

- [ ] **Step 5: Реализовать `players.rs`**

```rust
use std::collections::VecDeque;
use std::time::Instant;

use ghost_net::PlayerLink;

use crate::slots::SlotTable;

/// How many recent ping samples feed the average shown by `!ping`.
const PING_HISTORY: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameMatch {
    None,
    Ambiguous(usize),
}

#[derive(Debug)]
pub struct Player {
    pub pid: u8,
    pub name: String,
    pub conn_id: u64,
    pub link: PlayerLink,
    pub external_ip: [u8; 4],
    pub internal_ip: [u8; 4],
    /// How many action ticks this player has confirmed via keepalive.
    pub sync_counter: u32,
    pub lagging: bool,
    pub started_lagging: Option<Instant>,
    pub loaded: bool,
    /// 0..100 while downloading the map, 255 when not downloading.
    pub download_status: u8,
    pub ping_history: VecDeque<u32>,
    pub reconnect_key: u32,
    pub gproxy: bool,
    /// Set once the player is scheduled for removal; carries the reason.
    pub left: Option<String>,
}

impl Player {
    pub fn new(pid: u8, name: String, conn_id: u64, link: PlayerLink) -> Self {
        Self {
            pid,
            name,
            conn_id,
            link,
            external_ip: [0; 4],
            internal_ip: [0; 4],
            sync_counter: 0,
            lagging: false,
            started_lagging: None,
            loaded: false,
            download_status: 255,
            ping_history: VecDeque::with_capacity(PING_HISTORY),
            reconnect_key: 0,
            gproxy: false,
            left: None,
        }
    }

    pub fn record_ping(&mut self, ping_ms: u32) {
        if self.ping_history.len() == PING_HISTORY {
            self.ping_history.pop_front();
        }
        self.ping_history.push_back(ping_ms);
    }

    pub fn average_ping(&self) -> Option<u32> {
        if self.ping_history.is_empty() {
            return None;
        }
        let sum: u32 = self.ping_history.iter().sum();
        Some(sum / self.ping_history.len() as u32)
    }
}

#[derive(Debug, Default)]
pub struct PlayerTable {
    players: Vec<Player>,
}

impl PlayerTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, p: Player) {
        self.players.push(p);
    }

    pub fn remove_pid(&mut self, pid: u8) -> Option<Player> {
        let i = self.players.iter().position(|p| p.pid == pid)?;
        Some(self.players.remove(i))
    }

    pub fn by_pid(&self, pid: u8) -> Option<&Player> {
        self.players.iter().find(|p| p.pid == pid)
    }

    pub fn by_pid_mut(&mut self, pid: u8) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.pid == pid)
    }

    pub fn by_conn(&self, conn_id: u64) -> Option<&Player> {
        self.players.iter().find(|p| p.conn_id == conn_id)
    }

    pub fn by_conn_mut(&mut self, conn_id: u64) -> Option<&mut Player> {
        self.players.iter_mut().find(|p| p.conn_id == conn_id)
    }

    /// Exact match wins; otherwise a unique case-insensitive prefix match.
    pub fn by_name_partial(&self, needle: &str) -> Result<&Player, NameMatch> {
        if let Some(p) = self.players.iter().find(|p| p.name == needle) {
            return Ok(p);
        }
        let lower = needle.to_lowercase();
        let hits: Vec<&Player> = self
            .players
            .iter()
            .filter(|p| p.name.to_lowercase().starts_with(&lower))
            .collect();
        match hits.len() {
            0 => Err(NameMatch::None),
            1 => Ok(hits[0]),
            n => Err(NameMatch::Ambiguous(n)),
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Player> {
        self.players.iter()
    }

    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, Player> {
        self.players.iter_mut()
    }

    pub fn len(&self) -> usize {
        self.players.len()
    }

    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// PIDs run 1..=254; 255 is reserved for the virtual host player.
    pub fn next_free_pid(&self) -> Option<u8> {
        (1u8..=254).find(|c| !self.players.iter().any(|p| p.pid == *c))
    }

    pub fn next_free_colour(&self, slots: &SlotTable) -> u8 {
        let taken: Vec<u8> = slots.as_wire().iter().map(|s| s.colour).collect();
        (0u8..=11).find(|c| !taken.contains(c)).unwrap_or(0)
    }
}
```

Добавить в `crates/ghost-engine/src/lib.rs`:

```rust
pub mod players;
pub mod slots;

pub use players::{NameMatch, Player, PlayerTable};
pub use slots::{SlotStatus, SlotTable};
```

- [ ] **Step 6: Запустить тесты**

Run: `cargo test -p ghost-engine`
Expected: PASS, 13 тестов (4 из Task 8 + 5 слотов + 4 игроков).

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-engine crates/ghost-net
git commit -m "feat(engine): add slot and player tables owned by a single actor"
```

---

## Task 10: Актор игры — цикл `select!` и лобби

Замена `src/ghost.rs:210-309` + `src/game_base.rs:362`. Вместо глобального `CURRENT_GAME: RwLock<Option<Arc<Mutex<BaseGame>>>>`, который лочится на каждой итерации, — таска, единолично владеющая `GameState`. Внешний мир общается только через `mpsc`. Отсюда же исчезает `timeout(Duration::from_millis(1), game.update())` из `src/ghost.rs:285`, который отменял апдейт на середине.

**Files:**
- Create: `crates/ghost-engine/src/state.rs`, `src/handle.rs`, `src/actor.rs`, `src/lobby.rs`
- Modify: `crates/ghost-engine/src/lib.rs`

**Interfaces:**
- Consumes: `TickScheduler`, `SlotTable`, `PlayerTable`, `Player` (Tasks 8–9); `ghost_net::{ConnEvent, ConnEventKind, CloseReason, PlayerLink, LinkError}`; `ghost_protocol::w3gs::{Frame, ids, incoming, outgoing}`.
- Produces:
  - `struct GameConfig { pub name: String, pub owner: String, pub host_counter: u32, pub num_slots: usize, pub latency: Duration, pub sync_limit: u32, pub map: MapInfo, pub virtual_host_name: String }`
  - `struct MapInfo { pub path: String, pub size: u32, pub info: u32, pub crc: u32, pub sha1: [u8;20], pub num_players: u8, pub num_teams: u8, pub width: u16, pub height: u16, pub game_type: u32, pub flags: u32, pub data: Option<Arc<Vec<u8>>> }`
  - `enum GamePhase { Lobby, Countdown { remaining: u8 }, Loading, Playing, Over }`
  - `struct GameState { ... }` + `GameState::new(cfg: GameConfig) -> Self`
  - `enum GameCmd { NewConn { conn_id: u64, link: PlayerLink, external_ip: [u8;4] }, Conn(ConnEvent), Start { by: String }, Chat(String), Unhost, Shutdown }`
  - `struct GameHandle { tx: mpsc::Sender<GameCmd> }` + `GameHandle::send(&self, cmd: GameCmd)`, `GameHandle::is_closed(&self) -> bool`
  - `fn spawn_game(cfg: GameConfig) -> (GameHandle, JoinHandle<()>)`
  - методы `GameState`: `broadcast(&mut self, bytes: Bytes)`, `send_to(&mut self, pid: u8, bytes: Bytes)`, `on_tick(&mut self, skipped: u32)`, `on_frame(&mut self, conn_id: u64, frame: Frame)`, `reap_left_players(&mut self)`

- [ ] **Step 1: Написать падающие тесты**

`crates/ghost-engine/src/actor.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use ghost_protocol::w3gs::ids;
    use tokio::sync::mpsc;

    fn test_cfg() -> GameConfig {
        GameConfig {
            name: "test".into(),
            owner: "slash".into(),
            host_counter: 1,
            num_slots: 12,
            latency: Duration::from_millis(100),
            sync_limit: 50,
            map: MapInfo::test_default(),
            virtual_host_name: "|cFF4080C0Ghost".into(),
        }
    }

    /// Drains one player's outbound queue into a list of (id, payload) pairs.
    fn drain(rx: &mut mpsc::Receiver<Bytes>) -> Vec<u8> {
        let mut ids = Vec::new();
        while let Ok(b) = rx.try_recv() {
            ids.push(b[1]);
        }
        ids
    }

    #[tokio::test]
    async fn a_joining_player_gets_slotinfojoin_and_is_seated() {
        let mut st = GameState::new(test_cfg());
        let (tx, mut rx) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx), [1, 2, 3, 4]);

        st.on_frame(1, Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash")));

        assert_eq!(st.players.len(), 1);
        let p = st.players.by_conn(1).expect("seated");
        assert_eq!(p.name, "Slash");
        assert_eq!(st.slots.count_occupied(), 1);
        assert!(drain(&mut rx).contains(&ids::SLOT_INFO_JOIN));
    }

    #[tokio::test]
    async fn a_second_player_with_the_same_name_is_rejected() {
        let mut st = GameState::new(test_cfg());
        let (tx1, _rx1) = mpsc::channel(64);
        let (tx2, mut rx2) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx1), [1, 1, 1, 1]);
        st.add_conn(2, PlayerLink::for_test(tx2), [2, 2, 2, 2]);

        st.on_frame(1, Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash")));
        st.on_frame(2, Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash")));

        assert_eq!(st.players.len(), 1);
        assert!(drain(&mut rx2).contains(&ids::REJECT_JOIN));
    }

    #[tokio::test]
    async fn joining_a_full_lobby_is_rejected() {
        let mut cfg = test_cfg();
        cfg.num_slots = 1;
        let mut st = GameState::new(cfg);
        let (tx1, _rx1) = mpsc::channel(64);
        let (tx2, mut rx2) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx1), [1, 1, 1, 1]);
        st.add_conn(2, PlayerLink::for_test(tx2), [2, 2, 2, 2]);

        st.on_frame(1, Frame::new(ids::REQ_JOIN, reqjoin_bytes("A")));
        st.on_frame(2, Frame::new(ids::REQ_JOIN, reqjoin_bytes("B")));

        assert_eq!(st.players.len(), 1);
        assert!(drain(&mut rx2).contains(&ids::REJECT_JOIN));
    }

    #[tokio::test]
    async fn leaving_frees_the_slot_and_notifies_everyone_else() {
        let mut st = GameState::new(test_cfg());
        let (tx1, _rx1) = mpsc::channel(64);
        let (tx2, mut rx2) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx1), [1, 1, 1, 1]);
        st.add_conn(2, PlayerLink::for_test(tx2), [2, 2, 2, 2]);
        st.on_frame(1, Frame::new(ids::REQ_JOIN, reqjoin_bytes("A")));
        st.on_frame(2, Frame::new(ids::REQ_JOIN, reqjoin_bytes("B")));
        let _ = drain(&mut rx2);

        st.on_frame(1, Frame::new(ids::LEAVE_GAME, Bytes::from_static(&[7, 0, 0, 0])));
        st.reap_left_players();

        assert_eq!(st.players.len(), 1);
        assert_eq!(st.slots.count_occupied(), 1);
        assert!(drain(&mut rx2).contains(&ids::PLAYER_LEAVE_OTHERS));
    }

    #[tokio::test]
    async fn a_dead_link_removes_the_player_instead_of_stalling_the_tick() {
        let mut st = GameState::new(test_cfg());
        let (tx, rx) = mpsc::channel(64);
        st.add_conn(1, PlayerLink::for_test(tx), [1, 1, 1, 1]);
        st.on_frame(1, Frame::new(ids::REQ_JOIN, reqjoin_bytes("A")));
        drop(rx); // the writer task is gone

        st.broadcast(Bytes::from_static(&[0xF7, 0x0B, 0x04, 0x00]));
        st.reap_left_players();

        assert_eq!(st.players.len(), 0);
    }

    #[tokio::test]
    async fn the_actor_shuts_down_on_command() {
        let (handle, join) = spawn_game(test_cfg());
        handle.send(GameCmd::Shutdown);
        tokio::time::timeout(Duration::from_secs(2), join)
            .await
            .expect("actor must exit promptly")
            .expect("actor must not panic");
    }
}
```

Хелпер `reqjoin_bytes` вынести в `crates/ghost-engine/src/actor.rs` под `#[cfg(test)]`, собирая payload по раскладке из Task 4 (host_counter 1, entry_key 0, unknown 0, listen_port 6112, peer_key 0, имя, 4 нулевых байта, internal ip `[127,0,0,1]`).

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-engine actor`
Expected: FAIL — `cannot find struct GameState`.

- [ ] **Step 3: Реализовать `state.rs`**

```rust
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use ghost_net::{LinkError, PlayerLink};
use ghost_protocol::w3gs::{ActionBlock, outgoing};

use crate::players::PlayerTable;
use crate::slots::SlotTable;
use crate::tick::TickScheduler;

#[derive(Debug, Clone)]
pub struct MapInfo {
    pub path: String,
    pub size: u32,
    pub info: u32,
    pub crc: u32,
    pub sha1: [u8; 20],
    pub num_players: u8,
    pub num_teams: u8,
    pub width: u16,
    pub height: u16,
    pub game_type: u32,
    pub flags: u32,
    /// Present only when map downloads are enabled.
    pub data: Option<Arc<Vec<u8>>>,
}

#[derive(Debug, Clone)]
pub struct GameConfig {
    pub name: String,
    pub owner: String,
    pub host_counter: u32,
    pub num_slots: usize,
    pub latency: Duration,
    pub sync_limit: u32,
    pub map: MapInfo,
    pub virtual_host_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GamePhase {
    Lobby,
    Countdown { remaining: u8 },
    Loading,
    Playing,
    Over,
}

/// All mutable game state, owned exclusively by one actor task. No locks.
pub struct GameState {
    pub cfg: GameConfig,
    pub phase: GamePhase,
    pub slots: SlotTable,
    pub players: PlayerTable,
    pub tick: TickScheduler,
    /// Connections that have not sent REQ_JOIN yet.
    pub pending: Vec<(u64, PlayerLink, [u8; 4])>,
    pub actions: Vec<ActionBlock>,
    pub sync_counter: u32,
    pub game_ticks: u32,
    pub random_seed: u32,
    pub last_tick_at: Option<Instant>,
    pub created_at: Instant,
    pub lagging: bool,
    pub finished: bool,
}

impl GameState {
    pub fn new(cfg: GameConfig) -> Self {
        let slots = SlotTable::new(cfg.num_slots);
        let tick = TickScheduler::new(cfg.latency);
        Self {
            phase: GamePhase::Lobby,
            slots,
            players: PlayerTable::new(),
            tick,
            pending: Vec::new(),
            actions: Vec::new(),
            sync_counter: 0,
            game_ticks: 0,
            random_seed: rand::random(),
            last_tick_at: None,
            created_at: Instant::now(),
            lagging: false,
            finished: false,
            cfg,
        }
    }

    pub fn add_conn(&mut self, conn_id: u64, link: PlayerLink, external_ip: [u8; 4]) {
        self.pending.push((conn_id, link, external_ip));
    }

    /// Queues `bytes` for every seated player. Never awaits: a peer that cannot
    /// keep up is marked for removal rather than allowed to stall the tick.
    pub fn broadcast(&mut self, bytes: Bytes) {
        for p in self.players.iter_mut() {
            if p.left.is_some() {
                continue;
            }
            match p.link.try_send(bytes.clone()) {
                Ok(()) => {}
                Err(LinkError::Backpressure) => {
                    tracing::warn!(pid = p.pid, name = %p.name, "write queue full, dropping player");
                    p.left = Some("lagged out (write queue full)".into());
                }
                Err(LinkError::Closed) => {
                    p.left = Some("connection closed".into());
                }
            }
        }
    }

    pub fn send_to(&mut self, pid: u8, bytes: Bytes) {
        if let Some(p) = self.players.by_pid_mut(pid) {
            if p.link.try_send(bytes).is_err() {
                p.left = Some("connection closed".into());
            }
        }
    }

    pub fn send_chat_all(&mut self, message: &str) {
        let pids: Vec<u8> = self.players.iter().map(|p| p.pid).collect();
        if pids.is_empty() {
            return;
        }
        let flag = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
            0x10
        } else {
            0x20
        };
        let extra: &[u8] = if flag == 0x20 { &[0, 0, 0, 0] } else { &[] };
        match outgoing::chat_from_host(255, &pids, flag, extra, message) {
            Ok(b) => self.broadcast(b),
            Err(e) => tracing::warn!(error = %e, "failed to build chat packet"),
        }
    }

    /// Removes everyone marked as left and tells the rest. Called once per tick
    /// and after every batch of inbound frames, never mid-iteration.
    pub fn reap_left_players(&mut self) {
        let gone: Vec<(u8, String)> = self
            .players
            .iter()
            .filter_map(|p| p.left.as_ref().map(|r| (p.pid, r.clone())))
            .collect();

        for (pid, reason) in gone {
            self.players.remove_pid(pid);
            self.slots.release(pid);
            tracing::info!(game = %self.cfg.name, pid, %reason, "player left");
            self.broadcast(outgoing::player_leave_others(pid, 13));
            if matches!(self.phase, GamePhase::Lobby) {
                self.send_all_slot_info();
            }
        }
    }

    pub fn send_all_slot_info(&mut self) {
        match outgoing::slot_info(
            self.slots.as_wire(),
            self.random_seed,
            self.cfg.map.num_teams,
            self.cfg.map.num_players,
        ) {
            Ok(b) => self.broadcast(b),
            Err(e) => tracing::warn!(error = %e, "failed to build slot info"),
        }
    }
}
```

- [ ] **Step 4: Реализовать `lobby.rs` (обработка REQ_JOIN и выходов)**

```rust
use bytes::Bytes;
use ghost_protocol::w3gs::{incoming::ReqJoin, ids, outgoing};

use crate::state::{GamePhase, GameState};
use crate::players::Player;

/// REJECTJOIN reason codes, from src/gameprotocol.rs:249.
pub const REJECT_FULL: u32 = 0x09;
pub const REJECT_STARTED: u32 = 0x0A;
pub const REJECT_WRONG_PASSWORD: u32 = 0x1B;

impl GameState {
    pub(crate) fn handle_req_join(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(idx) = self.pending.iter().position(|(id, _, _)| *id == conn_id) else {
            tracing::debug!(conn_id, "REQ_JOIN from an already-seated connection, ignoring");
            return;
        };
        let (_, link, external_ip) = self.pending.remove(idx);

        let req = match ReqJoin::decode(payload) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(conn_id, error = %e, "malformed REQ_JOIN");
                return;
            }
        };

        // Reject before allocating anything.
        if !matches!(self.phase, GamePhase::Lobby) {
            let _ = link.try_send(outgoing::reject_join(REJECT_STARTED));
            return;
        }
        if self.players.iter().any(|p| p.name.eq_ignore_ascii_case(&req.name)) {
            let _ = link.try_send(outgoing::reject_join(REJECT_FULL));
            return;
        }
        let (Some(sid), Some(pid)) = (self.slots.first_open(), self.players.next_free_pid()) else {
            let _ = link.try_send(outgoing::reject_join(REJECT_FULL));
            return;
        };

        let colour = self.players.next_free_colour(&self.slots);
        let team = sid / 6;
        self.slots.occupy(sid, pid, team, colour);

        let mut player = Player::new(pid, req.name.clone(), conn_id, link);
        player.external_ip = external_ip;
        player.internal_ip = req.internal_ip;
        player.reconnect_key = rand::random();

        // 1. Tell the joiner who they are and what the lobby looks like.
        match outgoing::slot_info_join(
            pid,
            req.listen_port,
            external_ip,
            self.slots.as_wire(),
            self.random_seed,
            self.cfg.map.num_teams,
            self.cfg.map.num_players,
        ) {
            Ok(b) => {
                if player.link.try_send(b).is_err() {
                    self.slots.release(pid);
                    return;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to build slotinfojoin");
                self.slots.release(pid);
                return;
            }
        }

        // 2. Tell the joiner about everyone already here.
        let existing: Vec<(u8, String, [u8; 4], [u8; 4])> = self
            .players
            .iter()
            .map(|p| (p.pid, p.name.clone(), p.external_ip, p.internal_ip))
            .collect();
        for (other_pid, name, ext, int) in existing {
            if let Ok(b) = outgoing::player_info(other_pid, &name, ext, int) {
                let _ = player.link.try_send(b);
            }
        }

        // 3. Map check, so the client can start downloading or confirm it has the map.
        if let Ok(b) = outgoing::map_check(
            &self.cfg.map.path,
            self.cfg.map.size,
            self.cfg.map.info,
            self.cfg.map.crc,
            self.cfg.map.sha1,
        ) {
            let _ = player.link.try_send(b);
        }

        self.players.insert(player);

        // 4. Tell everyone else about the joiner.
        if let Ok(b) = outgoing::player_info(pid, &req.name, external_ip, req.internal_ip) {
            for p in self.players.iter_mut() {
                if p.pid != pid {
                    let _ = p.link.try_send(b.clone());
                }
            }
        }
        self.send_all_slot_info();

        tracing::info!(game = %self.cfg.name, %pid, name = %req.name, "player joined");
    }

    pub(crate) fn handle_leave(&mut self, conn_id: u64, reason_code: u32) {
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.left = Some(format!("left the game voluntarily (code {reason_code})"));
        } else {
            self.pending.retain(|(id, _, _)| *id != conn_id);
        }
    }

    pub(crate) fn handle_conn_closed(&mut self, conn_id: u64, reason: String) {
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            if p.left.is_none() {
                p.left = Some(reason);
            }
        } else {
            self.pending.retain(|(id, _, _)| *id != conn_id);
        }
    }
}
```

- [ ] **Step 5: Реализовать `handle.rs` и `actor.rs`**

```rust
// handle.rs
use ghost_net::{ConnEvent, PlayerLink};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum GameCmd {
    NewConn { conn_id: u64, link: PlayerLink, external_ip: [u8; 4] },
    Conn(ConnEvent),
    Start { by: String },
    Chat(String),
    Unhost,
    Shutdown,
}

/// Cheap, cloneable handle to a game actor.
#[derive(Debug, Clone)]
pub struct GameHandle {
    tx: mpsc::Sender<GameCmd>,
}

impl GameHandle {
    pub fn new(tx: mpsc::Sender<GameCmd>) -> Self {
        Self { tx }
    }

    /// Fire-and-forget. A full queue means the actor is wedged; log and drop
    /// rather than block whoever is calling us.
    pub fn send(&self, cmd: GameCmd) {
        if let Err(e) = self.tx.try_send(cmd) {
            tracing::warn!(error = %e, "game command dropped");
        }
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}
```

```rust
// actor.rs
use std::time::{Duration, Instant};

use bytes::Bytes;
use ghost_net::{ConnEventKind, PlayerLink};
use ghost_protocol::frame::Frame;
use ghost_protocol::w3gs::{ids, incoming};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::handle::{GameCmd, GameHandle};
use crate::state::{GameConfig, GamePhase, GameState};

/// Bounds how far the command queue may back up before the sender complains.
const CMD_CAPACITY: usize = 4096;

pub fn spawn_game(cfg: GameConfig) -> (GameHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(CMD_CAPACITY);
    let name = cfg.name.clone();
    let join = tokio::spawn(async move {
        let state = GameState::new(cfg);
        run(state, rx).await;
        tracing::info!(game = %name, "game actor exited");
    });
    (GameHandle::new(tx), join)
}

async fn run(mut state: GameState, mut rx: mpsc::Receiver<GameCmd>) {
    let sleep = tokio::time::sleep_until(state.tick.deadline().into());
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            // Commands first: actions that arrive just before a deadline should
            // make it into that tick rather than the next one.
            biased;

            cmd = rx.recv() => {
                match cmd {
                    Some(GameCmd::Shutdown) | None => break,
                    Some(c) => state.handle_cmd(c),
                }
            }

            () = &mut sleep => {
                let skipped = state.tick.advance(Instant::now());
                if skipped > 0 {
                    tracing::warn!(game = %state.cfg.name, skipped, "tick deadline missed");
                }
                state.on_tick(skipped);
                sleep.as_mut().reset(state.tick.deadline().into());
            }
        }

        state.reap_left_players();
        if state.finished {
            break;
        }
    }
}

impl GameState {
    pub fn handle_cmd(&mut self, cmd: GameCmd) {
        match cmd {
            GameCmd::NewConn { conn_id, link, external_ip } => {
                self.add_conn(conn_id, link, external_ip)
            }
            GameCmd::Conn(ev) => match ev.kind {
                ConnEventKind::Frame(f) => self.on_frame(ev.conn_id, f),
                ConnEventKind::Closed(reason) => {
                    self.handle_conn_closed(ev.conn_id, format!("{reason:?}"))
                }
            },
            GameCmd::Start { by } => self.start_countdown(&by),
            GameCmd::Chat(msg) => self.send_chat_all(&msg),
            GameCmd::Unhost => {
                if matches!(self.phase, GamePhase::Lobby) {
                    self.finished = true;
                }
            }
            GameCmd::Shutdown => self.finished = true,
        }
    }

    pub fn on_frame(&mut self, conn_id: u64, frame: Frame) {
        match frame.id {
            ids::REQ_JOIN => self.handle_req_join(conn_id, &frame.payload),
            ids::LEAVE_GAME => {
                let code = incoming::decode_leave_game(&frame.payload).unwrap_or(0);
                self.handle_leave(conn_id, code);
            }
            ids::OUTGOING_ACTION => self.handle_action(conn_id, &frame.payload),
            ids::OUTGOING_KEEPALIVE => self.handle_keepalive(conn_id, &frame.payload),
            ids::CHAT_TO_HOST => self.handle_chat_to_host(conn_id, &frame.payload),
            ids::GAME_LOADED_SELF => self.handle_loaded(conn_id),
            ids::PONG_TO_HOST => self.handle_pong(conn_id, &frame.payload),
            ids::MAP_SIZE => self.handle_map_size(conn_id, &frame.payload),
            ids::MAP_PART_OK => self.handle_map_part_ok(conn_id, &frame.payload),
            ids::DROP_REQ => self.handle_drop_request(conn_id),
            other => tracing::trace!(conn_id, id = format!("0x{other:02X}"), "ignoring packet"),
        }
    }

    pub fn start_countdown(&mut self, by: &str) {
        if matches!(self.phase, GamePhase::Lobby) {
            tracing::info!(game = %self.cfg.name, %by, "countdown started");
            self.phase = GamePhase::Countdown { remaining: 5 };
        }
    }
}
```

Заглушки, которые заполняются в задачах 11–13, добавить сейчас, чтобы крейт собирался — каждая логирует `trace!` и ничего не делает: `handle_action`, `handle_keepalive`, `handle_chat_to_host`, `handle_loaded`, `handle_pong`, `handle_map_size`, `handle_map_part_ok`, `handle_drop_request`, `on_tick`.

- [ ] **Step 6: Запустить тесты**

Run: `cargo test -p ghost-engine actor`
Expected: PASS, 6 тестов.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): add game actor with lock-free state and lobby join flow"
```

---

## Task 11: Тик игры — батчинг экшенов, счётчик синхронизации, старт

Здесь заменяются `src/game_base.rs:966-1032` (`send_all_actions`) и `src/engine/sync.rs`. Три отличия от легаси: пакет кодируется **один раз** и рассылается как общий `Bytes` (легаси клонировал `Vec<u8>` каждому игроку), CRC экшенов считается (в `src/engine/sync.rs` её нет), а `send_interval` берётся из планировщика, а не из фактического интервала между отправками.

**Files:**
- Create: `crates/ghost-engine/src/actions.rs`
- Modify: `crates/ghost-engine/src/state.rs`, `src/actor.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `ActionBlock`, `outgoing::{incoming_action, incoming_action2, countdown_start, countdown_end, game_loaded_others}` (Task 5), `GameState` (Task 10).
- Produces: методы `GameState`: `handle_action(&mut self, conn_id: u64, payload: &Bytes)`, `handle_keepalive(&mut self, conn_id: u64, payload: &Bytes)`, `handle_loaded(&mut self, conn_id: u64)`, `on_tick(&mut self, skipped: u32)`, `send_all_actions(&mut self)`; константа `MAX_ACTION_PAYLOAD: usize = 1400`.

- [ ] **Step 1: Написать падающие тесты**

`crates/ghost-engine/src/actions.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use ghost_protocol::w3gs::ids;

    #[test]
    fn a_playing_tick_broadcasts_one_action_packet_per_player() {
        let (mut st, mut rxs) = seated_game(3);
        st.begin_playing();
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        st.on_tick(0);

        for rx in rxs.iter_mut() {
            assert_eq!(drain_ids(rx), vec![ids::INCOMING_ACTION]);
        }
        assert_eq!(st.sync_counter, 1);
        assert_eq!(st.game_ticks, 100);
    }

    #[test]
    fn queued_actions_are_flushed_and_the_queue_is_emptied() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }
        st.actions.push(ActionBlock { pid: 1, data: Bytes::from_static(&[0x10, 0x20]) });

        st.on_tick(0);

        assert!(st.actions.is_empty(), "actions must not be replayed next tick");
        let first = rxs[0].try_recv().expect("action packet");
        assert_eq!(first[1], ids::INCOMING_ACTION);
        assert!(first.len() > 8, "packet must carry the action body and CRC");
    }

    #[test]
    fn oversized_action_batches_spill_into_incoming_action2() {
        let (mut st, mut rxs) = seated_game(1);
        st.begin_playing();
        let _ = drain_ids(&mut rxs[0]);

        // 20 x 100-byte actions = 2060 wire bytes, past the 1400-byte limit.
        for _ in 0..20 {
            st.actions.push(ActionBlock { pid: 1, data: Bytes::from(vec![7u8; 100]) });
        }
        st.on_tick(0);

        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::INCOMING_ACTION2), "overflow packet must be sent");
        assert_eq!(sent.last(), Some(&ids::INCOMING_ACTION), "main packet goes last");
    }

    #[test]
    fn keepalive_advances_the_players_sync_counter() {
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();
        st.on_tick(0);
        assert_eq!(st.players.by_pid(1).unwrap().sync_counter, 0);

        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_u8(&mut p, 0);
        bytes::BufMut::put_u32_le(&mut p, 0xDEAD);
        st.handle_keepalive(1, &p.freeze());

        assert_eq!(st.players.by_pid(1).unwrap().sync_counter, 1);
    }

    #[test]
    fn lobby_ticks_do_not_produce_action_packets() {
        let (mut st, mut rxs) = seated_game(2);
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }
        st.on_tick(0);
        for rx in rxs.iter_mut() {
            assert!(!drain_ids(rx).contains(&ids::INCOMING_ACTION));
        }
    }

    #[test]
    fn countdown_reaching_zero_starts_loading() {
        let (mut st, mut rxs) = seated_game(1);
        st.start_countdown("slash");
        for _ in 0..6 {
            st.on_tick(0);
        }
        assert_eq!(st.phase, GamePhase::Loading);
        let sent = drain_ids(&mut rxs[0]);
        assert!(sent.contains(&ids::COUNTDOWN_START));
        assert!(sent.contains(&ids::COUNTDOWN_END));
    }

    #[test]
    fn all_players_loaded_moves_the_game_to_playing() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_loading();
        st.handle_loaded(1);
        assert_eq!(st.phase, GamePhase::Loading);
        st.handle_loaded(2);
        assert_eq!(st.phase, GamePhase::Playing);
    }
}
```

Хелперы `seated_game(n)` (создаёт `GameState` с `n` усаженными игроками и возвращает их приёмные концы) и `drain_ids(rx)` вынести в `crates/ghost-engine/src/actor.rs` в `#[cfg(test)] pub mod tests_support`. `conn_id` для игрока `i` — `i`, pid — тоже `i`.

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-engine actions`
Expected: FAIL — `no method named begin_playing`.

- [ ] **Step 3: Реализовать `actions.rs`**

```rust
use bytes::Bytes;
use ghost_protocol::w3gs::{ActionBlock, incoming::OutgoingAction, outgoing};

use crate::state::{GamePhase, GameState};

/// Actions beyond this many wire bytes spill into an INCOMING_ACTION2 packet.
/// Matches src/game_base.rs:988.
pub const MAX_ACTION_PAYLOAD: usize = 1400;

impl GameState {
    pub fn handle_action(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        match OutgoingAction::decode(payload) {
            // The body is a slice of the read buffer: queuing it costs a
            // refcount bump, and it is relayed without ever being parsed.
            Ok(a) => self.actions.push(ActionBlock { pid, data: a.data }),
            Err(e) => tracing::debug!(conn_id, error = %e, "malformed action"),
        }
    }

    pub fn handle_keepalive(&mut self, conn_id: u64, payload: &Bytes) {
        if ghost_protocol::w3gs::incoming::decode_keepalive(payload).is_err() {
            return;
        }
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.sync_counter = p.sync_counter.saturating_add(1);
        }
    }

    pub fn handle_loaded(&mut self, conn_id: u64) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.loaded = true;
        }
        self.broadcast(outgoing::game_loaded_others(pid));

        if self.players.iter().all(|p| p.loaded) {
            tracing::info!(game = %self.cfg.name, "all players loaded, game is live");
            self.begin_playing();
        }
    }

    pub fn begin_loading(&mut self) {
        self.phase = GamePhase::Loading;
        self.broadcast(outgoing::countdown_start());
        self.broadcast(outgoing::countdown_end());
    }

    pub fn begin_playing(&mut self) {
        self.phase = GamePhase::Playing;
        for p in self.players.iter_mut() {
            p.loaded = true;
            p.sync_counter = 0;
        }
        self.sync_counter = 0;
        self.game_ticks = 0;
    }

    /// One scheduled tick. `skipped` counts periods lost to a stall.
    pub fn on_tick(&mut self, skipped: u32) {
        match self.phase {
            GamePhase::Lobby => {}
            GamePhase::Countdown { remaining } => {
                if remaining == 0 {
                    self.begin_loading();
                } else {
                    self.phase = GamePhase::Countdown { remaining: remaining - 1 };
                }
            }
            GamePhase::Loading => {}
            GamePhase::Playing => {
                if self.check_lag() {
                    return; // lag screen is up; no actions go out this tick
                }
                self.send_all_actions(skipped);
            }
            GamePhase::Over => self.finished = true,
        }

        if self.players.is_empty() && matches!(self.phase, GamePhase::Playing | GamePhase::Loading) {
            tracing::info!(game = %self.cfg.name, "no players left, ending game");
            self.phase = GamePhase::Over;
        }
    }

    /// Encodes the tick's action packets once and shares them with every player.
    pub fn send_all_actions(&mut self, skipped: u32) {
        let latency_ms = self.tick.period().as_millis() as u32;
        // A skipped period still advanced game time; report the real interval so
        // clients keep their simulation aligned with ours.
        let elapsed = latency_ms.saturating_mul(skipped + 1);
        let send_interval = elapsed.min(u16::MAX as u32) as u16;

        self.game_ticks = self.game_ticks.wrapping_add(elapsed);
        self.sync_counter = self.sync_counter.wrapping_add(1);

        let queued = std::mem::take(&mut self.actions);
        let mut batch: Vec<ActionBlock> = Vec::new();
        let mut batch_len = 0usize;

        for action in queued {
            let len = action.wire_len();
            if batch_len + len > MAX_ACTION_PAYLOAD && !batch.is_empty() {
                match outgoing::incoming_action2(&batch) {
                    Ok(b) => self.broadcast(b),
                    Err(e) => tracing::warn!(error = %e, "failed to build overflow packet"),
                }
                batch.clear();
                batch_len = 0;
            }
            batch_len += len;
            batch.push(action);
        }

        // The main packet always goes out, even empty: it is the clock tick.
        match outgoing::incoming_action(&batch, send_interval) {
            Ok(b) => self.broadcast(b),
            Err(e) => tracing::warn!(error = %e, "failed to build action packet"),
        }
    }
}
```

Убрать одноимённые заглушки из `actor.rs` и добавить `pub mod actions;` в `lib.rs`.

- [ ] **Step 4: Добавить временную заглушку `check_lag`**

В `crates/ghost-engine/src/actions.rs`, до Task 12:

```rust
impl GameState {
    /// Replaced by the real implementation in Task 12.
    pub(crate) fn check_lag(&mut self) -> bool {
        false
    }
}
```

- [ ] **Step 5: Запустить тесты**

Run: `cargo test -p ghost-engine`
Expected: PASS, 26 тестов.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): encode each action tick once and share it across players"
```

---

## Task 12: Обнаружение лага и лаг-скрин

Порт логики из `src/game_base.rs:600-713`. Игрок считается лагающим, когда его `sync_counter` отстаёт от игрового больше чем на `sync_limit`. Пока хоть кто-то лагает, экшены не рассылаются — все ждут отстающего.

**Files:**
- Create: `crates/ghost-engine/src/lagcheck.rs`
- Modify: `crates/ghost-engine/src/actions.rs` (убрать заглушку `check_lag`), `src/lib.rs`

**Interfaces:**
- Consumes: `GameState`, `outgoing::{start_lag, stop_lag}`, `Player.sync_counter/lagging/started_lagging`.
- Produces: `GameState::check_lag(&mut self) -> bool` (true = лаг-скрин активен, тик экшенов пропускается), `GameState::drop_lagging_players(&mut self, max_lag: Duration)`.

- [ ] **Step 1: Написать падающие тесты**

`crates/ghost-engine/src/lagcheck.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use ghost_protocol::w3gs::ids;

    #[test]
    fn a_player_past_the_sync_limit_raises_the_lag_screen() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5; // 55 ticks behind
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        assert!(st.check_lag(), "lag screen must be up");
        assert!(st.lagging);
        assert!(st.players.by_pid(2).unwrap().lagging);
        assert!(drain_ids(&mut rxs[0]).contains(&ids::START_LAG));
    }

    #[test]
    fn no_lag_screen_while_everyone_is_within_the_limit() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 30; // 30 < 50
        assert!(!st.check_lag());
        assert!(!st.lagging);
    }

    #[test]
    fn catching_up_halfway_clears_the_lag_screen() {
        let (mut st, mut rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5;
        assert!(st.check_lag());
        for rx in rxs.iter_mut() {
            let _ = drain_ids(rx);
        }

        // Legacy rule (src/game_base.rs:667): a lagger recovers once it is
        // within half the sync limit, not merely one tick better.
        st.players.by_pid_mut(2).unwrap().sync_counter = 40; // 20 < 50/2
        assert!(!st.check_lag());
        assert!(!st.lagging);
        assert!(drain_ids(&mut rxs[0]).contains(&ids::STOP_LAG));
    }

    #[test]
    fn a_player_lagging_past_the_timeout_is_dropped() {
        let (mut st, _rxs) = seated_game(2);
        st.begin_playing();
        st.sync_counter = 60;
        st.players.by_pid_mut(1).unwrap().sync_counter = 60;
        st.players.by_pid_mut(2).unwrap().sync_counter = 5;
        st.check_lag();
        st.players.by_pid_mut(2).unwrap().started_lagging =
            Some(Instant::now() - Duration::from_secs(120));

        st.drop_lagging_players(Duration::from_secs(60));
        st.reap_left_players();

        assert_eq!(st.players.len(), 1);
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-engine lagcheck`
Expected: FAIL — `check_lag` возвращает всегда `false`, три теста красные.

- [ ] **Step 3: Удалить заглушку и реализовать**

Убрать `check_lag` из `actions.rs`, создать `lagcheck.rs`:

```rust
use std::time::{Duration, Instant};

use ghost_protocol::w3gs::outgoing;

use crate::state::GameState;

impl GameState {
    /// Returns true while the lag screen is up, meaning no actions go out.
    pub fn check_lag(&mut self) -> bool {
        let limit = self.cfg.sync_limit;
        let game_sync = self.sync_counter;

        let mut newly_lagging: Vec<(u8, u32)> = Vec::new();
        let mut recovered: Vec<(u8, u32)> = Vec::new();

        for p in self.players.iter_mut() {
            let behind = game_sync.saturating_sub(p.sync_counter);
            if p.lagging {
                // Recover only once comfortably caught up (src/game_base.rs:667).
                if behind < limit / 2 {
                    p.lagging = false;
                    let lag_ms = p
                        .started_lagging
                        .take()
                        .map(|t| t.elapsed().as_millis() as u32)
                        .unwrap_or(0);
                    recovered.push((p.pid, lag_ms));
                }
            } else if behind > limit {
                p.lagging = true;
                p.started_lagging = Some(Instant::now());
                newly_lagging.push((p.pid, 0));
            }
        }

        for (pid, lag_ms) in recovered {
            tracing::info!(game = %self.cfg.name, pid, "player stopped lagging");
            self.broadcast(outgoing::stop_lag(pid, lag_ms));
        }

        if !newly_lagging.is_empty() {
            tracing::info!(
                game = %self.cfg.name,
                laggers = ?newly_lagging.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
                "lag screen raised"
            );
            match outgoing::start_lag(&newly_lagging) {
                Ok(b) => self.broadcast(b),
                Err(e) => tracing::warn!(error = %e, "failed to build start_lag"),
            }
        }

        self.lagging = self.players.iter().any(|p| p.lagging);
        self.lagging
    }

    /// Drops anyone stuck on the lag screen longer than `max_lag`.
    pub fn drop_lagging_players(&mut self, max_lag: Duration) {
        for p in self.players.iter_mut() {
            if p.lagging
                && p.left.is_none()
                && p.started_lagging.is_some_and(|t| t.elapsed() >= max_lag)
            {
                p.left = Some(format!("was dropped after lagging for {}s", max_lag.as_secs()));
            }
        }
    }
}
```

Вызвать `self.drop_lagging_players(Duration::from_secs(60))` в ветке `GamePhase::Playing` метода `on_tick`, сразу после `check_lag`. Добавить `pub mod lagcheck;` в `lib.rs`.

- [ ] **Step 4: Запустить тесты**

Run: `cargo test -p ghost-engine`
Expected: PASS, 30 тестов.

- [ ] **Step 5: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): detect laggers, raise the lag screen and drop timeouts"
```

---

## Task 13: Чат-команды и раздача карты

Порт `src/game_base.rs:1774-1918` (команды) и `2008-2065` (загрузка карты). Раздача карты идёт кусками по 1442 байта; в легаси она делалась внутри игрового цикла, здесь — тоже в тике, но с жёстким бюджетом на тик, чтобы скачивание не съело время у экшенов.

**Files:**
- Create: `crates/ghost-engine/src/chat.rs`, `src/mapxfer.rs`, `src/lang.rs`
- Modify: `crates/ghost-engine/src/state.rs` (поле `pub downloads: Vec<Download>`), `src/actor.rs`, `src/lib.rs`

**Interfaces:**
- Consumes: `incoming::{ChatToHost, MapSizeReport, decode_map_part_ok, decode_pong_to_host}`, `outgoing::{chat_from_host, start_download, map_part}`.
- Produces:
  - `struct Download { pub pid: u8, pub sent_upto: u32, pub acked_upto: u32, pub started: Instant }`
  - `GameState::handle_chat_to_host(&mut self, conn_id: u64, payload: &Bytes)`
  - `GameState::handle_map_size(&mut self, conn_id: u64, payload: &Bytes)`
  - `GameState::handle_map_part_ok(&mut self, conn_id: u64, payload: &Bytes)`
  - `GameState::handle_pong(&mut self, conn_id: u64, payload: &Bytes)`
  - `GameState::handle_drop_request(&mut self, conn_id: u64)`
  - `GameState::pump_downloads(&mut self)` — вызывается каждый тик, шлёт не более `MAX_PARTS_PER_TICK = 10` кусков на игрока
  - `enum ChatCommand { Start, Abort, Open(u8), Close(u8), Swap(u8, u8), Kick(String), Ping, Unhost, Say(String), Unknown(String) }` + `fn parse_command(trigger: char, msg: &str) -> Option<ChatCommand>`
  - `mod lang` — функции сообщений: `player_joined(name) -> String`, `player_left(name, reason) -> String`, `countdown(n) -> String`, `unable_to_start_not_enough(n) -> String`, `player_pings(pairs) -> String`, `command_not_allowed() -> String` (тексты перенести из `src/lang.rs`)

- [ ] **Step 1: Написать падающие тесты для парсера команд**

`crates/ghost-engine/src/chat.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_with_and_without_arguments() {
        assert_eq!(parse_command('!', "!start"), Some(ChatCommand::Start));
        assert_eq!(parse_command('!', "!close 3"), Some(ChatCommand::Close(2)));
        assert_eq!(parse_command('!', "!open 1"), Some(ChatCommand::Open(0)));
        assert_eq!(parse_command('!', "!swap 1 4"), Some(ChatCommand::Swap(0, 3)));
        assert_eq!(parse_command('!', "!kick Slash"), Some(ChatCommand::Kick("Slash".into())));
        assert_eq!(parse_command('!', "!ping"), Some(ChatCommand::Ping));
    }

    #[test]
    fn slot_numbers_are_one_based_on_the_wire_and_rejected_when_zero() {
        assert_eq!(parse_command('!', "!close 0"), None);
        assert_eq!(parse_command('!', "!close abc"), None);
    }

    #[test]
    fn plain_chat_is_not_a_command() {
        assert_eq!(parse_command('!', "hello"), None);
        assert_eq!(parse_command('!', ""), None);
        assert_eq!(parse_command('!', "!"), None);
    }

    #[test]
    fn the_trigger_character_is_configurable() {
        assert_eq!(parse_command('.', ".start"), Some(ChatCommand::Start));
        assert_eq!(parse_command('.', "!start"), None);
    }

    #[test]
    fn commands_are_case_insensitive() {
        assert_eq!(parse_command('!', "!START"), Some(ChatCommand::Start));
    }
}
```

`crates/ghost-engine/src/mapxfer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, seated_game};
    use ghost_protocol::w3gs::ids;

    #[test]
    fn a_client_reporting_a_partial_map_starts_a_download() {
        let (mut st, mut rxs) = seated_game(1);
        st.cfg.map.size = 100_000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 100_000]));
        let _ = drain_ids(&mut rxs[0]);

        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut p, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut p, 1);
        bytes::BufMut::put_u32_le(&mut p, 0); // has 0 of 100000 bytes
        st.handle_map_size(1, &p.freeze());

        assert_eq!(st.downloads.len(), 1);
        assert!(drain_ids(&mut rxs[0]).contains(&ids::START_DOWNLOAD));
    }

    #[test]
    fn a_client_with_the_whole_map_starts_no_download() {
        let (mut st, _rxs) = seated_game(1);
        st.cfg.map.size = 1000;
        let mut p = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut p, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut p, 1);
        bytes::BufMut::put_u32_le(&mut p, 1000);
        st.handle_map_size(1, &p.freeze());
        assert!(st.downloads.is_empty());
    }

    #[test]
    fn each_tick_sends_a_bounded_number_of_map_parts() {
        let (mut st, mut rxs) = seated_game(1);
        st.cfg.map.size = 100_000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 100_000]));
        st.downloads.push(Download::new(1));
        let _ = drain_ids(&mut rxs[0]);

        st.pump_downloads();

        let sent = drain_ids(&mut rxs[0]);
        assert_eq!(sent.len(), MAX_PARTS_PER_TICK);
        assert!(sent.iter().all(|&id| id == ids::MAP_PART));
    }

    #[test]
    fn a_finished_download_is_removed() {
        let (mut st, _rxs) = seated_game(1);
        st.cfg.map.size = 1000;
        st.cfg.map.data = Some(std::sync::Arc::new(vec![0u8; 1000]));
        let mut d = Download::new(1);
        d.sent_upto = 1000;
        d.acked_upto = 1000;
        st.downloads.push(d);

        st.pump_downloads();

        assert!(st.downloads.is_empty());
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-engine chat mapxfer`
Expected: FAIL — `cannot find function parse_command`.

- [ ] **Step 3: Реализовать `chat.rs`**

```rust
use bytes::Bytes;
use ghost_protocol::w3gs::incoming::ChatToHost;

use crate::lang;
use crate::players::NameMatch;
use crate::state::{GamePhase, GameState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCommand {
    Start,
    Abort,
    /// Slot ids are zero-based here; the chat syntax is one-based.
    Open(u8),
    Close(u8),
    Swap(u8, u8),
    Kick(String),
    Ping,
    Unhost,
    Say(String),
    Unknown(String),
}

fn slot_arg(s: &str) -> Option<u8> {
    let n: u8 = s.parse().ok()?;
    n.checked_sub(1)
}

pub fn parse_command(trigger: char, msg: &str) -> Option<ChatCommand> {
    let rest = msg.strip_prefix(trigger)?;
    let mut it = rest.split_whitespace();
    let verb = it.next()?.to_lowercase();
    let args: Vec<&str> = it.collect();

    Some(match verb.as_str() {
        "start" => ChatCommand::Start,
        "abort" => ChatCommand::Abort,
        "ping" => ChatCommand::Ping,
        "unhost" => ChatCommand::Unhost,
        "open" => ChatCommand::Open(slot_arg(args.first()?)?),
        "close" => ChatCommand::Close(slot_arg(args.first()?)?),
        "swap" => ChatCommand::Swap(slot_arg(args.first()?)?, slot_arg(args.get(1)?)?),
        "kick" => ChatCommand::Kick(args.first()?.to_string()),
        "say" => ChatCommand::Say(args.join(" ")),
        other => ChatCommand::Unknown(other.to_string()),
    })
}

impl GameState {
    pub fn handle_chat_to_host(&mut self, conn_id: u64, payload: &Bytes) {
        let Some((pid, name)) = self
            .players
            .by_conn(conn_id)
            .map(|p| (p.pid, p.name.clone()))
        else {
            return;
        };
        let chat = match ChatToHost::decode(payload) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(conn_id, error = %e, "malformed chat");
                return;
            }
        };

        // Team/colour/race/handicap change requests only apply in the lobby.
        if (0x11..=0x14).contains(&chat.flag) {
            if matches!(self.phase, GamePhase::Lobby) {
                self.apply_slot_request(pid, chat.flag, chat.byte);
                self.send_all_slot_info();
            }
            return;
        }

        let is_owner = name.eq_ignore_ascii_case(&self.cfg.owner);
        match parse_command('!', &chat.message) {
            Some(cmd) => {
                if !is_owner {
                    self.send_chat_to(pid, &lang::command_not_allowed());
                    return;
                }
                self.run_command(pid, cmd);
            }
            // Not a command: relay it to the recipients the client picked.
            None => {
                if let Ok(b) = ghost_protocol::w3gs::outgoing::chat_from_host(
                    pid,
                    &chat.to_pids,
                    chat.flag,
                    &chat.extra,
                    &chat.message,
                ) {
                    self.broadcast(b);
                }
            }
        }
    }

    fn run_command(&mut self, pid: u8, cmd: ChatCommand) {
        match cmd {
            ChatCommand::Start => {
                if self.players.len() < 2 {
                    let msg = lang::unable_to_start_not_enough(self.players.len());
                    self.send_chat_to(pid, &msg);
                } else {
                    let by = self.cfg.owner.clone();
                    self.start_countdown(&by);
                }
            }
            ChatCommand::Abort => {
                if matches!(self.phase, GamePhase::Countdown { .. }) {
                    self.phase = GamePhase::Lobby;
                    self.send_chat_all(&lang::countdown_aborted());
                }
            }
            ChatCommand::Open(sid) => {
                if self.slots.open(sid) {
                    self.send_all_slot_info();
                }
            }
            ChatCommand::Close(sid) => {
                if self.slots.close(sid) {
                    self.send_all_slot_info();
                }
            }
            ChatCommand::Swap(a, b) => {
                if self.slots.swap(a, b) {
                    self.send_all_slot_info();
                }
            }
            ChatCommand::Kick(name) => match self.players.by_name_partial(&name) {
                Ok(target) => {
                    let target_pid = target.pid;
                    if let Some(p) = self.players.by_pid_mut(target_pid) {
                        p.left = Some("was kicked".into());
                    }
                }
                Err(NameMatch::None) => self.send_chat_to(pid, &lang::no_such_player(&name)),
                Err(NameMatch::Ambiguous(n)) => {
                    self.send_chat_to(pid, &lang::ambiguous_player(&name, n))
                }
            },
            ChatCommand::Ping => {
                let pairs: Vec<(String, Option<u32>)> = self
                    .players
                    .iter()
                    .map(|p| (p.name.clone(), p.average_ping()))
                    .collect();
                let msg = lang::player_pings(&pairs);
                self.send_chat_to(pid, &msg);
            }
            ChatCommand::Unhost => {
                if matches!(self.phase, GamePhase::Lobby) {
                    self.finished = true;
                }
            }
            ChatCommand::Say(msg) => self.send_chat_all(&msg),
            ChatCommand::Unknown(v) => {
                tracing::debug!(command = %v, "unknown command");
            }
        }
    }

    fn apply_slot_request(&mut self, pid: u8, flag: u8, value: u8) {
        let Some(sid) = self.slots.sid_of_pid(pid) else { return };
        let Some(slot) = self.slots.as_wire().get(sid as usize).copied() else { return };
        let mut updated = slot;
        match flag {
            0x11 => updated.team = value.min(11),
            0x12 => updated.colour = value.min(11),
            0x13 => updated.race = value,
            0x14 => updated.handicap = value.clamp(50, 100),
            _ => return,
        }
        self.slots.replace(sid, updated);
    }

    pub fn send_chat_to(&mut self, pid: u8, message: &str) {
        let flag = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
            0x10
        } else {
            0x20
        };
        let extra: &[u8] = if flag == 0x20 { &[0, 0, 0, 0] } else { &[] };
        if let Ok(b) = ghost_protocol::w3gs::outgoing::chat_from_host(255, &[pid], flag, extra, message) {
            self.send_to(pid, b);
        }
    }
}
```

Добавить в `SlotTable` метод `pub fn replace(&mut self, sid: u8, info: SlotInfo) -> bool`, записывающий слот целиком.

- [ ] **Step 4: Реализовать `lang.rs`**

Перенести тексты из `src/lang.rs` в свободные функции, по одной на сообщение. Минимально нужны те, что вызываются выше:

```rust
pub fn command_not_allowed() -> String {
    "You are not the owner of this game.".to_string()
}

pub fn unable_to_start_not_enough(n: usize) -> String {
    format!("Unable to start: only {n} player(s) in the lobby.")
}

pub fn countdown_aborted() -> String {
    "Countdown aborted.".to_string()
}

pub fn no_such_player(name: &str) -> String {
    format!("No player matching [{name}].")
}

pub fn ambiguous_player(name: &str, n: usize) -> String {
    format!("[{name}] matches {n} players, be more specific.")
}

pub fn player_pings(pairs: &[(String, Option<u32>)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(name, ping)| match ping {
            Some(ms) => format!("{name}: {ms}ms"),
            None => format!("{name}: N/A"),
        })
        .collect();
    format!("Pings: {}", body.join(", "))
}

pub fn player_joined(name: &str) -> String {
    format!("[{name}] joined the game.")
}

pub fn player_left(name: &str, reason: &str) -> String {
    format!("[{name}] {reason}.")
}

pub fn countdown(n: u8) -> String {
    format!("Game starting in {n}...")
}
```

- [ ] **Step 5: Реализовать `mapxfer.rs`**

```rust
use std::time::Instant;

use bytes::Bytes;
use ghost_protocol::w3gs::{incoming::MapSizeReport, incoming, outgoing};

use crate::state::GameState;

/// Wire chunk size used by Warcraft III map downloads.
pub const MAP_CHUNK: usize = 1442;
/// Chunks sent per player per tick. Bounds how much of the tick budget map
/// downloads may consume; at 100 ms that is ~144 KB/s per downloader.
pub const MAX_PARTS_PER_TICK: usize = 10;

#[derive(Debug, Clone)]
pub struct Download {
    pub pid: u8,
    pub sent_upto: u32,
    pub acked_upto: u32,
    pub started: Instant,
}

impl Download {
    pub fn new(pid: u8) -> Self {
        Self { pid, sent_upto: 0, acked_upto: 0, started: Instant::now() }
    }
}

impl GameState {
    pub fn handle_map_size(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else { return };
        let report = match MapSizeReport::decode(payload) {
            Ok(r) => r,
            Err(e) => {
                tracing::debug!(conn_id, error = %e, "malformed map size report");
                return;
            }
        };

        if report.map_size >= self.cfg.map.size {
            if let Some(p) = self.players.by_pid_mut(pid) {
                p.download_status = 100;
            }
            return;
        }
        if self.cfg.map.data.is_none() {
            tracing::info!(pid, "player lacks the map and downloads are disabled");
            return;
        }
        if self.downloads.iter().any(|d| d.pid == pid) {
            return;
        }

        let mut d = Download::new(pid);
        d.sent_upto = report.map_size;
        d.acked_upto = report.map_size;
        self.downloads.push(d);
        self.send_to(pid, outgoing::start_download(255));
        tracing::info!(game = %self.cfg.name, pid, "map download started");
    }

    pub fn handle_map_part_ok(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else { return };
        let Ok(acked) = incoming::decode_map_part_ok(payload) else { return };
        let total = self.cfg.map.size.max(1);
        if let Some(d) = self.downloads.iter_mut().find(|d| d.pid == pid) {
            d.acked_upto = acked;
        }
        if let Some(p) = self.players.by_pid_mut(pid) {
            p.download_status = ((acked as u64 * 100) / total as u64).min(100) as u8;
        }
    }

    pub fn handle_pong(&mut self, conn_id: u64, payload: &Bytes) {
        let Ok(pong) = incoming::decode_pong_to_host(payload) else { return };
        let now = self.created_at.elapsed().as_millis() as u32;
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.record_ping(now.saturating_sub(pong) / 2);
        }
    }

    pub fn handle_drop_request(&mut self, conn_id: u64) {
        if !self.lagging {
            return;
        }
        tracing::info!(conn_id, "drop request while lagging, dropping laggers");
        for p in self.players.iter_mut() {
            if p.lagging && p.left.is_none() {
                p.left = Some("was dropped by vote".into());
            }
        }
    }

    /// Sends the next slice of every in-flight map download. Called once per tick.
    pub fn pump_downloads(&mut self) {
        let Some(data) = self.cfg.map.data.clone() else {
            self.downloads.clear();
            return;
        };
        let total = data.len() as u32;

        let mut packets: Vec<(u8, Bytes)> = Vec::new();
        self.downloads.retain_mut(|d| {
            if d.acked_upto >= total {
                tracing::info!(pid = d.pid, secs = d.started.elapsed().as_secs(), "map download finished");
                return false;
            }
            for _ in 0..MAX_PARTS_PER_TICK {
                if d.sent_upto >= total {
                    break;
                }
                let start = d.sent_upto as usize;
                let end = (start + MAP_CHUNK).min(data.len());
                match outgoing::map_part(255, d.pid, d.sent_upto, &data[start..end]) {
                    Ok(b) => packets.push((d.pid, b)),
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to build map part");
                        break;
                    }
                }
                d.sent_upto = end as u32;
            }
            true
        });

        for (pid, b) in packets {
            self.send_to(pid, b);
        }
    }
}
```

Добавить поле `pub downloads: Vec<Download>` в `GameState` (инициализация — `Vec::new()`), вызвать `self.pump_downloads()` в начале `on_tick`, и `pub mod chat; pub mod lang; pub mod mapxfer;` в `lib.rs`. Убрать соответствующие заглушки из `actor.rs`.

- [ ] **Step 6: Запустить тесты**

Run: `cargo test -p ghost-engine && cargo clippy -p ghost-engine -- -D warnings`
Expected: PASS, 39 тестов.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-engine
git commit -m "feat(engine): add chat commands, slot requests and bounded map downloads"
```

---

## Task 14: BNCS-клиент (PvPGN)

Порт `src/bnet.rs` + `src/bnetprotocol.rs` + `src/bncsutilinterface.rs`. Тот же принцип: актор с `mpsc`, никаких глобальных `m_BNETs: Lazy<RwLock<Vec<Arc<Mutex<BNET>>>>>`. Криптография логина (NLS/SRP, хеш ключей, версионный хеш) переносится **как есть** — это проверенный код, менять его алгоритмику нельзя.

**Files:**
- Create: `crates/ghost-bnet/Cargo.toml`, `src/lib.rs`, `src/auth.rs`, `src/client.rs`, `src/advert.rs`
- Create: `crates/ghost-protocol/src/bncs/incoming.rs`, `crates/ghost-protocol/src/bncs/outgoing.rs`

**Interfaces:**
- Consumes: `ghost_protocol::bncs::{BncsCodec, ids}`, `BufExt`, `put_cstring`.
- Produces:
  - `bncs::outgoing::{auth_info, auth_check, account_logon, account_logon_proof, enter_chat, join_channel, chat_command, netgameport, ping, startadvex3, stopadv, null}` — все возвращают `Result<Bytes, ProtoError>`
  - `bncs::incoming::{AuthInfo, AuthCheck, LogonProof, ChatEvent, decode_*}`
  - `enum BnetEvent { Connected, LoggedIn, ChatMessage { user: String, text: String }, Whisper { user: String, text: String }, Disconnected(String) }`
  - `enum BnetCmd { CreateGame { name: String, map: MapAdvert, host_counter: u32 }, RefreshGame { players: u32, slots: u32 }, UnhostGame, SendChat(String), Shutdown }`
  - `struct BnetHandle { tx: mpsc::Sender<BnetCmd> }`
  - `fn spawn_bnet(cfg: BnetConfig, events: mpsc::Sender<BnetEvent>) -> (BnetHandle, JoinHandle<()>)`
  - `struct BnetConfig { pub server: String, pub port: u16, pub username: String, pub password: String, pub cdkey_roc: String, pub cdkey_tft: String, pub first_channel: String, pub root_admins: Vec<String>, pub command_trigger: char, pub war3_version: u8, pub exe_version: [u8;4], pub exe_version_hash: [u8;4], pub reconnect_delay: Duration }`

- [ ] **Step 1: Написать падающие тесты для BNCS-пакетов**

`crates/ghost-protocol/src/bncs/outgoing.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_packet_is_framed_with_0xff_and_a_correct_length() {
        let packets = [
            null(),
            enter_chat().unwrap(),
            join_channel("iccup.pro").unwrap(),
            chat_command("/whois slash").unwrap(),
            netgameport(6112),
            stopadv(),
        ];
        for p in packets {
            assert_eq!(p[0], 0xFF, "bncs header");
            assert_eq!(u16::from_le_bytes([p[2], p[3]]) as usize, p.len(), "length field");
        }
    }

    #[test]
    fn join_channel_carries_a_nul_terminated_name() {
        let p = join_channel("iccup.pro").unwrap();
        assert_eq!(p[1], ids::SID_JOINCHANNEL);
        assert_eq!(p[p.len() - 1], 0);
        assert!(p.windows(9).any(|w| w == b"iccup.pro"));
    }

    #[test]
    fn auth_info_declares_the_configured_war3_version() {
        let p = auth_info(26, true, 1033, "USA", "United States").unwrap();
        assert_eq!(p[1], ids::SID_AUTH_INFO);
        // Product is "PX3W" (W3XP reversed) for The Frozen Throne.
        assert!(p.windows(4).any(|w| w == b"PX3W"));
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-protocol bncs`
Expected: FAIL — `cannot find function join_channel`.

- [ ] **Step 3: Перенести тела BNCS-пакетов**

Перенести из `src/bnetprotocol.rs` в `crates/ghost-protocol/src/bncs/outgoing.rs`, по одной функции на `SEND_SID_*`, заменив `ByteArray` на `BytesMut` и обрамление на `Frame::new(id, payload).encode_with(BNCS_HEADER)`:

| Новая функция | Легаси-источник |
|---|---|
| `null()` | `src/bnetprotocol.rs:368` |
| `stopadv()` | `src/bnetprotocol.rs:378` |
| `getadvlistex(game_name)` | `src/bnetprotocol.rs:388` |
| `enter_chat()` | `src/bnetprotocol.rs:409` |
| `join_channel(channel)` | `src/bnetprotocol.rs:421` |
| `chat_command(cmd)` | `src/bnetprotocol.rs:439` |
| `checkad()` | `src/bnetprotocol.rs:450` |
| `startadvex3(...)` | `src/bnetprotocol.rs:465` |
| `notifyjoin(game_name)` | `src/bnetprotocol.rs:551` |
| `ping(value)` | `src/bnetprotocol.rs:567` |
| `logon_response(...)` | `src/bnetprotocol.rs:580` |
| `netgameport(port)` | `src/bnetprotocol.rs:594` |
| `auth_info(...)` | `src/bnetprotocol.rs:605` |
| `auth_check(...)` | `src/bnetprotocol.rs:639` |
| `account_logon(...)` | `src/bnetprotocol.rs:663` |
| `account_logon_proof(...)` | `src/bnetprotocol.rs:677` |

Аналогично `RECEIVE_SID_*` (`src/bnetprotocol.rs:131-361`) → `crates/ghost-protocol/src/bncs/incoming.rs`, с возвратом `Result<_, ProtoError>` вместо `Option`.

- [ ] **Step 4: Перенести криптографию логина в `ghost-bnet/src/auth.rs`**

Перенести из `src/bncsutil.rs` и `src/bncsutilinterface.rs`: расчёт хеша CD-ключей, версионного хеша по `ValueStringFormula`, и NLS/SRP-обмен (`client_key`, `password_proof`). Алгоритмы **не менять** — только типы: `Vec<u8>` → `[u8; N]` там, где длина фиксирована, и `Result` вместо молчаливых пустых массивов. Если легаси грузит `bncsutil` через `libloading`, реализовать те же вычисления на `sha1` из workspace и снять зависимость `libloading`.

- [ ] **Step 5: Реализовать актор `client.rs`**

Машина состояний повторяет `src/bnet.rs:update`, но событийно:

```rust
/// One BNCS session. Reconnects with a fixed delay on any disconnect.
enum Stage {
    Connecting,
    AwaitAuthInfo,
    AwaitAuthCheck,
    AwaitLogonProof,
    InChat,
}
```

Цикл — `tokio::select!` над: входящими фреймами (`FramedRead<_, BncsCodec>`), командами `BnetCmd`, таймером `SID_NULL` каждые 30 секунд (keepalive) и таймером обновления рекламы игры каждые 3 секунды (`startadvex3` с текущим числом слотов). При обрыве — `Disconnected`, пауза `reconnect_delay`, `Stage::Connecting`.

Чат-события: `SID_CHATEVENT` с `event_id` 0x05 (TALK) и 0x04 (WHISPER) превращаются в `BnetEvent::ChatMessage` / `Whisper`; команды с триггером от root-админов супервизор превращает в `GameCmd`.

- [ ] **Step 6: Тест интеграции на локальном фейковом сервере**

`crates/ghost-bnet/tests/handshake.rs`: поднять `TcpListener` на `127.0.0.1:0`, который отвечает записанной последовательностью `SID_AUTH_INFO` → `SID_AUTH_CHECK` (успех) → `SID_AUTH_ACCOUNTLOGON` → `SID_AUTH_ACCOUNTLOGONPROOF` (успех) → `SID_ENTERCHAT`. Проверить, что актор доходит до `BnetEvent::LoggedIn` за 5 секунд и что первым байтом в сокет ушёл селектор протокола `0x01`.

- [ ] **Step 7: Запустить тесты**

Run: `cargo test -p ghost-protocol bncs && cargo test -p ghost-bnet`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/ghost-bnet crates/ghost-protocol
git commit -m "feat(bnet): add BNCS packet builders and a reconnecting login actor"
```

---

## Task 15: GProxy++ — реконнект без потери игры

Порт `src/engine/gproxy.rs` и логики `gproxy`-полей из `src/gameplayer.rs`. Каждому GProxy-клиенту ведётся кольцевой буфер отправленных пакетов; при переподключении по `GPS_RECONNECT` буфер переигрывается с подтверждённой позиции.

**Files:**
- Modify: `crates/ghost-engine/src/gproxy.rs` (перенос из `src/engine/gproxy.rs`), `src/state.rs`, `src/actor.rs`
- Modify: `crates/ghost-net/src/conn.rs` (распознавание GPS-фреймов на том же сокете)

**Interfaces:**
- Consumes: `ghost_protocol::gps::{GPS_HEADER, ids as gps_ids, init, ack, reject, decode_reconnect, ReconnectReq, reject_reason}`.
- Produces:
  - `struct GProxyBuffer { capacity: usize, first_packet_id: u32, packets: VecDeque<Bytes> }` с методами `new(capacity: usize)`, `push(&mut self, packet: Bytes)`, `total_sent(&self) -> u32`, `replay_from(&self, last_received: u32) -> Option<Vec<Bytes>>`
  - `GameState::handle_gps_reconnect(&mut self, conn_id: u64, req: ReconnectReq, link: PlayerLink) -> bool`
  - поле `Player.gproxy_buffer: Option<GProxyBuffer>`, `Player.disconnected_since: Option<Instant>`
  - `GameState::reap_gproxy_timeouts(&mut self, grace: Duration)`

- [ ] **Step 1: Написать падающие тесты**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_returns_everything_after_the_acknowledged_packet() {
        let mut b = GProxyBuffer::new(10);
        for i in 0..5u8 {
            b.push(Bytes::from(vec![i]));
        }
        assert_eq!(b.total_sent(), 5);
        let replay = b.replay_from(3).expect("packets 4 and 5");
        assert_eq!(replay.len(), 2);
        assert_eq!(&replay[0][..], &[3]);
    }

    #[test]
    fn replay_of_everything_returns_the_whole_buffer() {
        let mut b = GProxyBuffer::new(10);
        b.push(Bytes::from_static(&[1]));
        assert_eq!(b.replay_from(0).unwrap().len(), 1);
    }

    #[test]
    fn replay_fails_once_the_needed_packets_have_been_evicted() {
        let mut b = GProxyBuffer::new(3);
        for i in 0..10u8 {
            b.push(Bytes::from(vec![i]));
        }
        // Packet 2 is long gone: the client cannot be resynchronised.
        assert!(b.replay_from(2).is_none());
        assert!(b.replay_from(7).is_some());
    }

    #[test]
    fn a_client_claiming_more_packets_than_we_sent_is_rejected() {
        let mut b = GProxyBuffer::new(10);
        b.push(Bytes::from_static(&[1]));
        assert!(b.replay_from(99).is_none());
    }

    #[tokio::test]
    async fn a_valid_reconnect_reattaches_the_player_and_replays() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.begin_playing();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.players.by_pid_mut(1).unwrap().gproxy_buffer = Some(GProxyBuffer::new(100));
        let key = st.players.by_pid(1).unwrap().reconnect_key;
        st.on_tick(0); // one action packet is buffered
        st.players.by_pid_mut(1).unwrap().disconnected_since = Some(Instant::now());

        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let ok = st.handle_gps_reconnect(
            99,
            ReconnectReq { pid: 1, reconnect_key: key, last_packet: 0 },
            PlayerLink::for_test(tx),
        );

        assert!(ok);
        assert_eq!(st.players.by_pid(1).unwrap().conn_id, 99);
        assert!(st.players.by_pid(1).unwrap().disconnected_since.is_none());
        assert!(rx.try_recv().is_ok(), "buffered packets must be replayed");
    }

    #[tokio::test]
    async fn a_wrong_reconnect_key_is_refused() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.begin_playing();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        let ok = st.handle_gps_reconnect(
            99,
            ReconnectReq { pid: 1, reconnect_key: 0xBAD, last_packet: 0 },
            PlayerLink::for_test(tx),
        );
        assert!(!ok);
        assert_ne!(st.players.by_pid(1).unwrap().conn_id, 99);
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-engine gproxy`
Expected: FAIL.

- [ ] **Step 3: Реализовать буфер и реконнект**

```rust
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use bytes::Bytes;
use ghost_net::PlayerLink;
use ghost_protocol::gps::{ReconnectReq, ack, reject, reject_reason};

use crate::state::GameState;

/// Ring buffer of packets sent to one GProxy client, so a reconnecting client
/// can be replayed exactly what it missed.
#[derive(Debug, Clone)]
pub struct GProxyBuffer {
    capacity: usize,
    /// Sequence number of the oldest packet still held.
    first_packet_id: u32,
    packets: VecDeque<Bytes>,
}

impl GProxyBuffer {
    pub fn new(capacity: usize) -> Self {
        Self { capacity, first_packet_id: 0, packets: VecDeque::with_capacity(capacity) }
    }

    pub fn push(&mut self, packet: Bytes) {
        if self.packets.len() == self.capacity {
            self.packets.pop_front();
            self.first_packet_id += 1;
        }
        self.packets.push_back(packet);
    }

    /// Total packets ever pushed, i.e. the sequence number of the newest one.
    pub fn total_sent(&self) -> u32 {
        self.first_packet_id + self.packets.len() as u32
    }

    /// Packets the client has not confirmed, or None when they were evicted or
    /// the client claims to have more than we ever sent.
    pub fn replay_from(&self, last_received: u32) -> Option<Vec<Bytes>> {
        if last_received > self.total_sent() || last_received < self.first_packet_id {
            return None;
        }
        let skip = (last_received - self.first_packet_id) as usize;
        Some(self.packets.iter().skip(skip).cloned().collect())
    }
}

impl GameState {
    pub fn handle_gps_reconnect(
        &mut self,
        conn_id: u64,
        req: ReconnectReq,
        link: PlayerLink,
    ) -> bool {
        let Some(p) = self.players.by_pid_mut(req.pid) else {
            let _ = link.try_send(reject(reject_reason::NOT_FOUND));
            return false;
        };
        if !p.gproxy || p.reconnect_key != req.reconnect_key {
            let _ = link.try_send(reject(reject_reason::INVALID_KEY));
            return false;
        }
        let Some(replay) = p
            .gproxy_buffer
            .as_ref()
            .and_then(|b| b.replay_from(req.last_packet))
        else {
            let _ = link.try_send(reject(reject_reason::NOT_FOUND));
            return false;
        };

        let received = p.gproxy_buffer.as_ref().map(|b| b.total_sent()).unwrap_or(0);
        p.conn_id = conn_id;
        p.link = link;
        p.disconnected_since = None;
        p.left = None;

        let _ = p.link.try_send(ack(received));
        for packet in replay {
            if p.link.try_send(packet).is_err() {
                break;
            }
        }
        tracing::info!(game = %self.cfg.name, pid = req.pid, "gproxy client reconnected");
        true
    }

    /// Removes GProxy players who never came back within the grace period.
    pub fn reap_gproxy_timeouts(&mut self, grace: Duration) {
        for p in self.players.iter_mut() {
            if p.disconnected_since.is_some_and(|t| t.elapsed() >= grace) {
                p.left = Some("failed to reconnect in time".into());
                p.disconnected_since = None;
            }
        }
    }
}
```

- [ ] **Step 4: Подключить к жизненному циклу**

1. В `GameState::broadcast` — после успешного `try_send` пушить пакет в `p.gproxy_buffer`, если он есть.
2. В `handle_conn_closed` — для GProxy-игрока вместо `left` ставить `disconnected_since = Some(Instant::now())`, чтобы место в игре сохранилось.
3. В `on_tick` — вызывать `self.reap_gproxy_timeouts(Duration::from_secs(reconnect_wait))`, где `reconnect_wait` берётся из `GameConfig` (дефолт 180 секунд).
4. В `handle_req_join` — после посадки игрока слать `gps::init(1, pid, reconnect_key, 0)`; клиент без GProxy этот пакет проигнорирует.
5. В `ghost-net::conn` — reader должен различать заголовки `0xF7` и `0xF8` на одном сокете. Заменить `W3gsCodec` на кодек, который читает оба: если `src[0] == 0xF8`, вернуть фрейм с признаком GPS. Реализовать как `enum AnyFrame { W3gs(Frame), Gps(Frame) }` и `struct DualCodec;`, а `ConnEventKind::Frame(Frame)` заменить на `ConnEventKind::Frame(AnyFrame)`; обновить `on_frame` в акторе на матч по `AnyFrame`.

- [ ] **Step 5: Запустить тесты**

Run: `cargo test -p ghost-engine && cargo test -p ghost-net`
Expected: PASS, 45 тестов в engine.

- [ ] **Step 6: Commit**

```bash
git add crates/ghost-engine crates/ghost-net
git commit -m "feat(engine): add GProxy++ replay buffer and reconnect handling"
```

---

## Task 16: Спектатор-релей (DotaTV) и запись реплея

Перенос `src/spectator_relay.rs`. Зрители — не игроки: они не занимают слот, не влияют на `sync_counter` и никогда не поднимают лаг-скрин. Релей получает уже закодированные `Bytes` тика и раздаёт их с настраиваемой задержкой.

**Files:**
- Create: `crates/ghost-spectator/Cargo.toml`, `src/lib.rs`, `src/relay.rs`, `src/replay.rs`
- Modify: `crates/ghost-engine/src/state.rs`, `src/actions.rs`

**Interfaces:**
- Consumes: `ghost_net::{spawn_listener, spawn_conn, PlayerLink}`, `ghost_protocol::w3gs::outgoing`.
- Produces:
  - `struct RelayConfig { pub port: u16, pub delay: Duration, pub max_viewers: usize, pub game_name: String }`
  - `enum RelayCmd { GameBlock(Bytes), PlayerInfo { pid: u8, name: String }, GameOver, Shutdown }`
  - `struct RelayHandle { tx: mpsc::Sender<RelayCmd> }` + `RelayHandle::push(&self, block: Bytes)`
  - `fn spawn_relay(cfg: RelayConfig) -> (RelayHandle, JoinHandle<()>)`
  - `struct ReplayWriter` + `ReplayWriter::create(path: &Path, game_name: &str) -> io::Result<Self>`, `ReplayWriter::push_block(&mut self, block: &[u8]) -> io::Result<()>`, `ReplayWriter::finish(self) -> io::Result<()>`

- [ ] **Step 1: Написать падающие тесты**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn blocks_are_released_only_after_the_configured_delay() {
        let (handle, _join) = spawn_relay(RelayConfig {
            port: 0,
            delay: Duration::from_secs(120),
            max_viewers: 8,
            game_name: "t".into(),
        });
        handle.push(Bytes::from_static(&[1, 2, 3]));

        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(handle.debug_released_count().await, 0);

        tokio::time::advance(Duration::from_secs(61)).await;
        assert_eq!(handle.debug_released_count().await, 1);
    }

    #[tokio::test]
    async fn viewers_beyond_the_limit_are_refused() {
        let cfg = RelayConfig { port: 0, delay: Duration::ZERO, max_viewers: 2, game_name: "t".into() };
        let mut relay = Relay::new(cfg);
        assert!(relay.add_viewer(1, test_link()).is_ok());
        assert!(relay.add_viewer(2, test_link()).is_ok());
        assert!(relay.add_viewer(3, test_link()).is_err());
    }

    #[test]
    fn replay_header_is_written_and_the_block_count_updated() {
        let dir = std::env::temp_dir().join("ghostrs-replay-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.w3g");
        let mut w = ReplayWriter::create(&path, "test").unwrap();
        w.push_block(&[0xF7, 0x0C, 0x04, 0x00]).unwrap();
        w.finish().unwrap();

        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b"Warcraft III recorded game\x1A\0"));
        assert!(data.len() > 68);
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-spectator`
Expected: FAIL — крейта нет.

- [ ] **Step 3: Реализовать релей**

Актор с `select!` над: `RelayCmd`, таймером выпуска отложенных блоков (`VecDeque<(Instant, Bytes)>`, выпускать всё, у чего `deadline <= now`), и каналом новых зрителей от `spawn_listener`. Новый зритель получает синтетическое лобби (`slot_info_join`, `player_info` для каждого игрока, `countdown_start`, `countdown_end`), затем поток отложенных блоков. Отправка — тот же `try_send`; зритель, не успевающий читать, отключается и на игру не влияет.

- [ ] **Step 4: Реализовать `replay.rs`**

Формат `.w3g`: заголовок 68 байт (`"Warcraft III recorded game\x1A\0"`, размер заголовка `0x44`, сжатый/несжатый размеры, версия, число блоков), затем блоки, сжатые zlib кусками по 8 КБ. Использовать `flate2`. Размеры и число блоков дописываются в `finish()` через `seek` к началу.

- [ ] **Step 5: Подключить к движку**

В `GameState` добавить `pub relay: Option<RelayHandle>`. В `send_all_actions`, сразу после построения основного пакета, вызвать `if let Some(r) = &self.relay { r.push(packet.clone()) }` — это refcount-клон `Bytes`, а не копия.

- [ ] **Step 6: Запустить тесты**

Run: `cargo test -p ghost-spectator && cargo test -p ghost-engine`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-spectator crates/ghost-engine
git commit -m "feat(spectator): add delayed DotaTV relay and w3g replay writer"
```

---

## Task 17: Хранилище на SQLite (WAL)

Перенос `src/ghostdb.rs`. Запись в БД не должна попадать в игровой тик, поэтому `rusqlite` (синхронный) живёт в отдельной таске на `spawn_blocking`-пуле, а движок общается с ней через `mpsc`.

**Files:**
- Create: `crates/ghost-store/Cargo.toml`, `src/lib.rs`, `src/schema.rs`, `src/writer.rs`

**Interfaces:**
- Produces:
  - `enum StoreCmd { AddBan { name: String, ip: String, admin: String, reason: String }, RemoveBan { name: String }, LogGame { name: String, map: String, started: i64, ended: i64, players: Vec<String> }, Query(StoreQuery) }`
  - `enum StoreQuery { IsBanned { name: String, ip: String, reply: oneshot::Sender<Option<Ban>> }, IsAdmin { name: String, reply: oneshot::Sender<bool> } }`
  - `struct Store { tx: mpsc::Sender<StoreCmd> }` + `Store::open(path: &Path) -> anyhow::Result<(Store, JoinHandle<()>)>`, `Store::ban(&self, ...)`, `async Store::is_banned(&self, name, ip) -> Option<Ban>`
  - `struct Ban { pub name: String, pub ip: String, pub admin: String, pub reason: String, pub created: i64 }`

Схема (`schema.rs`, применяется при открытии):

```sql
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 5000;

CREATE TABLE IF NOT EXISTS bans (
    id      INTEGER PRIMARY KEY,
    name    TEXT NOT NULL,
    ip      TEXT NOT NULL DEFAULT '',
    admin   TEXT NOT NULL DEFAULT '',
    reason  TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_bans_name ON bans(name COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_bans_ip   ON bans(ip);

CREATE TABLE IF NOT EXISTS admins (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS games (
    id      INTEGER PRIMARY KEY,
    name    TEXT NOT NULL,
    map     TEXT NOT NULL,
    started INTEGER NOT NULL,
    ended   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS game_players (
    game_id INTEGER NOT NULL REFERENCES games(id),
    name    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_game_players_game ON game_players(game_id);
```

- [ ] **Step 1: Написать падающие тесты**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_ban_survives_a_round_trip_and_is_case_insensitive() {
        let (store, _join) = Store::open_in_memory().unwrap();
        store.ban("Slash", "1.2.3.4", "admin", "flaming");
        assert!(store.is_banned("slash", "9.9.9.9").await.is_some());
        assert!(store.is_banned("Someone", "1.2.3.4").await.is_some());
        assert!(store.is_banned("Nobody", "9.9.9.9").await.is_none());
    }

    #[tokio::test]
    async fn removing_a_ban_clears_it() {
        let (store, _join) = Store::open_in_memory().unwrap();
        store.ban("Slash", "", "admin", "test");
        store.unban("Slash");
        assert!(store.is_banned("Slash", "").await.is_none());
    }

    #[tokio::test]
    async fn wal_mode_is_enabled_on_a_file_database() {
        let path = std::env::temp_dir().join("ghostrs-store-test.db");
        let _ = std::fs::remove_file(&path);
        let (store, _join) = Store::open(&path).unwrap();
        assert_eq!(store.journal_mode().await, "wal");
    }

    #[tokio::test]
    async fn a_logged_game_records_its_players() {
        let (store, _join) = Store::open_in_memory().unwrap();
        store.log_game("g1", "dota.w3x", 100, 200, vec!["a".into(), "b".into()]);
        assert_eq!(store.game_player_count("g1").await, 2);
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghost-store`
Expected: FAIL — крейта нет.

- [ ] **Step 3: Реализовать**

`Store::open` открывает `Connection`, применяет схему, затем `tokio::task::spawn_blocking` с циклом `while let Some(cmd) = rx.blocking_recv()`. Запросы отвечают через `oneshot`. Команды записи — fire-and-forget через `try_send`; при переполнении очереди логировать `warn!` и **не блокировать** вызывающего.

- [ ] **Step 4: Запустить тесты**

Run: `cargo test -p ghost-store && cargo clippy -p ghost-store -- -D warnings`
Expected: PASS, 4 теста.

- [ ] **Step 5: Commit**

```bash
git add crates/ghost-store
git commit -m "feat(store): add WAL SQLite store with a non-blocking writer task"
```

---

## Task 18: Бинарь — конфиг и супервизор

Заменяет `src/ghost.rs` и `src/config.rs`. Супервизор владеет BNET-акторами и играми, маршрутизирует между ними события и не держит ни одного глобального состояния.

**Files:**
- Create: `crates/ghostrs/src/config.rs`, `src/supervisor.rs`
- Modify: `crates/ghostrs/src/main.rs`, `crates/ghostrs/Cargo.toml`

**Interfaces:**
- Produces:
  - `struct Config { pub bot: BotConfig, pub bnet: BnetConfig, pub game: GameDefaults, pub spectator: SpectatorConfig, pub db_path: PathBuf }`
  - `Config::load(path: &Path) -> anyhow::Result<Config>` — парсит формат GHost++ `key = value` из `default.cfg`, с типизацией и дефолтами
  - `struct Supervisor` + `Supervisor::run(cfg: Config) -> anyhow::Result<()>`

- [ ] **Step 1: Написать падающие тесты конфига**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
bot_war3path = C:\\war3\\
bot_mappath = maps/
bot_maxgames = 20
bot_tft = 1
bnet_server = wc3.theabyss.ru
bnet_username = BOT
bnet_commandtrigger = !
# a comment
bnet_rootadmin = slash admin2
";

    #[test]
    fn parses_types_and_lists() {
        let c = Config::parse(SAMPLE).unwrap();
        assert_eq!(c.bot.max_games, 20);
        assert!(c.bot.tft);
        assert_eq!(c.bnet.server, "wc3.theabyss.ru");
        assert_eq!(c.bnet.command_trigger, '!');
        assert_eq!(c.bnet.root_admins, vec!["slash", "admin2"]);
    }

    #[test]
    fn comments_and_blank_lines_are_ignored() {
        let c = Config::parse("# nothing\n\n  \nbot_maxgames = 3\n").unwrap();
        assert_eq!(c.bot.max_games, 3);
    }

    #[test]
    fn missing_keys_fall_back_to_documented_defaults() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.game.latency.as_millis(), 100);
        assert_eq!(c.game.sync_limit, 50);
    }

    #[test]
    fn an_unparseable_number_is_an_error_not_a_silent_zero() {
        assert!(Config::parse("bot_maxgames = twenty\n").is_err());
    }
}
```

- [ ] **Step 2: Запустить, убедиться что падает**

Run: `cargo test -p ghostrs config`
Expected: FAIL.

- [ ] **Step 3: Реализовать конфиг**

Парсер: разбить по строкам, отбросить пустые и начинающиеся с `#`, разрезать по первому `=`, обрезать пробелы, собрать `HashMap<String, String>`. Затем типизированные геттеры с явными ошибками (`anyhow::bail!` с именем ключа), никаких молчаливых нулей. Дефолты — из легаси `src/ghost.rs:150-200`.

- [ ] **Step 4: Реализовать супервизор**

```rust
/// Owns every actor. Holds no game state itself: it only routes.
pub struct Supervisor {
    cfg: Config,
    store: Store,
    bnet: BnetHandle,
    bnet_events: mpsc::Receiver<BnetEvent>,
    /// The lobby currently advertised on Battle.net, if any.
    current: Option<GameHandle>,
    /// Games that already started; they run until they end on their own.
    running: Vec<(String, GameHandle)>,
    conns: mpsc::Receiver<(u64, TcpStream, SocketAddr)>,
    conn_events: mpsc::Sender<ConnEvent>,
}
```

Цикл `select!` над: новыми соединениями (создать `spawn_conn`, отдать `GameCmd::NewConn` текущему лобби), `ConnEvent` (маршрутизировать в игру по `conn_id`), `BnetEvent` (чат-команды root-админов → `!priv`/`!pub` создают игру через `spawn_game`, `!unhost` шлёт `GameCmd::Unhost`), таймером `SIGINT` (корректное завершение: всем играм `Shutdown`, дождаться `JoinHandle`).

Ограничение `bot_maxgames`: перед `spawn_game` проверять `running.len() < cfg.bot.max_games`.

- [ ] **Step 5: Проверить запуск end-to-end**

Run: `cargo run -p ghostrs`
Expected: логи `ghostrs starting`, подключение к PvPGN, вход в канал, ожидание команд. Ctrl+C завершает процесс без паник.

- [ ] **Step 6: Commit**

```bash
git add crates/ghostrs
git commit -m "feat(bin): add typed config and actor supervisor"
```

---

## Task 19: Нагрузочный стенд, бенчмарки и проверка KPI

Здесь доказываются цифры, ради которых всё затевалось. Без измерений «максимум перформанса» — просто заявление.

**Files:**
- Create: `crates/ghost-loadtest/Cargo.toml`, `src/main.rs`
- Create: `crates/ghost-protocol/benches/codec.rs`
- Create: `crates/ghost-engine/benches/tick.rs`
- Create: `docs/PERFORMANCE.md`

**Interfaces:**
- Produces: бинарь `ghost-loadtest --games N --players-per-game M --duration SECS --addr HOST:PORT`, печатающий гистограмму джиттера тика и потребление CPU/RSS.

**KPI (цель v1, 50 игр × 10 игроков на 8-ядерной машине):**

| Метрика | Цель | Как меряется |
|---|---|---|
| Джиттер тика p99 | < 2 мс при latency 100 мс | разница между `deadline()` и фактическим `Instant::now()` в начале тика |
| Пропущенные тики | 0 за 10 минут | счётчик `skipped > 0` из `actor.rs` |
| Кодирование одного тика (10 экшенов) | < 2 мкс | criterion, `incoming_action` |
| Рассылка тика на 10 игроков | < 5 мкс | criterion, `GameState::broadcast` |
| RSS | < 200 МБ на 500 игроков | `/proc/self/status` в конце прогона |

- [ ] **Step 1: Добавить измерение джиттера в актор**

В `actor.rs`, в ветке таймера, до `advance`:

```rust
let now = Instant::now();
let jitter = now.saturating_duration_since(deadline_before);
state.record_jitter(jitter);
```

`GameState::record_jitter` складывает значения в гистограмму из 5 корзин (`<1ms`, `<2ms`, `<5ms`, `<20ms`, `>=20ms`) и раз в 60 секунд пишет `tracing::info!` со сводкой. Никаких аллокаций на тик.

- [ ] **Step 2: Написать бенчмарк кодека**

`crates/ghost-protocol/benches/codec.rs`: criterion-группа из трёх кейсов — `incoming_action` с 0 / 10 / 100 экшенами; `W3gsCodec::decode` на буфере из 1000 склеенных фреймов.

- [ ] **Step 3: Запустить бенчмарки, записать базовую линию**

Run: `cargo bench -p ghost-protocol`
Expected: результаты сохранены; числа занести в `docs/PERFORMANCE.md` как базовую линию.

- [ ] **Step 4: Написать нагрузочный стенд**

`crates/ghost-loadtest/src/main.rs`: на каждого синтетического игрока — таска, которая подключается по TCP, шлёт `REQ_JOIN`, затем на каждый полученный `INCOMING_ACTION` отвечает `OUTGOING_KEEPALIVE` (это и есть то, что двигает `sync_counter`), плюс раз в секунду шлёт `OUTGOING_ACTION` со случайными 20 байтами. Клиент измеряет интервалы между приходящими тиками и печатает p50/p99/max.

- [ ] **Step 5: Прогнать нагрузку**

Run: `cargo run --release -p ghostrs & cargo run --release -p ghost-loadtest -- --games 50 --players-per-game 10 --duration 600`
Expected: p99 интервала между тиками ≤ 102 мс, max < 120 мс, 0 пропущенных тиков, 0 отвалившихся клиентов.

- [ ] **Step 6: Записать результаты**

Заполнить `docs/PERFORMANCE.md`: конфигурация железа, команда запуска, таблица KPI «цель / факт», вывод бенчмарков. Если KPI не достигнут — не подгонять цель, а завести отдельную задачу на профилирование (`tokio-console`, `perf`) и записать это в документ.

- [ ] **Step 7: Commit**

```bash
git add crates/ghost-loadtest crates/ghost-protocol/benches crates/ghost-engine/benches docs/PERFORMANCE.md
git commit -m "perf: add load-test harness, codec benchmarks and measured KPI baseline"
```

---

## Task 20: Удаление легаси

Последний шаг. До него легаси-крейт собирался всё время — теперь он больше не нужен ни как рабочий бот, ни как справочник по протоколу.

**Files:**
- Delete: `src/game_base.rs`, `src/game.rs`, `src/ghost.rs`, `src/gameplayer.rs`, `src/gameprotocol.rs`, `src/bnet.rs`, `src/bnetprotocol.rs`, `src/bncsutil.rs`, `src/bncsutilinterface.rs`, `src/socket.rs`, `src/gameslot.rs`, `src/logger.rs`, `src/config.rs`, `src/lang.rs`, `src/crc32.rs`, `src/sha1.rs`, `src/util.rs`, `src/gpsprotocol.rs`, `src/commandpacket.rs`, `src/main.rs`, `src/engine/`, `src/protocol/`, `src/stats/`, `src/storage/`, `src/spectator/`, `src/spectator_relay.rs`, `src/ghostdb.rs`
- Move: `src/map.rs`, `src/packed.rs`, `src/savegame.rs`, `src/stats_dota.rs`, `src/stats_w3mmd.rs` → `crates/ghost-engine/src/map.rs` и `crates/ghost-legacy-attic/` соответственно
- Modify: `Cargo.toml` (убрать корневой `[package]`, оставить только `[workspace]`)

- [ ] **Step 1: Убедиться, что паритет достигнут**

Пройти чек-лист вручную на живом PvPGN: бот логинится и заходит в канал; `!pub`/`!priv` создают игру, она видна в списке; игрок заходит, видит слоты и чат; `!start` запускает игру; игра доигрывается до конца; GProxy-клиент переживает разрыв сети на 30 секунд; зритель подключается к DotaTV-порту и видит игру с задержкой. Каждый пункт — отметка в теле коммита.

- [ ] **Step 2: Перенести парсер карты**

`src/map.rs` и `src/packed.rs` содержат разбор `.w3x`/MPQ и вычисление `map_crc`/`map_sha1` — это нужно движку. Перенести в `crates/ghost-engine/src/map.rs`, переименовав поля из `m_MapPath` в `path` и т.д. Тест: на реальном файле карты из `bot_mappath` полученные `crc` и `sha1` совпадают с теми, что печатал легаси-бот на том же файле (записать эталон до удаления).

- [ ] **Step 3: Переместить нереализованный функционал в чердак**

`stats_dota.rs`, `stats_w3mmd.rs`, `savegame.rs` — вне скоупа v1, но код рабочий. Создать `crates/ghost-legacy-attic/` (не в `members`, `publish = false`) и положить их туда с `README.md`, объясняющим, что это заготовки для v2, не собираемые в составе workspace.

- [ ] **Step 4: Удалить остальное**

```bash
git rm -r src
```

Затем убрать из корневого `Cargo.toml` секции `[package]`, `[[bin]]` и `[dependencies]`, оставив только `[workspace]`, `[workspace.package]` и `[workspace.dependencies]`.

- [ ] **Step 5: Проверить, что всё собирается и проходит**

Run: `cargo check --workspace && cargo test --workspace && cargo clippy --workspace -- -D warnings`
Expected: PASS. Ноль предупреждений (для сравнения: легаси-крейт давал 496).

- [ ] **Step 6: Обновить CI**

Убрать `--exclude ghostrs-legacy` из шага тестов и расширить clippy на весь workspace.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore: remove the legacy GHost++ transliteration

Parity verified on a live PvPGN server: login, hosting, join, start,
full game, GProxy reconnect and DotaTV spectating all confirmed."
```

---

## Порядок выполнения и зависимости

```
1 ─> 2 ─> 3 ─> 4 ─┐
          └> 6    ├─> 5 ─┐
                  │      ├─> 10 ─> 11 ─> 12 ─> 13 ─> 15 ─┐
7 ─────────────────┘      │                              │
8 ─> 9 ────────────────────┘                              ├─> 18 ─> 19 ─> 20
14 ───────────────────────────────────────────────────────┤
16 ───────────────────────────────────────────────────────┤
17 ───────────────────────────────────────────────────────┘
```

Задачи 14, 16 и 17 не зависят друг от друга и от цепочки движка после Task 11 — их можно делать параллельно. Задачи 1–13 строго последовательны.

## Проверка полноты (self-review)

Сверено с запросом:

- «полностью переписать» — Task 20 удаляет весь легаси-код; ни один модуль `src/` не переживает план.
- «максимум перформанса» — event-driven цикл вместо поллинга (Task 10), тик без дрейфа (Task 8), однократное кодирование пакета и рассылка по refcount (Task 11), запись в сокеты вне тика (Task 7), измеряемые KPI (Task 19).
- «доделать то что я сделал» — `protocol/`, `engine/`, `spectator_relay.rs`, `ghostdb.rs` не выбрасываются, а переезжают в крейты (таблица в разделе «Структура файлов»), включая три исправленных бага.
- Скоуп v1 из ответов: ядро хостинга (10–13), GProxy++ (15), DotaTV (16). Статистика DotA/W3MMD осознанно отложена — Task 20, Step 3 сохраняет её код.

Известные допущения, требующие проверки при выполнении:

1. Раскладки байтов в таблицах задач 4 и 5 выведены из сигнатур `src/gameprotocol.rs`. Шаги «сверить с легаси» в обеих задачах обязательны — при расхождении легаси главнее.
2. Task 14 предполагает, что криптографию логина можно посчитать на `sha1` без `libloading`/`bncsutil`. Если в `src/bncsutil.rs` окажется вызов внешней библиотеки с неизвестным алгоритмом — сохранить `libloading` и вызывать её как раньше.
3. Формат `.w3g` (Task 16, Step 4) взят по описанию заголовка; проверять открытием готового реплея в World Editor.
