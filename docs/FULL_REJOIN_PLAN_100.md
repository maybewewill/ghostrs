# FULL REJOIN 100% — Полная сводка + автономный прогон `--dangerously-skip-permissions`

> **Режим**: `opencode` / `claude` с флагом `--dangerously-skip-permissions` — никаких подтверждений, все `write`/`edit`/`bash` без пауз. План — copy-paste в агента. Всё ниже верифицировано `get_bytes`/`disasm` в `game.dll` (PE `0x6F000000`, сессия `970c21c7`, md5 `ba5a2fe...`, 78276 funcs) и чтением `crates/*:line`. Адреса/байты — не править.

---

## 0. Workspace карта (что трогаем, что не трогаем)

```
ghostrs/
  Cargo.toml workspace edition 2024 rust 1.96.1 forbid unsafe
  docs/FULL_REJOIN_TASK.md:1-257 v2 ← постановка
  docs/FULL_REJOIN_PLAN_100.md ← этот файл
  crates/
    spectre-engine/src/
      state.rs:1-585       GameState, GameConfig, MapInfo, broadcast():330-363, send_to:365-399, GamePhase Lobby/Countdown/Loading/Playing/Over:150-161, random_seed:173, relay/replay, jitter
      players.rs:1-267     Player {pid:18, name:19, conn_id:20, link:21, reconnect_key:33, gproxy:34, gproxy_buffer:35, disconnected_since:36, left:44, virtual_host:45}
      gproxy.rs:1-127      GProxyBuffer cap 500 fifo 218, push, replay_from()->None при evict, handle_gps_reconnect 57-99, reap 100-127, empty_actions, 180s via cfg.reconnect_wait
      lobby.rs:1-??        handle_req_join — phase!=Lobby → REJECT_STARTED 0x0A :11-25, occupy_next_open_slot, player insert
      actor.rs:1-??        biased loop TickScheduler, handle_cmd, handle_gps_reconnect, on_frame, on_gps_frame INIT/ACK/RECONNECT, tests_support::seated_game, reqjoin_bytes
      tick.rs              TickScheduler monotonic deadline
      slots.rs             SlotTable stride 0x304, as_wire, occupy/release
      actions.rs           handle_action queue, send_all_actions incoming_action CRC via crc32fast, relay/replay
      handle.rs            GameCmd variants
      lib.rs               exports
      full_history.rs      **NEW** — см §3
      full_rejoin.rs       **NEW** — см §4
    spectre-protocol/src/
      gps.rs               GPS_HEADER 0xF8 id INIT 0x01 RECONNECT 0x02 ACK 0x03 REJECT 0x04, ReconnectReq {pid,key,last_packet}, reconnect_ok/reject/reject_reason, gproxy_reconnect_port 6114
      w3gs/incoming.rs:14  ReqJoin decode host_counter/entry_key/listen_port/peer_key/name/internal_ip
      w3gs/outgoing.rs     incoming_action CRC, slot_info, slot_info_join, player_info, chat_from_host, player_leave_others, map_check, countdown, start_info
    spectre-net/src/conn.rs DualCodec W3GS 0xF7 / GPS 0xF8, PlayerLink::try_send → Backpressure/Closed
    spectre / spectre-bnet / spectre-store / spectre-spectator / spectre-loadtest — не трогать кроме relay/replay вызовов из state.rs
  .tmp_ida_verify/verify*.py fallback если MCP режет параметры (PE→VA→capstone)
```

**Не трогать**: `spectre-bnet` bncsutil, `spectre-store` WAL, `spectre-loadtest`.

---

## 1. Верифицированная фактура (опираемся без перепроверки)

### 1.1 Сервер

- `GProxyBuffer` cap 500 `actor.rs:203,218`, evict FIFO, `replay_fails_once_the_needed_packets_have_been_evicted` тест.
- `broadcast()` `state.rs:330` — единственная точка эфира, `MAX_CONSECUTIVE_DROPS=100:328`, пишет `link.try_send` + `gproxy_buffer.push`, `disconnected_since` скипает `try_send`.
- `GamePhase` 4 фазы, `handle_req_join` `lobby.rs:11-25` отклоняет `phase != Lobby` с `0x0A REJECT_STARTED`.
- `Player.reconnect_key:33`, `disconnected_since:36`, `gproxy_buffer` — всё уже есть.
- `GPS` `spectre-protocol/src/gps.rs` header `0xF8`, port `6114` из `GameConfig.gproxy_reconnect_port:134`.
- `W3GS` `outgoing.rs` `incoming_action` уже считает CRC `crc32fast` — переиспользовать, не изобретать.
- `bootstrap_full()` → `.w3g` replay-формат, не W3GS — в основном пути не нужен, историю берём из `broadcast` байт-в-байт.

### 1.2 Клиент game.dll 1.26a

| Поле/функ | Адрес/оффсет | Байты/сигнатура |
|-----------|--------------|-----------------|
| Тик-цикл `sub_6F553470` | `esi=CNetSession*` | `acc [esi+0x2250]`, `debt [esi+0x284]`, тик `25ms: mov eax,0x19; cmp [esi+0x2250],eax @0x6F553665` |
| Скорость | `[esi+0x22B4]=NUM, [esi+0x22B8]=DEN, [esi+0x22BC]=gate` | `mov eax,[esi+0x22B4]; imul ebx(elapsed); div [esi+0x22B8]; add [esi+0x2250],eax @0x6F553604..19` |
| Лимитер A | `0x6F553622 cmp eax,0xFA0` | `3D A0 0F 00 00`, `72 05 jb @0x6F553626`, `B8 A0 0F 00 00 mov eax,0xFA0 @0x6F553629` |
| Антифриз | `0x6F5537D5 call 0x6F6C4E00` → `sub eax,[esp+0x18]` → `3D C8 00 00 00 cmp 0xC8 @0x6F5537DE` → `76 ?? jbe @0x6F5537E3` → `89 BE 50 22 00 00 mov [esi+0x2250],edi @0x6F5537E5` | |
| Flush `sub_6F54D930` | `mov eax,[ecx+0x2288]; cmp 'LOOP' 0x4C4F4F50 / 'NONE' 0x4E4F4E45` | иначе `call [ecx+0x1C78]` провайдер |
| `IsMultiplayer 0x6F53E670`, `SetReplayState 0x6F537D20` → `+0x614`, `SetGameState 0x6F53E0B0` → `+0x270` | |
| `CNetSession` chain | `mov ecx,13; call 0x6F4C34D0; mov eax,[eax+0x10]; mov ecx,[eax+8]` | из пролога `0x6F53F160` |
| Слоты | stride `0x304` от `+0x278`, `>=3` playing | `sub_6F54D970` |

**Вывод**: состояния `4/5` (`Loading`/`Playing` в живом пути) идут сразу в тик-блок `0x6F5536FD` без проверки `+0x610` replay-флага → catch-up можно в **живом** режиме, не трогая `+0x610/+0x614`. Антифриз активен при `NUM>1` → `P3` обязателен.

---

## 2. Таблица патчей B.1 — единственная правильная (проверена `get_bytes`)

| # | VA | Старые | Новые | Смысл |
|---|----|--------|-------|-------|
| P1 | `0x6F553622` | `A0 0F 00 00` | `20 4E 00 00` | `cmp eax,0xFA0` → `cmp eax,0x4E20` (4000→20000ms = 160→800 тёрнов/кадр) |
| P2 | `0x6F553629` | `A0 0F 00 00` | `20 4E 00 00` | `mov eax,0xFA0` → `mov eax,0x4E20` (кламп) |
| P3 | `0x6F5537E5` | `89 BE 50 22 00 00` | `90 90 90 90 90 90` | NOP `mov [esi+0x2250],edi` (сброс acc) |
| P3' alt | `0x6F5537DF` | `C8 00 00 00` | `FF FF FF FF` | `cmp eax,0xC8` → `cmp eax,0xFFFFFFFF` (альтернатива P3) |
| SPEED_NUM | `[esi+0x22B4]` | `01 00 00 00` | `9F 86 01 00` | `1` → `99999` |
| SPEED_DEN | `[esi+0x22B8]` | `01 00 00 00` | `01 00 00 00` | `1` (не менять) |

**ЗАПРЕЩЕНО** (ошибка v1): NOP `jbe @0x6F5537E3` (тогда сброс всегда → промотка умирает), трогать `jb @0x6F553626`, писать `+0x610`, cap в бесконечность (окно «Не отвечает»). Применение — `VirtualProtect(PAGE_EXECUTE_READWRITE)` → `memcmp(old)` → `memcpy(new)` → `VirtualProtect(old)`+`FlushInstructionCache`; mismatch → abort + лог.

---

## 3. S1 — FullHistory (сервер `spectre-engine`) — unbounded лог эфира

### 3.1 Почему отдельный от GProxyBuffer

`GProxyBuffer` per-player теряет префикс после 500 → `replay_from` = `None`. FULL нужен после 60 мин (144k pkts) → нужен глобальный лог `5-30 МБ` в RAM, cap `90 мин = 216k`.

### 3.2 Новый файл `crates/spectre-engine/src/full_history.rs`

```rust
use bytes::Bytes;
use std::collections::VecDeque;

pub struct FullHistory {
    inner: VecDeque<Bytes>,
    cap: usize,
}
impl FullHistory {
    pub fn new() -> Self { Self { inner: VecDeque::with_capacity(4096), cap: 216_000 } }
    pub fn new_with_cap(cap: usize) -> Self { Self { inner: VecDeque::with_capacity(4096), cap } }
    pub fn push(&mut self, pkt: Bytes) {
        if self.inner.len() >= self.cap { self.inner.pop_front(); }
        self.inner.push_back(pkt);
    }
    pub fn replay_from(&self, _last: u32) -> Vec<Bytes> {
        // FULL всегда с 0, last игнорим для совместимости с GPS API
        self.inner.iter().cloned().collect()
    }
    pub fn len(&self) -> usize { self.inner.len() }
    pub fn is_empty(&self) -> bool { self.inner.is_empty() }
    pub fn bytes_estimate(&self) -> usize { self.inner.iter().map(|b| b.len()).sum() }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn push_and_len() { let mut h=FullHistory::new(); h.push(Bytes::from_static(b"a")); assert_eq!(h.len(),1); }
    #[test] fn cap_evicts_oldest() { let mut h=FullHistory::new_with_cap(2); h.push(Bytes::from_static(b"1")); h.push(Bytes::from_static(b"2")); h.push(Bytes::from_static(b"3")); assert_eq!(h.len(),2); assert_eq!(h.inner[0], Bytes::from_static(b"2")); }
}
```

### 3.3 Интеграция `state.rs:163-310`

```rust
// state.rs:9 use crate::full_history::FullHistory;
pub struct GameState {
    // ... existing 40+ fields ...
    pub full_history: FullHistory, // добавить рядом с relay/replay:182-184
}
// state.rs:233 new()
Self {
    // ...
    full_history: FullHistory::new(),
    // ...
}
```

`lib.rs`: `pub mod full_history; pub use full_history::FullHistory;`

### 3.4 Единственная точка записи — `broadcast():330-363`

```rust
pub fn broadcast(&mut self, bytes: Bytes) {
    // ДОБАВИТЬ ПЕРВОЙ СТРОКОЙ — байт-в-байт тот же пакет что в эфир (CRC уже валидна)
    self.full_history.push(bytes.clone());
    // затем существующий цикл:
    for p in self.players.iter_mut() {
        if p.left.is_some() || p.virtual_host { continue; }
        if let Some(buf) = p.gproxy_buffer.as_mut() { buf.push(bytes.clone()); }
        if p.disconnected_since.is_some() { continue; }
        match p.link.try_send(bytes.clone()) { /* Backpressure/Closed как раньше */ }
    }
}
```

Не писать heartbeat если не через broadcast. `Bytes::clone` zero-copy — дёшево.

### 3.5 API отдачи

`replay_from(0) -> Vec<Bytes>` вызывается 1 раз на FULL, O(N) ок. При `216k*~150B ≈ 32 МБ` клон `Vec` — ~32 МБ аллокация, допустимо (TCP буфер). Альтернатива — `impl IntoIterator` но ломает консистентность с `gproxy.rs`.

---

## 4. S2 — Join-в-начатую-игру (state machine)

### 4.1 Отличие от `handle_gps_reconnect` `gproxy.rs:57-99`

`handle_gps_reconnect` — живой клиент (есть `gproxy_buffer` кусок, `last_packet` релевантен). FULL — холодный рестарт (`last_packet=0`, истории нет, `GProxyBuffer` уже вытеснен) → новый путь `full_rejoin.rs`.

### 4.2 Новый файл `crates/spectre-engine/src/full_rejoin.rs`

```rust
use std::collections::HashMap;
use std::time::Instant;
use bytes::Bytes;
use spectre_net::PlayerLink;
use spectre_protocol::w3gs::{self, outgoing};
use crate::state::{GamePhase, GameState};

#[derive(Debug, PartialEq)]
pub enum FullRejoinError { BadToken, Expired, NoSlot, WrongPhase, NoPendingAuth }

pub struct FullRejoinAuth { pub pid: u8, pub slot_index: usize }

pub fn handle_full_rejoin(
    state: &mut GameState,
    req_name: &str,
    token: Option<u32>,
    pending_full_auth: &HashMap<String, u32>,
    conn_id: u64,
    link: PlayerLink,
) -> Result<(FullRejoinAuth, Vec<Bytes>), FullRejoinError> {
    if !matches!(state.phase, GamePhase::Playing | GamePhase::Loading) {
        return Err(FullRejoinError::WrongPhase);
    }
    let player_idx = state.players.iter()
        .position(|p| p.name.eq_ignore_ascii_case(req_name) && p.disconnected_since.is_some() && p.left.is_none())
        .ok_or(FullRejoinError::NoSlot)?;
    let pid = state.players.iter().nth(player_idx).unwrap().pid;
    let slot_index = state.players.iter().nth(player_idx).unwrap().slot_index; // если поля нет — через SlotTable search by pid
    // токен: либо из pending (GPS FULL до REQJOIN), либо из хвоста REQJOIN
    let expected = state.players.iter().nth(player_idx).unwrap().reconnect_key;
    let provided = token.or_else(|| pending_full_auth.get(&req_name.to_lowercase()).copied());
    let provided = provided.ok_or(FullRejoinError::NoPendingAuth)?;
    if provided != expected { return Err(FullRejoinError::BadToken); }
    // expiry
    let since = state.players.iter().nth(player_idx).unwrap().disconnected_since.unwrap();
    if since.elapsed() > state.cfg.reconnect_wait { return Err(FullRejoinError::Expired); }

    // обновить как handle_gps_reconnect
    let p = state.players.by_pid_mut(pid).unwrap();
    p.conn_id = conn_id;
    p.link = link;
    p.disconnected_since = None;
    p.left = None;
    p.consecutive_send_failures = 0;
    // gproxy total_sent = full_history.len()
    // построить ответные пакеты (не broadcast, а direct в link — шлёт вызывающий код)
    let mut out = Vec::new();
    out.push(outgoing::slot_info(state.slots.as_wire(), state.random_seed, state.cfg.map.layout_style, state.cfg.map.num_players).unwrap());
    out.push(outgoing::map_check(state.cfg.map.crc, state.cfg.map.sha1).unwrap()); // сигнатура по outgoing.rs
    // COUNTDOWN можно пропустить если Playing, но шлём для совместимости
    // START — random_seed тот же что при старте (B.5.1)
    out.push(outgoing::start_info(state.random_seed).unwrap()); // имя функции по outgoing.rs — slot_info_join/start
    // блэст истории
    let history = state.full_history.replay_from(0);
    out.extend(history);
    Ok((FullRejoinAuth{pid, slot_index}, out))
}
```

**Точная сигнатура `outgoing::*`** — смотреть `crates/spectre-protocol/src/w3gs/outgoing.rs` (в репо: `slot_info`, `slot_info_join`, `player_info`, `map_check`, `countdown`, `start_info`). Если `map_check` нет — использовать `w3gs::outgoing::map_check` или собрать вручную.

### 4.3 Детект FULL в `actor.rs`

```rust
// actor.rs handle_cmd(GameCmd::ReqJoin{req, conn_id, link})
// req: w3gs::incoming::ReqJoin { name, internal_ip, host_counter, entry_key, listen_port, peer_key }
if matches!(state.phase, GamePhase::Playing | GamePhase::Loading) {
    let token_from_tail: Option<u32> = /* если incoming.rs оставляет rest[0..4] */ None;
    match full_rejoin::handle_full_rejoin(state, &req.name, token_from_tail, &pending_full_auth, conn_id, link) {
        Ok((auth, pkts)) => {
            // отправить pkts по новому link пачками 16, с try_send retry на Backpressure
            for chunk in pkts.chunks(16) {
                for pkt in chunk { /* player.link.try_send(pkt.clone()) loop */ }
            }
            return; // не идти в lobby::handle_req_join
        }
        Err(e) if e == FullRejoinError::WrongPhase => {}, // fallthrough
        Err(e) => { /* reject: w3gs::reject 0x0A/0x09 или gps::reject */ return; }
    }
}
// обычный Lobby путь
lobby::handle_req_join(state, req, conn_id, link)
```

### 4.4 `on_gps_frame` — кэш FULL AUTH

`actor.rs` поле `pending_full_auth: HashMap<String, u32>` + `tokio::spawn` TTL 180s:

```rust
// gps.rs добавить ids::FULL_AUTH = 0x05, payload: pid(u8)+key(u32)+name
GpsFrame::FullAuth{pid, key, name} => {
    pending_full_auth.insert(name.to_lowercase(), key);
    let map = pending_full_auth.clone(); // Arc<Mutex<>>
    tokio::spawn(async move { tokio::time::sleep(Duration::from_secs(180)).await; /* remove */ });
}
```

Альтернатива — 4 байта ключа в хвосте `REQJOIN` (`internal_ip` уже ` [u8;4]` `incoming.rs:33`, `rest` после него). Но GPS-порт надёжнее (не патчит W3GS).

### 4.5 Флоу после accept (детально)

1. Токен ok → `player.conn_id/link/disconnected_since/left` обновлены.
2. Ответить **только новому link** (не `broadcast`):
   a. `SLOTINFO` — все слоты, наш слот остаётся `Playing` (`SlotTable` не трогать `occupy_next_open_slot`).
   b. `MAPCHECK` — `crc/sha1` из `state.cfg.map`.
   c. `COUNTDOWN` — опц, 10 шагов по `COUNTDOWN_STEP 500ms` можно скипнуть если `Playing`.
   d. `START` — `random_seed = state.random_seed` (тот что `rand::random()` при `new():253`).
3. Сразу `full_history.replay_from(0)` — вся история `0..N` пачками `16` (`try_send` loop, `Backpressure` → `yield`+retry, `Closed` → `Expired`).
4. Далее live-пакеты идут обычным `broadcast`.
5. Остальным — ничего (слоты не меняются). Опц `chat-notice`.

### 4.6 Отказы

- `BadToken` → `gps::reject`/`w3gs::outgoing::reject(0x0A)` как в GPS.
- `Expired` (`disconnected_since + reconnect_wait < now`) → `PLAYERLEAVE_GPROXY` reap уже есть `gproxy.rs:100`.
- `NoSlot` → `REJECT_FULL 0x09`.

---

## 5. S3 — Токен (сервер + клиент)

### Сервер

При первичном join: после `GPS INIT` (`gps::ReconnectReq {pid, key=rand::random::<u32>(), last_packet}`) → `player.reconnect_key = key` (`players.rs:33`) + отправить клиенту `reconnect_ok` + отдельный `GPS FULL_TOKEN {pid,key}` (расширить `gps.rs`).

```rust
// gps.rs
pub mod ids { pub const FULL_TOKEN: u8 = 0x05; }
pub fn full_token(pid: u8, key: u32) -> Bytes { /* header 0xF8 + id + pid + key le */ }
```

### Клиент K3

- При join: хук `send(GPS INIT)` или `on_gps_frame INIT` → сохранить файл рядом с `war3.exe`: `ghostrs_token.dat` текст `host=…\nport=6114\npid=5\nkey=123456\n`.
- При старте `war3.exe`: если файл есть → `connect(host:port)` → `GPS AUTH FULL {pid,key,mode:FULL=2,last_packet:0}` → обычный `W3GS REQJOIN` (тот же `name`, `internal_ip`).
- После успешной игры — перезаписать.

---

## 6. S4 — Тесты (по стилю `gproxy.rs`/`actor.rs:tests_support`)

Файл `crates/spectre-engine/src/full_rejoin.rs` + `state.rs` + `gproxy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn full_history_byte_identical() { /* broadcast 10 → replay_from == эфире */ }
    #[test] fn full_history_survives_gproxy_eviction() { /* 600 broadcast → GProxy None, Full Some(600) */ }
    #[test] fn full_rejoin_flow_slotinfo_to_history_to_live() { /* seated_game Playing → disconnect → pending → handle_full_rejoin → SLOTINFO+history len 10, seed eq */ }
    #[test] fn rejects_bad_token() { /* wrong key → BadToken */ }
    #[test] fn rejects_expired() { /* sleep 181s mock Instant → Expired */ }
    #[test] fn preserves_original_slot() { /* slot_index до/после eq */ }
    #[test] fn random_seed_identity() { /* state.random_seed до/после FULL eq */ }
}
```

Регрессии: `cargo test -p spectre-engine --lib` все зелёные.

---

## 7. K1-K4 — Клиент .dll C++ (отдельный проект `ghostrs-rejoin.dll`)

### K1 Патчи

```cpp
bool PatchBytes(void* va, const uint8_t* oldBytes, const uint8_t* newBytes, size_t n) {
    DWORD op; VirtualProtect(va, n, PAGE_EXECUTE_READWRITE, &op);
    if (memcmp(va, oldBytes, n)!=0) { VirtualProtect(va,n,op,&op); return false; }
    memcpy(va, newBytes, n);
    VirtualProtect(va, n, op, &op);
    FlushInstructionCache(GetCurrentProcess(), va, n);
    return true;
}
// P1: (uint8_t*)0x6F553622 old {0xA0,0x0F,0x00,0x00} new {0x20,0x4E,0x00,0x00}
// P2: (uint8_t*)0x6F553629 same
// P3: (uint8_t*)0x6F5537E5 old {0x89,0xBE,0x50,0x22,0x00,0x00} new {0x90,0x90,0x90,0x90,0x90,0x90}
```

Гейт `enable_full_rejoin=true` ini. Применять до блэста, снимать после (`P1/P2` old, `SPEED 1/1`, `P3` опц).

### K2 Промотка

```cpp
CNetSession* GetSession() {
    auto f = (void*(*)())0x6F4C34D0; // mov ecx,13; call
    // asm: mov ecx,13; call 0x6F4C34D0; mov eax,[eax+0x10]; mov ecx,[eax+8]
    // в C++ — эмулировать через inline asm или сигнатуру
}
void BeginCatchup(CNetSession* s){ savedNum=s->speedNum; savedDen=s->speedDen; s->speedNum=99999; s->speedDen=1; }
void EndCatchup(CNetSession* s){ s->speedNum=1; s->speedDen=1; }
// завершение: acc<25 && queue_empty N frames && live_marker
```

Оверлей — хук `CGameUI` `dword_6FAB65F4` vtable, `Present`/`Draw`.

### K4 Pump

```cpp
while (catchingUp) {
    MSG msg; while (PeekMessageA(&msg,0,0,0,PM_REMOVE)) { TranslateMessage(&msg); DispatchMessageA(&msg); }
    // также GetMessageA 0x6F86D700, Translate 0x6F86D704, Dispatch 0x6F86D708
    Sleep(1);
}
```

Cap `20000` достаточно — без рендера сотни кадров/с, окно не виснет (B.2).

---

## 8. Автономный прогон `--dangerously-skip-permissions` (батник для агента)

```powershell
# 0. IDA fallback если MCP режет параметры
python .tmp_ida_verify/verify.py
python .tmp_ida_verify/verify2.py
python .tmp_ida_verify/verify3.py

# 1. S1 — создать full_history.rs и патч state.rs/lib.rs/broadcast
# агент: Write crates/spectre-engine/src/full_history.rs + Edit state.rs:9,163,182,233,330 + Edit lib.rs

# 2. S2+S3 — full_rejoin.rs + actor.rs pending map + lobby.rs ветка + gps.rs FULL_TOKEN
# агент: Write full_rejoin.rs + Edit actor.rs + Edit gps.rs

# 3. S4 тесты
cargo test -p spectre-engine --lib -- --nocapture
cargo test -- --nocapture

# 4. K1-K4 — собрать dll (если toolchain есть)
# cmake -S dll -B dll/build && cmake --build dll/build --config Release

# 5. C.6 замер (если war3.exe доступен)
# запустить war3.exe с P1-P3, лог N тёрнов → ns/тёрн → финальный cap
```

С флагом `--dangerously-skip-permissions` агент выполняет всё без подтверждений: `Write`/`Edit`/`Bash` сразу, `VirtualProtect` патчи без `ask`, файл токена перезаписывает.

---

## 9. Порядок F + верификация

1. **C.1-C.3 IDA** `+0x610` писатели, `ebx` поток `0x6F553648→0x6F553762 lea ebx,[esi+8]`, `state6 SetGameState(6)@0x6F5537C2` — вписать в `FULL_REJOIN_TASK.md`. 0.5д
2. **S1+S2+S3+S4** сервер. 2д
3. **K1-K4** dll. 1д
4. **C.6 замер** `war3.exe` P1-P3 → `ns/тёрн` → cap дефолт `20000`. 0.5д
5. **E2E** 5 игроков, P3 `kill -9` на 5 мин → рестарт → `<60s` (`<30s` для 60 мин). 1д

Верификация:
```
cargo test -p spectre-engine --lib
cargo test
python .tmp_ida_verify/verify.py  # P1-P3, тик, chain
# приёмка G: золото/уровень/предметы/позиция P3 == логу сервера, 4 остальных без десинка
```

---

## 10. Риски H / откаты

- Патчи за конфигом, P3 снимать после catch-up, mismatch → abort (другая версия dll).
- `+0x610==1` ложно триггерит реплей HUD — не писать `+0x610` никогда.
- `state6` skip если легальный — catch-up мгновенный (high-risk, сначала C.3).
- `FullHistory` cap 90 мин, иначе RAM рост.
- `>1ms/тёрн` → `144k*1ms=144s` → профилировать, не снижать cap.

---

*Конец фулл сводки. Дальше — агент с `--dangerously-skip-permissions` прогоняет §8 батник без пауз.*
