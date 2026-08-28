# Full Rejoin (Dota-2-style) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Позволить игроку, полностью потерявшему клиент (краш / ребут / закрыл war3.exe), перезапустить war3 с нашим .dll, переподключиться к идущей игре как обычный мультиплеер-клиент, получить всю историю тёрнов 0..N, промотать её за лоадинг-скрином (патчи скорости game.dll) и продолжить играть вживую.

**Architecture:** Две части. **Сервер (Rust, этот репозиторий, spectre-engine):** глобальный неограниченный лог эфира `FullHistory`, реактивный state-machine «join-in-progress» поверх существующих обработчиков REQJOIN/MAPSIZE/GAMELOADED_SELF, GPS-фрейм токена `FULL`, backpressure-совместимая подача истории курсором внутри актора, подавление ложного десинка во время catch-up. Ничего в супервизоре менять не нужно — соединение переподключается на исходный `host_port` игры и роутится существующим `port_to_game`. **Клиент (C++ .dll, отдельный проект):** три верифицированных байт-патча game.dll (лимитер + антифриз), доступ к `CNetSession` и управление `SPEED_NUM/DEN`, накачка message-pump, файл-токен, GProxy-подобный локальный шим, который заставляет war3 инициировать join в начатую игру.

**Tech Stack:** Rust (workspace edition 2024, `#![forbid(unsafe_code)]` в spectre-engine — весь серверный код безопасный), tokio, `bytes::Bytes`, `crc32fast` (переиспользуется, не изобретается). Клиент: C++ / WinAPi (`VirtualProtect`, `FlushInstructionCache`), MSVC x86 (game.dll 32-битная).

---

## Global Constraints

- **game.dll = 1.26a, imagebase `0x6F000000`.** Все адреса/байты ниже верифицированы в этой сессии через ida-pro-mcp (`get_bytes`/`disasm`, session на `D:\rewar3\game.dll.i64`). Патчить только по сигнатуре старых байт; несовпадение → abort + лог.
- **spectre-engine `#![forbid(unsafe_code)]`** — серверная часть без `unsafe`.
- **CRC пакетов НЕ изобретать.** In-game экшены уже собираются `spectre_protocol::w3gs::outgoing::incoming_action`/`incoming_action2` с `crc32fast` и уходят живым игрокам, которые их принимают. `FullHistory` хранит те же самые `Bytes` — CRC валидна by-construction (закрывает чек-лист C.4 без декода клиентского CRC).
- **RandomSeed идентичность:** `GameState.random_seed` создаётся один раз в `new()` (`rand::random()`), в SLOTINFO/SLOTINFOJOIN уходит как есть. FULL-rejoin переиспользует тот же `random_seed`. Никогда не регенерировать при переджойне (тест A9-seed).
- **Не трогать `+0x610` (индекс слота локального игрока) и `+0x614` (флаг реплея) в game.dll.** Резолюция чек-листов ниже.
- Порядок пакетов истории = порядок эфира: `broadcast()` — единственная точка записи.

---

## Верифицированная фактура из ida-pro-mcp (опираться без перепроверки)

Таблица патчей — байт-в-байт подтверждена `get_bytes` в этой сессии:

| # | VA | Инструкция | Старые байты | Новые байты | Смысл |
|---|----|-----------|--------------|-------------|-------|
| P1 | `0x6F553622` | imm у `cmp eax,0FA0h` | `A0 0F 00 00` | `20 4E 00 00` | cap 4000→20000 ms (160→800 тёрнов/кадр) |
| P2 | `0x6F553629` | imm у `mov eax,0FA0h` | `A0 0F 00 00` | `20 4E 00 00` | кламп 4000→20000 ms |
| P3 | `0x6F5537E5` | `mov [esi+2250h],edi` | `89 BE 50 22 00 00` | `90 90 90 90 90 90` | NOP сброса acc (антифриз) |
| P3' (альт.) | `0x6F5537DF` | imm у `cmp eax,0C8h` | `C8 00 00 00` | `FF FF FF FF` | порог антифриза 200ms→∞ |

**ЗАПРЕЩЕНО:** NOP-ать `jb @0x6F553626` и `jbe @0x6F5537E3`; ставить cap в бесконечность; писать `+0x610`/`+0x614`.

Поля `CNetSession` (esi), тик-цикл `sub_6F553470`:
- `+0x2250` acc(ms); `+0x284` долг тёрнов; тик `= 0x19 = 25ms`.
- `+0x22B4` SPEED_NUM; `+0x22B8` SPEED_DEN; `+0x22BC` gate (== 0, чтобы копить). Накопление: `acc += SPEED_NUM*elapsed/SPEED_DEN` @ `0x6F553604`.
- **Резолюция C.2:** `ebx = (SPEED_NUM>1 ? 1 : 0)` (`mov eax,1; cmp eax,[esi+22B4h]; sbb ebx,ebx; neg ebx` @ `0x6F553639`). Антифриз (`0x6F5537D1: cmp ebx,edi; jz skip`) активен **только при SPEED_NUM>1**. Значит P3 обязателен во время промотки и безопасен как постоянный (при SPEED_NUM==1 ветка антифриза не исполняется).
- **Резолюция C.3:** состояния слота `+0x278 == 4 (Loading) / 5 (Playing)` идут в `loc_6F5536FD` (нормальная обработка тёрна) **без проверки `+0x610`/`+0x614`**. Ветка state==6 (пауза, требует `+0x610==1`) — не на нашем пути. Catch-up = детерминированный lockstep в живом режиме; авантюра с state-6 не нужна.
- **Резолюция C.1:** `+0x610` = **индекс слота локального игрока** (доказательство: `mov eax,[esi+610h]; imul eax,304h; cmp [eax+esi+278h],4` @ `0x6F5534AA`, и `a2 == v5[388]` в `sub_6F54D970`, где 388*4=0x610). Это НЕ флаг реплея (тот — `+0x614`, `cmp [esi+614h],2` @ `0x6F5534A1`). При обычном мультиплеер-джойне war3 сам проставит `+0x610`. Мы его не трогаем.
- **CNetSession из хука** (пролог `sub_6F53F160`, верифицирован): `mov ecx,0Dh; call sub_6F4C34D0; mov eax,[eax+10h]; mov ecx,[eax+8]` → `ecx = CNetSession*`. То есть `sess = *(void**)(*(void**)(getctx(13)+0x10)+0x08)`.
- **Резолюция C.5 (оверлей):** `dword_6FAB65F4` — живой глобал (CGameUI-подобный, используется в `sub_6F54D970` при слоте 0). Оверлей лоадинг-скрина — опциональная косметика, НЕ на критическом пути приёмки G.

---

# ЧАСТЬ A — Сервер (spectre-engine). Полностью реализуемо и тестируемо сейчас.

## File Structure (Part A)

- Create `crates/spectre-engine/src/full_history.rs` — глобальный лог эфира `FullHistory`.
- Create `crates/spectre-engine/src/full_rejoin.rs` — детект и handshake FULL-переджойна, подача истории.
- Modify `crates/spectre-engine/src/state.rs` — поле `full_history`, `pending_full`, запись в `broadcast()`, скип живой отправки переджойнеру.
- Modify `crates/spectre-engine/src/players.rs` — поля `rejoin: RejoinStage`, `catchup_cursor: Option<u32>`, `catching_up: bool` + enum `RejoinStage`.
- Modify `crates/spectre-engine/src/mapxfer.rs` — ветка rejoin в `handle_map_size`.
- Modify `crates/spectre-engine/src/actions.rs` — ветка rejoin в `handle_loaded`, вызов помпы catch-up в `on_tick`, скип catching-up игрока в `check_desync`, снятие флага catch-up в `handle_keepalive`.
- Modify `crates/spectre-engine/src/actor.rs` — арм `FULL` в `on_gps_frame`, очистка `pending_full` при закрытии conn.
- Modify `crates/spectre-engine/src/lobby.rs` — ветка FULL-rejoin в начале not-Lobby отказа.
- Modify `crates/spectre-engine/src/lib.rs` — `pub mod full_history; pub mod full_rejoin;` + ре-экспорты.
- Modify `crates/spectre-protocol/src/gps/mod.rs` — `ids::FULL = 0x05`, `full(pid,key)`, `decode_full`.

---

### Task A1: `FullHistory` — глобальный лог эфира

**Files:**
- Create: `crates/spectre-engine/src/full_history.rs`
- Modify: `crates/spectre-engine/src/lib.rs`

**Interfaces:**
- Produces: `FullHistory::new() -> FullHistory`, `new_with_cap(cap: usize)`, `push(&mut self, Bytes)`, `snapshot_from(&self, start: u32) -> Vec<Bytes>`, `len(&self) -> u32`, `is_empty(&self) -> bool`, `bytes_estimate(&self) -> usize`.

- [ ] **Step 1: Write the failing test** — создать файл с телом типа и тестами:

```rust
use bytes::Bytes;
use std::collections::VecDeque;

/// Неограниченный (с потолком) лог всех in-game broadcast-пакетов игры,
/// байт-в-байт совпадающий с живым эфиром. Отдаётся FULL-переджойнеру целиком.
pub struct FullHistory {
    inner: VecDeque<Bytes>,
    cap: usize,
}

impl FullHistory {
    /// Потолок 216_000 пакетов ≈ 90 минут при ~40 пакетах/сек, ~10-32 МБ RAM.
    pub fn new() -> Self {
        Self::new_with_cap(216_000)
    }

    pub fn new_with_cap(cap: usize) -> Self {
        Self { inner: VecDeque::with_capacity(4096), cap }
    }

    pub fn push(&mut self, pkt: Bytes) {
        if self.inner.len() >= self.cap {
            self.inner.pop_front();
        }
        self.inner.push_back(pkt);
    }

    /// Все пакеты, начиная с индекса `start` (0 = вся история). `start` за пределом → пусто.
    pub fn snapshot_from(&self, start: u32) -> Vec<Bytes> {
        self.inner.iter().skip(start as usize).cloned().collect()
    }

    pub fn len(&self) -> u32 {
        self.inner.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn bytes_estimate(&self) -> usize {
        self.inner.iter().map(|b| b.len()).sum()
    }
}

impl Default for FullHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_increments_len() {
        let mut h = FullHistory::new();
        h.push(Bytes::from_static(b"a"));
        h.push(Bytes::from_static(b"b"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn snapshot_from_zero_returns_all_in_order() {
        let mut h = FullHistory::new();
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        let s = h.snapshot_from(0);
        assert_eq!(s, vec![Bytes::from_static(b"1"), Bytes::from_static(b"2")]);
    }

    #[test]
    fn snapshot_from_cursor_skips_prefix() {
        let mut h = FullHistory::new();
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        h.push(Bytes::from_static(b"3"));
        assert_eq!(h.snapshot_from(2), vec![Bytes::from_static(b"3")]);
        assert_eq!(h.snapshot_from(3), Vec::<Bytes>::new());
    }

    #[test]
    fn cap_evicts_oldest() {
        let mut h = FullHistory::new_with_cap(2);
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        h.push(Bytes::from_static(b"3"));
        assert_eq!(h.len(), 2);
        assert_eq!(h.snapshot_from(0)[0], Bytes::from_static(b"2"));
    }
}
```

Затем добавить в `crates/spectre-engine/src/lib.rs` после строки `pub mod chat;` (сохраняя алфавит — вставить в блок `pub mod`):

```rust
pub mod full_history;
pub mod full_rejoin;
```

и в блок ре-экспортов:

```rust
pub use full_history::FullHistory;
```

> Примечание: `full_rejoin` модуль появится в Task A6; чтобы крейт компилировался между тасками, создайте пустой файл `crates/spectre-engine/src/full_rejoin.rs` уже сейчас (одна строка-заглушка `//! FULL rejoin — заполняется в Task A6`). Это единственное исключение из «no placeholder» — пустой модуль нужен для сборки.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-engine --lib full_history`
Expected: FAIL — модуль `full_history` не существует / не подключён.

- [ ] **Step 3: Implement** — код уже приведён в Step 1 (файл + правки lib.rs + пустой full_rejoin.rs).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p spectre-engine --lib full_history`
Expected: PASS (4 теста).

- [ ] **Step 5: Commit**

```bash
git add crates/spectre-engine/src/full_history.rs crates/spectre-engine/src/full_rejoin.rs crates/spectre-engine/src/lib.rs
git commit -m "feat(engine): add FullHistory ring-log for full rejoin"
```

---

### Task A2: Запись эфира в `FullHistory` из `broadcast()` (только in-game)

**Files:**
- Modify: `crates/spectre-engine/src/state.rs` (поле в `GameState` ~line 185; init в `new()` ~line 267; тело `broadcast()` 330-363)
- Test: `crates/spectre-engine/src/state.rs` (`#[cfg(test)]` в конце файла — добавить модуль)

**Interfaces:**
- Consumes: `FullHistory` (Task A1).
- Produces: `GameState.full_history: FullHistory`. Инвариант: `broadcast()` пишет пакет в `full_history` **тогда и только тогда**, когда `phase == GamePhase::Playing`.

- [ ] **Step 1: Write the failing test** — добавить в конец `state.rs`:

```rust
#[cfg(test)]
mod full_history_recording_tests {
    use super::*;
    use crate::actor::tests_support::seated_game;

    #[test]
    fn lobby_broadcasts_are_not_recorded() {
        let (mut st, _rxs) = seated_game(1);
        st.broadcast(Bytes::from_static(&[0xF7, 0x0F, 0x04, 0x00]));
        assert_eq!(st.full_history.len(), 0, "lobby packets must not enter FullHistory");
    }

    #[test]
    fn playing_broadcasts_are_recorded_byte_identical() {
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();
        let pkt = Bytes::from_static(&[0xF7, 0x0C, 0x06, 0x00, 0x64, 0x00]);
        st.broadcast(pkt.clone());
        assert_eq!(st.full_history.len(), 1);
        assert_eq!(st.full_history.snapshot_from(0)[0], pkt);
    }

    #[test]
    fn history_survives_gproxy_buffer_eviction() {
        let (mut st, _rxs) = seated_game(1);
        st.begin_playing();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.players.by_pid_mut(1).unwrap().gproxy_buffer =
            Some(crate::gproxy::GProxyBuffer::new(500));
        for i in 0..600u32 {
            st.broadcast(Bytes::from(i.to_le_bytes().to_vec()));
        }
        // per-player GProxyBuffer(500) вытеснил префикс, но глобальный лог держит всё
        assert_eq!(st.full_history.len(), 600);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-engine --lib full_history_recording`
Expected: FAIL — поля `full_history` нет.

- [ ] **Step 3: Implement**

В `struct GameState` добавить поле (рядом с `relay`/`replay`, напр. после `pub replay: ...` строки; порядок полей не важен):

```rust
    pub full_history: crate::full_history::FullHistory,
    pub pending_full: std::collections::HashMap<u64, (u8, u32)>,
```

> `pending_full` понадобится в Task A5, но добавляем сразу, чтобы `new()` инициализировать один раз.

В `GameState::new()` добавить в конструктор `Self { ... }` (в любом месте до `cfg,`):

```rust
            full_history: crate::full_history::FullHistory::new(),
            pending_full: std::collections::HashMap::new(),
```

В `broadcast()` — **первой строкой** тела, до цикла по игрокам:

```rust
    pub fn broadcast(&mut self, bytes: Bytes) {
        if matches!(self.phase, GamePhase::Playing) {
            self.full_history.push(bytes.clone());
        }
        for p in self.players.iter_mut() {
            // ... существующий код без изменений ...
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p spectre-engine --lib full_history_recording`
Expected: PASS (3 теста).

- [ ] **Step 5: Run full engine suite (regression)**

Run: `cargo test -p spectre-engine --lib`
Expected: PASS — существующие тесты не сломаны (broadcast сигнатура не менялась).

- [ ] **Step 6: Commit**

```bash
git add crates/spectre-engine/src/state.rs
git commit -m "feat(engine): record in-game broadcast stream into FullHistory"
```

---

### Task A3: Поля переджойна на `Player` + `RejoinStage`

**Files:**
- Modify: `crates/spectre-engine/src/players.rs` (enum перед `struct Player`; поля в `struct Player` 17-53; дефолты в `Player::new()` 56-94)

**Interfaces:**
- Produces: `pub enum RejoinStage { None, AwaitingMapSize, AwaitingLoaded }` (Copy, PartialEq). Поля `Player.rejoin: RejoinStage` (default `None`), `Player.catchup_cursor: Option<u32>` (default `None`), `Player.catching_up: bool` (default `false`).

- [ ] **Step 1: Write the failing test** — добавить в `#[cfg(test)] mod tests` в `players.rs`:

```rust
    #[test]
    fn new_player_has_no_rejoin_state() {
        let p = test_player(1, "a");
        assert_eq!(p.rejoin, RejoinStage::None);
        assert_eq!(p.catchup_cursor, None);
        assert!(!p.catching_up);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-engine --lib new_player_has_no_rejoin_state`
Expected: FAIL — `RejoinStage` не существует.

- [ ] **Step 3: Implement**

Перед `#[derive(Debug)] pub struct Player {` добавить:

```rust
/// Стадия «join-in-progress» переджойнящегося игрока. Управляет реактивным
/// handshake поверх обычных обработчиков REQJOIN → MAPSIZE → GAMELOADED_SELF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejoinStage {
    None,
    AwaitingMapSize,
    AwaitingLoaded,
}
```

В `struct Player` добавить поля (в конец списка, после `gproxy_disconnect_notice_sent: bool,`):

```rust
    pub rejoin: RejoinStage,
    pub catchup_cursor: Option<u32>,
    pub catching_up: bool,
```

В `Player::new()` в `Self { ... }` добавить дефолты (в конец, до закрывающей `}`):

```rust
            rejoin: RejoinStage::None,
            catchup_cursor: None,
            catching_up: false,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p spectre-engine --lib new_player_has_no_rejoin_state`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spectre-engine/src/players.rs
git commit -m "feat(engine): add rejoin/catch-up state fields to Player"
```

---

### Task A4: GPS-фрейм токена `FULL` (протокол)

**Files:**
- Modify: `crates/spectre-protocol/src/gps/mod.rs` (`mod ids` 10-15; функции; тесты в конце файла)

**Interfaces:**
- Produces: `gps::ids::FULL: u8 = 0x05`; `gps::full(pid: u8, reconnect_key: u32) -> Bytes`; `gps::decode_full(&Bytes) -> Result<(u8, u32), ProtoError>`.

- [ ] **Step 1: Write the failing test** — добавить в `#[cfg(test)] mod tests` в `gps/mod.rs`:

```rust
    #[test]
    fn full_token_roundtrip() {
        let b = full(7, 0xDEAD_BEEF);
        assert_eq!(b[0], GPS_HEADER);
        assert_eq!(b[1], ids::FULL);
        assert_eq!(u16::from_le_bytes([b[2], b[3]]) as usize, b.len());
        let (pid, key) = decode_full(&b.slice(4..)).unwrap();
        assert_eq!(pid, 7);
        assert_eq!(key, 0xDEAD_BEEF);
    }

    #[test]
    fn decode_full_rejects_truncated() {
        assert!(decode_full(&Bytes::from_static(&[7, 0, 0])).is_err());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-protocol --lib full_token`
Expected: FAIL — `full` / `ids::FULL` не существуют.

- [ ] **Step 3: Implement**

В `pub mod ids { ... }` добавить:

```rust
    pub const FULL: u8 = 0x05;
```

После функции `reject(...)` добавить:

```rust
/// Токен полного переджойна: pid + reconnect_key. Клиентский .dll шлёт этот фрейм
/// на игровой host_port сразу после TCP-connect, ДО обычного W3GS REQJOIN.
pub fn full(pid: u8, reconnect_key: u32) -> Bytes {
    let mut p = BytesMut::with_capacity(5);
    p.put_u8(pid);
    p.put_u32_le(reconnect_key);
    Frame::new(ids::FULL, p.freeze())
        .encode_with(GPS_HEADER)
        .expect("5-byte gps full always fits")
}

pub fn decode_full(payload: &Bytes) -> Result<(u8, u32), ProtoError> {
    let mut b = payload.clone();
    let pid = b.try_get_u8()?;
    let key = b.try_get_u32_le()?;
    Ok((pid, key))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p spectre-protocol --lib full_token && cargo test -p spectre-protocol --lib decode_full`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spectre-protocol/src/gps/mod.rs
git commit -m "feat(proto): add GPS FULL token frame (0x05)"
```

---

### Task A5: Кэш токена `pending_full` + арм `FULL` в `on_gps_frame`

**Files:**
- Modify: `crates/spectre-engine/src/actor.rs` (`on_gps_frame` 189-232; `handle_conn_closed` вызывается из `handle_cmd` — очистку добавим в `lobby.rs::handle_conn_closed`)
- Modify: `crates/spectre-engine/src/lobby.rs` (`handle_conn_closed` 168-187 — очистка `pending_full`)

**Interfaces:**
- Consumes: `GameState.pending_full` (Task A2), `gps::decode_full` (Task A4).
- Produces: после GPS-фрейма `FULL` от conn X, `pending_full[X] == (pid, key)`. При закрытии conn X — запись удаляется.

- [ ] **Step 1: Write the failing test** — добавить в `#[cfg(test)] mod tests` в `actor.rs`:

```rust
    #[tokio::test]
    async fn gps_full_frame_caches_the_token() {
        let (mut st, _rxs) = tests_support::seated_game(1);
        let conn_id = st.players.by_pid(1).unwrap().conn_id;
        let frame = spectre_protocol::frame::Frame::new(
            spectre_protocol::gps::ids::FULL,
            spectre_protocol::gps::full(9, 0x1234_5678).slice(4..),
        );
        st.on_gps_frame(conn_id, frame);
        assert_eq!(st.pending_full.get(&conn_id), Some(&(9u8, 0x1234_5678u32)));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-engine --lib gps_full_frame_caches`
Expected: FAIL — арм `FULL` отсутствует.

- [ ] **Step 3: Implement**

В `on_gps_frame`, в `match frame.id { ... }`, перед `_ => {}` добавить арм:

```rust
            spectre_protocol::gps::ids::FULL => {
                if let Ok((pid, key)) = spectre_protocol::gps::decode_full(&frame.payload) {
                    self.pending_full.insert(conn_id, (pid, key));
                }
            }
```

В `crates/spectre-engine/src/lobby.rs::handle_conn_closed`, первой строкой тела:

```rust
    pub fn handle_conn_closed(&mut self, conn_id: u64, reason: String) {
        self.pending_full.remove(&conn_id);
        // ... существующий код ...
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p spectre-engine --lib gps_full_frame_caches`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/spectre-engine/src/actor.rs crates/spectre-engine/src/lobby.rs
git commit -m "feat(engine): cache GPS FULL token by conn_id"
```

---

### Task A6: Детект и старт handshake — ветка FULL-rejoin в `handle_req_join`

**Files:**
- Create: `crates/spectre-engine/src/full_rejoin.rs` (заменить заглушку из A1)
- Modify: `crates/spectre-engine/src/lobby.rs` (`handle_req_join` — вставить ветку в блок `if !matches!(self.phase, GamePhase::Lobby)` 31-34)

**Interfaces:**
- Consumes: `RejoinStage` (A3), `pending_full` (A5), `outgoing::{slot_info_join, player_info, map_check, reject_join}`, `SlotTable::sid_of_pid`, `PlayerLink` (Clone).
- Produces: `GameState::try_full_rejoin(&mut self, conn_id: u64, req: &ReqJoin, external_ip: [u8;4], link: PlayerLink) -> bool`. Возвращает `true` если это валидный FULL-rejoin (обработан; вызывающий должен вернуться), `false` — не FULL-rejoin (нужен обычный REJECT_STARTED). При успехе: re-attach conn к существующему игроку, отправка SLOTINFOJOIN+PLAYERINFO(others)+MAPCHECK новому link, `player.rejoin = AwaitingMapSize`.

- [ ] **Step 1: Write the failing test** — записать `full_rejoin.rs` целиком (реализация + тесты):

```rust
//! FULL rejoin — переподключение игрока, полностью потерявшего клиент.
//!
//! Отличие от `handle_gps_reconnect` (gproxy.rs): тот — живой war3 с целым
//! `GProxyBuffer` (докидывает хвост). FULL — холодный рестарт war3: истории у
//! клиента нет, per-player буфер давно вытеснен, нужна ВСЯ история из
//! `FullHistory`. Клиент проходит обычный join-in-progress handshake, а сервер
//! реагирует на его штатные пакеты (REQJOIN → MAPSIZE → GAMELOADED_SELF).

use bytes::Bytes;
use spectre_net::PlayerLink;
use spectre_protocol::w3gs::incoming::ReqJoin;
use spectre_protocol::w3gs::outgoing;

use crate::players::RejoinStage;
use crate::state::{GamePhase, GameState};

impl GameState {
    /// Пытается обработать REQJOIN как FULL-rejoin. Предусловие вызова: phase != Lobby.
    /// Возвращает true, если переджойн валиден и обработан.
    pub fn try_full_rejoin(
        &mut self,
        conn_id: u64,
        req: &ReqJoin,
        _external_ip: [u8; 4],
        link: PlayerLink,
    ) -> bool {
        if !matches!(self.phase, GamePhase::Playing | GamePhase::Loading) {
            return false;
        }
        // Токен: pid+key из кэша GPS FULL по этому conn.
        let Some(&(token_pid, token_key)) = self.pending_full.get(&conn_id) else {
            return false;
        };
        // Место должно быть удержано (gproxy-grace, ещё не reaped).
        let Some(p) = self.players.by_pid(token_pid) else {
            return false;
        };
        let held = p.disconnected_since.is_some() && p.left.is_none();
        let name_ok = p.name.eq_ignore_ascii_case(&req.name);
        let key_ok = p.reconnect_key == token_key;
        if !held || !name_ok || !key_ok {
            return false;
        }

        // Re-attach: как handle_gps_reconnect, но без replay per-player буфера.
        let pid = token_pid;
        {
            let p = self.players.by_pid_mut(pid).unwrap();
            p.conn_id = conn_id;
            p.link = link;
            p.disconnected_since = None;
            p.left = None;
            p.consecutive_send_failures = 0;
            p.loaded = false;
            p.rejoin = RejoinStage::AwaitingMapSize;
        }
        self.pending_full.remove(&conn_id);

        // Отправляем ТОЛЬКО новому link (не broadcast): его личный join-flow.
        let listen_port = req.listen_port;
        let ext_ip = _external_ip;
        // a) SLOTINFOJOIN — оригинальный pid, текущий расклад слотов, тот же seed.
        if let Ok(b) = outgoing::slot_info_join(
            pid,
            listen_port,
            ext_ip,
            self.slots.as_wire(),
            self.random_seed,
            self.cfg.map.layout_style,
            self.cfg.map.num_players,
        ) {
            self.send_to(pid, b);
        }
        // b) PLAYERINFO про всех ОСТАЛЬНЫХ живых игроков.
        let others: Vec<(u8, String, [u8; 4], [u8; 4])> = self
            .players
            .iter()
            .filter(|q| q.pid != pid && !q.virtual_host && q.left.is_none())
            .map(|q| (q.pid, q.name.clone(), q.external_ip, q.internal_ip))
            .collect();
        for (opid, oname, oext, oint) in others {
            if let Ok(b) = outgoing::player_info(opid, &oname, oext, oint) {
                self.send_to(pid, b);
            }
        }
        // c) MAPCHECK — клиент ответит MAPSIZE (карта у него есть).
        if let Ok(b) = outgoing::map_check(
            &self.cfg.map.path,
            self.cfg.map.size,
            self.cfg.map.info,
            self.cfg.map.crc,
            self.cfg.map.sha1,
        ) {
            self.send_to(pid, b);
        }

        tracing::info!(game = %self.cfg.name, pid, name = %req.name, "FULL rejoin accepted, handshake started");
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::tests_support::{drain_ids, reqjoin_bytes, seated_game};
    use crate::players::RejoinStage;
    use spectre_protocol::w3gs::{ids, incoming::ReqJoin};

    /// Ставит игру в Playing, роняет игрока pid в held-состояние, кэширует валидный токен.
    fn playing_with_disconnected(name: &str) -> (GameState, u8, u32, tokio::sync::mpsc::Receiver<Bytes>) {
        let (mut st, _rxs) = seated_game(1);
        // переименуем P1 в нужное имя для наглядности
        st.players.by_pid_mut(1).unwrap().name = name.to_string();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.begin_playing();
        let key = st.players.by_pid(1).unwrap().reconnect_key;
        st.players.by_pid_mut(1).unwrap().disconnected_since = Some(std::time::Instant::now());
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        // новый conn с кэшированным токеном
        st.add_conn(99, PlayerLink::for_test(tx.clone()), [5, 6, 7, 8]);
        st.pending_full.insert(99, (1, key));
        (st, 1, key, rx)
    }

    #[test]
    fn valid_full_rejoin_sends_join_handshake_and_sets_stage() {
        let (mut st, _pid, _key, mut rx) = playing_with_disconnected("Slash");
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(256);
        let handled = st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2));
        assert!(handled);
        assert_eq!(st.players.by_pid(1).unwrap().rejoin, RejoinStage::AwaitingMapSize);
        assert_eq!(st.players.by_pid(1).unwrap().conn_id, 99);
        assert!(st.players.by_pid(1).unwrap().disconnected_since.is_none());
        let ids_sent = drain_ids(&mut rx);
        assert!(ids_sent.contains(&ids::SLOT_INFO_JOIN), "got {ids_sent:?}");
        assert!(ids_sent.contains(&ids::MAP_CHECK));
    }

    #[test]
    fn wrong_key_is_not_full_rejoin() {
        let (mut st, _pid, _key, _rx) = playing_with_disconnected("Slash");
        st.pending_full.insert(99, (1, 0xBAD)); // подменяем ключ
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(64);
        assert!(!st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
        assert_eq!(st.players.by_pid(1).unwrap().rejoin, RejoinStage::None);
    }

    #[test]
    fn no_token_is_not_full_rejoin() {
        let (mut st, _pid, _key, _rx) = playing_with_disconnected("Slash");
        st.pending_full.remove(&99);
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(64);
        assert!(!st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
    }

    #[test]
    fn a_live_seat_is_not_rejoinable() {
        let (mut st, _pid, key, _rx) = playing_with_disconnected("Slash");
        st.players.by_pid_mut(1).unwrap().disconnected_since = None; // место занято живым
        st.pending_full.insert(99, (1, key));
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(64);
        assert!(!st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-engine --lib full_rejoin::tests`
Expected: FAIL — `try_full_rejoin` не подключён к `handle_req_join`, но unit-тесты модуля должны собраться и пройти после реализации; сначала упадут на компиляции (нет метода). После записи файла — тесты модуля проходят. Убедитесь, что падение именно ожидаемое (сборка), затем Step 3 — файл уже записан.

- [ ] **Step 3: Wire into `handle_req_join`**

В `crates/spectre-engine/src/lobby.rs`, заменить блок not-Lobby отказа:

```rust
        if !matches!(self.phase, GamePhase::Lobby) {
            if self.try_full_rejoin(conn_id, &req, external_ip, link) {
                return;
            }
            let _ = link.try_send(outgoing::reject_join(REJECT_STARTED));
            return;
        }
```

> Внимание к borrow: `try_full_rejoin` берёт `link` по значению (переиспользует внутри). Если вернул `false`, `link` уже перемещён — поэтому клонируйте перед вызовом: замените на:
>
> ```rust
>         if !matches!(self.phase, GamePhase::Lobby) {
>             if self.try_full_rejoin(conn_id, &req, external_ip, link.clone()) {
>                 return;
>             }
>             let _ = link.try_send(outgoing::reject_join(REJECT_STARTED));
>             return;
>         }
> ```
>
> `PlayerLink: Clone` (verified conn.rs:129).

- [ ] **Step 4: Run tests**

Run: `cargo test -p spectre-engine --lib full_rejoin`
Expected: PASS (4 теста модуля).

- [ ] **Step 5: Run regression**

Run: `cargo test -p spectre-engine --lib`
Expected: PASS — обычный REJECT_STARTED-путь для нетокенных джойнов не сломан (тестов на него нет, но существующие lobby-тесты в Lobby-фазе зелёные).

- [ ] **Step 6: Commit**

```bash
git add crates/spectre-engine/src/full_rejoin.rs crates/spectre-engine/src/lobby.rs
git commit -m "feat(engine): detect FULL rejoin and start join-in-progress handshake"
```

---

### Task A7: Реактивный handshake — `handle_map_size` → countdown, `handle_loaded` → старт catch-up

**Files:**
- Modify: `crates/spectre-engine/src/mapxfer.rs` (`handle_map_size` 31-87 — ветка rejoin в начале)
- Modify: `crates/spectre-engine/src/actions.rs` (`handle_loaded` 171-186 — ветка rejoin)

**Interfaces:**
- Consumes: `RejoinStage`, `outgoing::{countdown_start, countdown_end, game_loaded_others}`.
- Produces: при `rejoin == AwaitingMapSize` и корректной MAPSIZE → отправка COUNTDOWN_START+COUNTDOWN_END игроку, `rejoin = AwaitingLoaded`. При `rejoin == AwaitingLoaded` и GAMELOADED_SELF → отправка GAME_LOADED_OTHERS(others) игроку, `loaded=true`, `rejoin=None`, `catchup_cursor=Some(0)`; broadcast GAME_LOADED_OTHERS(pid) остальным; **не** вызывать `begin_playing`.

- [ ] **Step 1: Write the failing test** — добавить в `full_rejoin.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn map_size_during_awaiting_advances_to_countdown() {
        let (mut st, _pid, _key, mut rx) = playing_with_disconnected("Slash");
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(256);
        assert!(st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2)));
        let _ = drain_ids(&mut rx);

        // клиент рапортует, что карта у него целиком: MAPSIZE(size_flag=1, map_size>=size)
        let mut mp = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut mp, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut mp, 1);
        bytes::BufMut::put_u32_le(&mut mp, st.cfg.map.size);
        st.handle_map_size(99, &mp.freeze());

        assert_eq!(st.players.by_pid(1).unwrap().rejoin, RejoinStage::AwaitingLoaded);
        let ids_sent = drain_ids(&mut rx);
        assert!(ids_sent.contains(&ids::COUNTDOWN_START), "got {ids_sent:?}");
        assert!(ids_sent.contains(&ids::COUNTDOWN_END));
    }

    #[test]
    fn game_loaded_self_starts_catch_up() {
        let (mut st, _pid, _key, mut rx) = playing_with_disconnected("Slash");
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(256);
        st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2));
        st.players.by_pid_mut(1).unwrap().rejoin = RejoinStage::AwaitingLoaded;
        let _ = drain_ids(&mut rx);

        st.handle_loaded(99);

        let p = st.players.by_pid(1).unwrap();
        assert_eq!(p.rejoin, RejoinStage::None);
        assert!(p.loaded, "rejoiner must be marked loaded");
        assert_eq!(p.catchup_cursor, Some(0), "catch-up cursor must start at 0");
        // begin_playing НЕ должен был перезапуститься (sync_counter не обнулён внезапно) —
        // проверяем, что фаза осталась Playing и другие игроки не потеряли loaded.
        assert_eq!(st.phase, GamePhase::Playing);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-engine --lib full_rejoin::tests::map_size_during_awaiting full_rejoin::tests::game_loaded_self_starts`
Expected: FAIL — ветки rejoin не реализованы.

- [ ] **Step 3: Implement**

В `mapxfer.rs::handle_map_size`, сразу после получения `pid` (после `let Some(pid) = ... else { return; };`, до декода report — важно вставить ДО обычной логики):

```rust
    pub fn handle_map_size(&mut self, conn_id: u64, payload: &Bytes) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        // FULL rejoin: карта у клиента есть → сразу к countdown, минуя download-логику.
        if self.players.by_pid(pid).map(|p| p.rejoin) == Some(crate::players::RejoinStage::AwaitingMapSize) {
            self.send_to(pid, spectre_protocol::w3gs::outgoing::countdown_start());
            self.send_to(pid, spectre_protocol::w3gs::outgoing::countdown_end());
            if let Some(p) = self.players.by_pid_mut(pid) {
                p.rejoin = crate::players::RejoinStage::AwaitingLoaded;
            }
            return;
        }
        // ... существующий код декода report и download ...
```

В `actions.rs::handle_loaded`, в начале тела (после получения `pid`):

```rust
    pub fn handle_loaded(&mut self, conn_id: u64) {
        let Some(pid) = self.players.by_conn(conn_id).map(|p| p.pid) else {
            return;
        };
        // FULL rejoin: клиент догрузил карту. Сообщаем ему, что ОСТАЛЬНЫЕ уже в игре,
        // помечаем его loaded и запускаем подачу истории. begin_playing НЕ трогаем.
        if self.players.by_pid(pid).map(|p| p.rejoin) == Some(crate::players::RejoinStage::AwaitingLoaded) {
            let others: Vec<u8> = self
                .players
                .iter()
                .filter(|q| q.pid != pid && !q.virtual_host && q.left.is_none())
                .map(|q| q.pid)
                .collect();
            for opid in others {
                self.send_to(pid, outgoing::game_loaded_others(opid));
            }
            if let Some(p) = self.players.by_pid_mut(pid) {
                p.loaded = true;
                p.rejoin = crate::players::RejoinStage::None;
                p.catchup_cursor = Some(0);
            }
            // остальным — что переджойнер загрузился
            self.broadcast(outgoing::game_loaded_others(pid));
            return;
        }
        // ... существующий код (loaded=true, broadcast, all-loaded → begin_playing) ...
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p spectre-engine --lib full_rejoin`
Expected: PASS (6 тестов).

- [ ] **Step 5: Regression**

Run: `cargo test -p spectre-engine --lib`
Expected: PASS — обычные `all_players_loaded_moves_the_game_to_playing`, `handle_map_size` download-тесты зелёные (rejoin-ветки гейтятся `rejoin != None`).

- [ ] **Step 6: Commit**

```bash
git add crates/spectre-engine/src/mapxfer.rs crates/spectre-engine/src/actions.rs
git commit -m "feat(engine): reactive rejoin handshake (mapsize->countdown, loaded->catchup)"
```

---

### Task A8: Подача истории курсором + скип живой отправки + подавление ложного десинка

**Files:**
- Modify: `crates/spectre-engine/src/state.rs` (`broadcast()` — скип catching-up; новый метод `pump_rejoin_catchup`)
- Modify: `crates/spectre-engine/src/actions.rs` (`on_tick` Playing-арм — вызов помпы; `check_desync` — скип catching_up; `handle_keepalive` — снятие флага при догоне)

**Interfaces:**
- Consumes: `FullHistory::snapshot_from`, `Player.{catchup_cursor, catching_up, sync_counter}`.
- Produces: `GameState::pump_rejoin_catchup(&mut self)`. Инварианты: (1) пока `catchup_cursor.is_some()`, `broadcast()` НЕ шлёт живой пакет этому игроку (он получит всё через курсор, порядок сохранён); (2) помпа шлёт `full_history[cursor..]` через `try_send` до Backpressure, двигая курсор; при `cursor == full_history.len()` → `catchup_cursor=None`, `catching_up=true`; (3) `check_desync` игнорирует `catching_up` игроков; (4) `handle_keepalive` снимает `catching_up`, когда `p.sync_counter + 2 >= self.sync_counter`.

- [ ] **Step 1: Write the failing test** — добавить в `full_rejoin.rs` `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn pump_feeds_history_in_order_then_switches_to_live() {
        let (mut st, _pid, _key, mut rx) = playing_with_disconnected("Slash");
        // наполним историю 5 маркерными пакетами
        for i in 0..5u8 {
            st.broadcast(Bytes::from(vec![0xF7, 0x0C, i]));
        }
        assert_eq!(st.full_history.len(), 5);
        // переджойнер догрузился → курсор на 0
        st.players.by_pid_mut(1).unwrap().conn_id = 99;
        st.players.by_pid_mut(1).unwrap().loaded = true;
        st.players.by_pid_mut(1).unwrap().catchup_cursor = Some(0);
        let _ = drain_ids(&mut rx);

        st.pump_rejoin_catchup();

        // получил все 5 в порядке, курсор снят, флаг catching_up выставлен
        let got = drain_ids(&mut rx);
        assert_eq!(got.len(), 5, "all history must be fed");
        assert_eq!(st.players.by_pid(1).unwrap().catchup_cursor, None);
        assert!(st.players.by_pid(1).unwrap().catching_up);

        // теперь живой broadcast идёт напрямую (курсор снят)
        st.broadcast(Bytes::from(vec![0xF7, 0x0C, 0x63]));
        let live = drain_ids(&mut rx);
        assert_eq!(live.len(), 1, "live packet delivered after catch-up");
    }

    #[test]
    fn broadcast_skips_live_send_while_catching_up_via_cursor() {
        let (mut st, _pid, _key, mut rx) = playing_with_disconnected("Slash");
        st.players.by_pid_mut(1).unwrap().conn_id = 99;
        st.players.by_pid_mut(1).unwrap().catchup_cursor = Some(0);
        let _ = drain_ids(&mut rx);
        // пока курсор активен, живой broadcast не шлётся напрямую (уйдёт через помпу)
        st.broadcast(Bytes::from(vec![0xF7, 0x0C, 1]));
        assert!(drain_ids(&mut rx).is_empty(), "no direct live send during cursor feed");
        // но пакет записан в историю
        assert_eq!(st.full_history.len(), 1);
    }

    #[test]
    fn catching_up_player_is_excluded_from_desync() {
        let (mut st, _pid, _key, _rx) = playing_with_disconnected("Slash");
        // один игрок, помечен catching_up → check_desync не должен его дропнуть/сравнивать
        st.players.by_pid_mut(1).unwrap().loaded = true;
        st.players.by_pid_mut(1).unwrap().catching_up = true;
        st.players.by_pid_mut(1).unwrap().checksums.push_back(0xDEAD);
        st.check_desync(); // не паникует, никого не роняет
        assert!(st.players.by_pid(1).unwrap().left.is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p spectre-engine --lib full_rejoin::tests::pump_feeds full_rejoin::tests::broadcast_skips full_rejoin::tests::catching_up_player`
Expected: FAIL — `pump_rejoin_catchup` нет, скипов нет.

- [ ] **Step 3: Implement**

В `state.rs::broadcast()` — в цикле по игрокам, добавить скип для курсорных игроков ПЕРЕД `try_send` (но после gproxy_buffer.push — нет, для переджойнера буфера нет; просто continue до try_send):

```rust
        for p in self.players.iter_mut() {
            if p.left.is_some() || p.virtual_host {
                continue;
            }
            if let Some(buf) = p.gproxy_buffer.as_mut() {
                buf.push(bytes.clone());
            }
            // FULL rejoin: пока идёт подача истории курсором — не слать живьём (порядок!).
            if p.catchup_cursor.is_some() {
                continue;
            }
            if p.disconnected_since.is_some() {
                continue;
            }
            match p.link.try_send(bytes.clone()) {
                // ... без изменений ...
```

Добавить метод в `impl GameState` (в `state.rs`, рядом с `broadcast`):

```rust
    /// Подаёт FULL-переджойнеру накопленную историю курсором, уважая backpressure.
    /// Когда курсор достигает конца лога — переключает игрока на живой эфир.
    pub fn pump_rejoin_catchup(&mut self) {
        let total = self.full_history.len();
        for p in self.players.iter_mut() {
            let Some(cursor) = p.catchup_cursor else {
                continue;
            };
            if p.left.is_some() {
                p.catchup_cursor = None;
                continue;
            }
            let mut cur = cursor;
            let pending = self.full_history.snapshot_from(cur);
            for pkt in pending {
                match p.link.try_send(pkt) {
                    Ok(()) => {
                        cur += 1;
                        p.consecutive_send_failures = 0;
                    }
                    Err(spectre_net::LinkError::Backpressure) => break,
                    Err(spectre_net::LinkError::Closed) => {
                        // клиент снова умер — вернём в grace, прекратим подачу
                        if p.gproxy && p.disconnected_since.is_none() {
                            p.disconnected_since = Some(std::time::Instant::now());
                        } else {
                            p.left = Some("connection closed during catch-up".into());
                        }
                        p.catchup_cursor = None;
                        break;
                    }
                }
            }
            if p.catchup_cursor.is_some() {
                if cur >= total {
                    // всё, что было на момент старта помпы, отдано → переходим на живой эфир
                    p.catchup_cursor = None;
                    p.catching_up = true;
                } else {
                    p.catchup_cursor = Some(cur);
                }
            }
        }
    }
```

> Замечание по инварианту переключения: `total` фиксируется в начале помпы. Живые пакеты, добавленные в `full_history` во время этой же помпы, будут отданы на следующем вызове помпы, пока курсор < новой длины; переключение на живой эфир происходит только когда курсор догнал `total` этого вызова. Поскольку во время `catchup_cursor.is_some()` `broadcast()` НЕ шлёт живьём, дубликатов/реордера нет: игрок получает строго `history[0..len]` по порядку, затем — живые `broadcast()` начиная с первого пакета после снятия курсора.

В `actions.rs::on_tick`, в `GamePhase::Playing =>` арме, первой строкой (до `check_desync`):

```rust
            GamePhase::Playing => {
                self.pump_rejoin_catchup();
                if let Some(fpid) = self.fake_player_pid
                // ... существующий код ...
```

В `actions.rs::check_desync`, в фильтре активных игроков (строка `if p.left.is_none() && !p.virtual_host && p.loaded {`) добавить `&& !p.catching_up`:

```rust
                if p.left.is_none() && !p.virtual_host && p.loaded && !p.catching_up {
```

В `actions.rs::handle_keepalive`, после `p.sync_counter = p.sync_counter.saturating_add(1);` добавить снятие флага при догоне:

```rust
        if let Some(p) = self.players.by_conn_mut(conn_id) {
            p.sync_counter = p.sync_counter.saturating_add(1);
            if p.catching_up && p.sync_counter + 2 >= self.sync_counter {
                p.catching_up = false;
            }
            if p.checksums.len() >= 512 {
                p.checksums.pop_front();
            }
            p.checksums.push_back(checksum);
        }
```

> Borrow: `self.sync_counter` читается внутри `if let Some(p) = self.players.by_conn_mut(...)`. `self.players` и `self.sync_counter` — раздельные поля, но заимствование `self.players` мутабельно конфликтует с чтением `self.sync_counter` только если компилятор не видит disjoint. В Rust 2021+ disjoint closure/field borrows разрешены для прямых полей: `self.players.by_conn_mut` берёт `&mut self.players`, чтение `self.sync_counter` — `&self.sync_counter`; это РАЗНЫЕ поля, borrow-checker пропускает. Если всё же ругается — считайте `let sc = self.sync_counter;` до `if let`.

- [ ] **Step 4: Run tests**

Run: `cargo test -p spectre-engine --lib full_rejoin`
Expected: PASS (9 тестов).

- [ ] **Step 5: Regression**

Run: `cargo test -p spectre-engine --lib`
Expected: PASS — десинк-тесты (`desync_detection_drops_minority_player`, `..._tie_drops_all_players`) зелёные (обычные игроки `catching_up==false`).

- [ ] **Step 6: Commit**

```bash
git add crates/spectre-engine/src/state.rs crates/spectre-engine/src/actions.rs
git commit -m "feat(engine): cursor-based history feed + catch-up desync suppression"
```

---

### Task A9: Сквозной сценарный тест + seed identity + регрессия всего воркспейса

**Files:**
- Test: `crates/spectre-engine/src/full_rejoin.rs` (`#[cfg(test)]` — сценарный тест)

**Interfaces:**
- Consumes: весь путь A6-A8 + актор `on_frame`.

- [ ] **Step 1: Write the end-to-end test** — добавить в `full_rejoin.rs` тесты:

```rust
    #[test]
    fn random_seed_is_identical_after_full_rejoin() {
        let (mut st, _pid, _key, _rx) = playing_with_disconnected("Slash");
        let seed_before = st.random_seed;
        let req = ReqJoin::decode(&reqjoin_bytes("Slash")).unwrap();
        let (tx2, _r2) = tokio::sync::mpsc::channel(256);
        st.try_full_rejoin(99, &req, [5, 6, 7, 8], PlayerLink::for_test(tx2));
        assert_eq!(st.random_seed, seed_before, "seed must never change on rejoin");
    }

    #[tokio::test]
    async fn end_to_end_full_rejoin_via_actor_frames() {
        use spectre_net::AnyFrame;
        use spectre_protocol::frame::Frame;

        let (mut st, _rxs) = seated_game(2);
        st.players.by_pid_mut(1).unwrap().name = "Slash".into();
        st.players.by_pid_mut(1).unwrap().gproxy = true;
        st.begin_playing();
        // накопим немного истории
        for _ in 0..3 {
            st.on_tick(0);
        }
        let key = st.players.by_pid(1).unwrap().reconnect_key;
        // P1 «умер»: место удержано
        st.players.by_pid_mut(1).unwrap().disconnected_since = Some(std::time::Instant::now());

        // новый процесс: NewConn(конн 99) + GPS FULL + REQJOIN на игровом порту
        let (tx, mut rx) = tokio::sync::mpsc::channel(4096);
        st.add_conn(99, PlayerLink::for_test(tx), [5, 6, 7, 8]);
        st.on_frame(99, AnyFrame::Gps(Frame::new(
            spectre_protocol::gps::ids::FULL,
            spectre_protocol::gps::full(1, key).slice(4..),
        )));
        st.on_frame(99, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Slash"))));

        // клиент рапортует карту → сервер шлёт countdown
        let mut mp = bytes::BytesMut::new();
        bytes::BufMut::put_slice(&mut mp, &[0, 0, 0, 0]);
        bytes::BufMut::put_u8(&mut mp, 1);
        bytes::BufMut::put_u32_le(&mut mp, st.cfg.map.size);
        st.on_frame(99, AnyFrame::W3gs(Frame::new(ids::MAP_SIZE, mp.freeze())));

        // клиент догрузился → сервер запускает catch-up
        st.on_frame(99, AnyFrame::W3gs(Frame::new(ids::GAME_LOADED_SELF, Bytes::new())));
        // помпа отдаёт историю
        st.pump_rejoin_catchup();

        let got = drain_ids(&mut rx);
        assert!(got.contains(&ids::SLOT_INFO_JOIN));
        assert!(got.contains(&ids::MAP_CHECK));
        assert!(got.contains(&ids::COUNTDOWN_START));
        assert!(got.contains(&ids::COUNTDOWN_END));
        assert!(got.contains(&ids::GAME_LOADED_OTHERS));
        assert!(got.contains(&ids::INCOMING_ACTION), "history timeslots must be fed, got {got:?}");
        assert_eq!(st.players.by_pid(1).unwrap().rejoin, RejoinStage::None);
        assert!(st.players.by_pid(1).unwrap().loaded);
    }
```

- [ ] **Step 2: Run to verify it fails/passes**

Run: `cargo test -p spectre-engine --lib full_rejoin::tests::end_to_end`
Expected: PASS (весь путь уже реализован A6-A8). Если FAIL — чинить по сообщению (ожидается PASS).

- [ ] **Step 3: Full workspace regression**

Run: `cargo test`
Expected: PASS — все крейты зелёные. GPS-reconnect живого клиента (`a_valid_reconnect_reattaches_the_player_and_replays`) не сломан (отдельный путь `handle_gps_reconnect`).

- [ ] **Step 4: Lint**

Run: `cargo clippy --all-targets -- -D warnings && cargo fmt --check`
Expected: чисто.

- [ ] **Step 5: Commit**

```bash
git add crates/spectre-engine/src/full_rejoin.rs
git commit -m "test(engine): end-to-end full rejoin scenario + seed identity"
```

---

# ЧАСТЬ B — Клиент `.dll` (C++, отдельный проект `ghostrs-rejoin`)

> **ЧЕСТНОЕ ОГРАНИЧЕНИЕ.** Байты патчей, механика промотки, доступ к `CNetSession`, формулы cap/скорости — верифицированы дизасмом в этой сессии и приведены точно. НО «заставить war3.exe инициировать join в уже начатую игру» и оверлей рендера — это инъекция в живой сетевой/UI-стейт клиента, которую невозможно на 100% де-рисковать статикой: **шаги B6 требуют живого war3.exe для интеграционной проверки.** Это соответствует самому TASK (этапы K + интеграция + замер C.6). Части B1-B5 — детерминированные и юнит-проверяемые в изоляции; B6 — с явным live-gate.
>
> Проект НЕ в этом репозитории (Rust-воркспейс). Создать рядом: `ghostrs-rejoin/` (CMake, MSVC x86, `/MT`). game.dll 32-битная → dll 32-битная.

## File Structure (Part B)

- `ghostrs-rejoin/src/patch.cpp` / `.h` — `PatchBytes` с сигнатурной защитой, таблица P1/P2/P3, apply/revert.
- `ghostrs-rejoin/src/session.cpp` — доступ к `CNetSession*`, `BeginCatchup`/`EndCatchup`.
- `ghostrs-rejoin/src/pump.cpp` — накачка message-pump во время catch-up.
- `ghostrs-rejoin/src/token.cpp` — чтение/запись `ghostrs_token.dat`.
- `ghostrs-rejoin/src/shim.cpp` — GProxy-подобный локальный TCP-шим, инъекция GPS FULL, оркестрация переджойна (**live-gated**).
- `ghostrs-rejoin/src/dllmain.cpp` — точка входа, гейт по ini `enable_full_rejoin`.

---

### Task B1: Скелет проекта + гейт

- [ ] **Step 1:** `CMakeLists.txt` (x86, `add_library(ghostrs_rejoin SHARED ...)`), `dllmain.cpp` с `DllMain` → при `DLL_PROCESS_ATTACH` читать `ghostrs_rejoin.ini` ключ `enable_full_rejoin=1`; если 0/нет — ничего не делать.
- [ ] **Step 2:** Сборка `cmake -S . -B build -A Win32 && cmake --build build --config Release`. Expected: `ghostrs_rejoin.dll` собран.
- [ ] **Step 3: Commit** (в проекте ghostrs-rejoin).

### Task B2: `PatchBytes` с сигнатурной защитой + таблица патчей

- [ ] **Step 1: Юнит-тест (без war3):** написать тест на буфере в памяти, что `PatchBytes` (a) отказывается патчить при несовпадении `oldBytes` и возвращает `false`, (b) патчит и `FlushInstructionCache` при совпадении.

```cpp
// patch.h
#include <cstdint>
#include <cstddef>
bool PatchBytes(void* va, const uint8_t* oldBytes, const uint8_t* newBytes, size_t n);

// patch.cpp
#include <windows.h>
#include <cstring>
#include "patch.h"
bool PatchBytes(void* va, const uint8_t* oldBytes, const uint8_t* newBytes, size_t n) {
    DWORD op;
    if (!VirtualProtect(va, n, PAGE_EXECUTE_READWRITE, &op)) return false;
    if (memcmp(va, oldBytes, n) != 0) { VirtualProtect(va, n, op, &op); return false; }
    memcpy(va, newBytes, n);
    VirtualProtect(va, n, op, &op);
    FlushInstructionCache(GetCurrentProcess(), va, n);
    return true;
}
```

Таблица патчей (`patch.cpp`), применять только при активной промотке, снимать после:

```cpp
// P1 cap imm:  0x6F553622  {A0 0F 00 00} -> {20 4E 00 00}
// P2 clamp imm:0x6F553629  {A0 0F 00 00} -> {20 4E 00 00}
// P3 antifreeze:0x6F5537E5 {89 BE 50 22 00 00} -> {90 90 90 90 90 90}
static const uint8_t P1_OLD[4]={0xA0,0x0F,0x00,0x00}, P1_NEW[4]={0x20,0x4E,0x00,0x00};
static const uint8_t P2_OLD[4]={0xA0,0x0F,0x00,0x00}, P2_NEW[4]={0x20,0x4E,0x00,0x00};
static const uint8_t P3_OLD[6]={0x89,0xBE,0x50,0x22,0x00,0x00}, P3_NEW[6]={0x90,0x90,0x90,0x90,0x90,0x90};
bool ApplySpeedPatches() {
    bool ok = PatchBytes((void*)0x6F553622, P1_OLD, P1_NEW, 4)
           && PatchBytes((void*)0x6F553629, P2_OLD, P2_NEW, 4)
           && PatchBytes((void*)0x6F5537E5, P3_OLD, P3_NEW, 6);
    return ok; // если хоть один false — сигнатуры не совпали (другая версия dll) → abort
}
void RevertSpeedPatches() {
    PatchBytes((void*)0x6F553622, P1_NEW, P1_OLD, 4);
    PatchBytes((void*)0x6F553629, P2_NEW, P2_OLD, 4);
    // P3 можно оставить (безопасен при SPEED_NUM==1), но для чистоты откатываем:
    PatchBytes((void*)0x6F5537E5, P3_NEW, P3_OLD, 6);
}
```

- [ ] **Step 2:** Прогнать юнит-тест `PatchBytes` на локальном буфере. Expected: PASS.
- [ ] **Step 3: Live-smoke (нужен war3):** загрузить dll в war3 1.26a, вызвать `ApplySpeedPatches()` при старте; проверить в отладчике, что байты по адресам сменились и war3 не крашится. Gate.
- [ ] **Step 4: Commit.**

### Task B3: `CNetSession*` + управление `SPEED_NUM/DEN`

- [ ] **Step 1:** реализовать доступ по верифицированной цепочке пролога `sub_6F53F160`:

```cpp
// session.cpp — цепочка: mov ecx,0Dh; call 0x6F4C34D0; mov eax,[eax+0x10]; mov ecx,[eax+8]
typedef void* (__fastcall *getctx_t)(int, int, int); // __fastcall: ecx=arg
static void* GetCtx13() {
    // 0x6F4C34D0 принимает индекс в ecx (mov ecx,0Dh). Оборачиваем через __fastcall.
    getctx_t f = (getctx_t)0x6F4C34D0;
    return f(13, 0, 0);
}
struct CNetSession; // непрозрачно
CNetSession* GetSession() {
    uint8_t* ctx = (uint8_t*)GetCtx13();
    if (!ctx) return nullptr;
    uint8_t* a = *(uint8_t**)(ctx + 0x10);
    if (!a) return nullptr;
    return *(CNetSession**)(a + 0x08);
}
static inline uint32_t* SpeedNum(CNetSession* s){ return (uint32_t*)((uint8_t*)s + 0x22B4); }
static inline uint32_t* SpeedDen(CNetSession* s){ return (uint32_t*)((uint8_t*)s + 0x22B8); }
static inline uint32_t* SpeedGate(CNetSession* s){ return (uint32_t*)((uint8_t*)s + 0x22BC); }

static uint32_t g_savedNum=1, g_savedDen=1;
void BeginCatchup(CNetSession* s) {
    g_savedNum = *SpeedNum(s); g_savedDen = *SpeedDen(s);
    *SpeedNum(s) = 99999; *SpeedDen(s) = 1; // acc насыщает cap → 800 тёрнов/кадр
}
void EndCatchup(CNetSession* s) {
    *SpeedNum(s) = g_savedNum ? g_savedNum : 1;
    *SpeedDen(s) = g_savedDen ? g_savedDen : 1;
}
```

- [ ] **Step 2: Live-smoke:** в лобби/игре залогировать `GetSession()`, `*SpeedNum` (ожидаем 1), `*SpeedDen` (1), gate `+0x22BC` (0). Gate.
- [ ] **Step 3: Commit.**

### Task B4: Накачка message-pump во время catch-up

- [ ] **Step 1:**

```cpp
// pump.cpp — во время промотки один кадр может занять сотни тёрнов; качаем окно,
// чтобы Windows не показала «Не отвечает». cap=20000ms (800 тёрнов/кадр) уже
// не даёт кадру занять 15+с, но помпа обязательна.
void PumpWindowOnce() {
    MSG msg;
    while (PeekMessageA(&msg, nullptr, 0, 0, PM_REMOVE)) {
        TranslateMessage(&msg);
        DispatchMessageA(&msg);
    }
}
```

- [ ] **Step 2: Live-smoke:** во время искусственной промотки убедиться, что окно war3 остаётся отзывчивым. Gate.
- [ ] **Step 3: Commit.**

### Task B5: Файл-токен `ghostrs_token.dat`

- [ ] **Step 1: Юнит-тест (без war3):** парс/сериализация формата (текст, рядом с war3.exe):

```
host=1.2.3.4
port=6113
pid=5
key=305419896
```

Функции `bool WriteToken(host,port,pid,key)`, `bool ReadToken(&host,&port,&pid,&key)`. `port` = **игровой host_port** (тот, на который war3 изначально приконнектился, НЕ 6114). Захватывать его хуком успешного `connect()` при первичном джойне, `pid`/`key` — из ответа GPS INIT (`gps::init` шлёт `pid` по смещению [4], `key` LE по [5..9]).

- [ ] **Step 2:** прогнать юнит-тест round-trip. Expected: PASS.
- [ ] **Step 3: Commit.**

### Task B6: GProxy-подобный шим + инъекция GPS FULL + оркестрация (**LIVE-GATED**)

> Это ядро клиента и единственная часть, которую нельзя гарантированно закрыть без живого war3. Рекомендуемая архитектура — как у GProxy++: war3 коннектится на `127.0.0.1:<localport>`, dll-шим держит апстрим к серверу.

- [ ] **Step 1:** Локальный TCP-listener в dll. На старте war3 (если `ReadToken` успешен): открыть апстрим TCP к `host:port`; **первым делом** отправить `gps::full(pid,key)` (кадр `F8 05 09 00 <pid> <key LE>`); затем проксировать war3↔сервер.
- [ ] **Step 2:** Заставить war3 приконнектиться к локальному шиму и выдать REQJOIN. Вариант A (рекомендуется): подменить адрес назначения в хуке `connect()` war3 на локальный шим, спровоцировав join через обычный LAN/direct-join флоу war3 на «фейковую» запись игры, которую dll кладёт в список. Вариант B: если war3 отказывается инициировать join сам — синтезировать REQJOIN в апстрим от имени war3, а нисходящие SLOTINFOJOIN/PLAYERINFO/MAPCHECK/COUNTDOWN — пропускать в war3 как обычный джойн. **Конкретный рабочий вариант выбирается на живом war3 (gate).**
- [ ] **Step 3:** Оркестрация промотки: когда апстрим начал слать историю (первый `INCOMING_ACTION 0x0C`), вызвать `ApplySpeedPatches()` + `BeginCatchup(GetSession())`; в цикле `PumpWindowOnce()`; критерий конца — очередь тёрнов пуста N кадров подряд И `acc < 25` (`*(uint32_t*)(s+0x2250) < 25`) И живой поток продолжается → `EndCatchup()` + `RevertSpeedPatches()`.
- [ ] **Step 4: LIVE E2E gate** (см. Часть C). Здесь итерации на реальном war3 ОЖИДАЕМЫ и допустимы — это не «доп-ресерч», а неизбежная интеграция с закрытым клиентом.
- [ ] **Step 5: Commit.**

---

# ЧАСТЬ C — Интеграция, замер, приёмка

### Task C1: Замер цены тёрна (закрывает чек-лист C.6)

- [ ] Собрать war3 с P1-P3, залогировать wall-time на N тёрнов промотки → нс/тёрн. Подтвердить дефолт cap `0x4E20` (20000ms). Если >1ms/тёрн в поздней игре — принять как данность (144k×1ms=144s — единственный сценарий, где цель <30s недостижима; тогда профилировать симуляцию, не снижать cap). Записать замер в `docs/FULL_REJOIN_TASK.md` раздел C.6.

### Task C2: Сквозная приёмка (сценарий G)

- [ ] 5 игроков стартуют (один — с нашим dll, gproxy включён). На 5-й минуте у него `kill -9 war3.exe`. Рестарт war3 с dll → автоподключение (B6) → сервер отдаёт handshake (A6-A7) → промотка (B2-B4) → игрок управляет героем.
  - Цель: < 60s от запуска war3 до контроля (30-мин игра); < 30s для 60-мин.
  - Состояние (золото/уровень/предметы/позиция) совпадает с логом сервера.
  - Остальные 4 — без десинка (A8 подавляет ложный десинк catch-up).
  - Обычный GPS-reconnect живого клиента не сломан; `cargo test` зелёный (A9).

---

## Риски / откаты

- Серверная часть (A) полностью за существующей инфраструктурой; фича активна только при валидном токене + удержанном месте. Регрессия покрыта A9 (`cargo test`).
- Клиентские патчи — за конфигом `enable_full_rejoin`; несовпадение байт-сигнатур → `ApplySpeedPatches()` вернёт false → abort промотки, war3 продолжает как обычно.
- P3 безопасен постоянно (антифриз-ветка исполняется лишь при SPEED_NUM>1 — резолюция C.2).
- `FullHistory` cap 216k (~90 мин) → рост RAM ограничен.
- B6 — единственный компонент с неизбежной живой итерацией; изолирован от сервера, не влияет на `cargo test`.

---

## Self-review (спец против плана)

- **Покрытие TASK:** S1 FullHistory → A1-A2; S2 join-in-progress → A6-A7; S3 токен → A4-A5 + B5; S4 тесты → A2/A6/A7/A8/A9; K1 патчи → B2; K2 промотка → B3; K3 автоподключение → B5-B6; K4 pump → B4; C.1/C.2/C.3/C.4/C.5 → резолюции в шапке; C.6 → C1; приёмка G → C2. RandomSeed (B.5.1) → A9-seed. Восстановление скорости (B.3.3) → B3 `EndCatchup`.
- **Исправления против PLAN_100:** `outgoing::start_info` не существует → используется `countdown_start`+`countdown_end`; `Player.slot_index` не существует → `SlotTable::sid_of_pid`; `map_check` 5-арг (не 2); подача истории — курсором с backpressure (актор синхронный, нельзя блокировать на 30 МБ); добавлено подавление ложного десинка на catch-up (в PLAN_100 отсутствовало, привело бы к кику переджойнера); токен — GPS FULL (0x05), а не хвост REQJOIN; роутинг — без изменений супервизора (переконнект на игровой host_port).
- **Типы согласованы:** `RejoinStage` (players.rs) используется в full_rejoin/mapxfer/actions единообразно; `catchup_cursor: Option<u32>`; `pump_rejoin_catchup`, `try_full_rejoin`, `full`, `decode_full`, `FullHistory::snapshot_from` — имена сквозные.
