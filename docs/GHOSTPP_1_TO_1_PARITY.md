# ghostrs ↔ GHost++: сверка 1:1 по исходникам (файл-в-файл)

> Дата: 2026-08-16. Сверка сделана **по фактическим исходникам**:
> референс `C:\Users\slash\iccwc3_work\ref\ghostpp\ghost\` (61 файл) против кода в `crates/`.
> Все расхождения ниже подтверждены чтением кода обеих сторон (номера строк GHost++ указаны).
> Цель: **привести к 1:1 (с улучшениями)**. Сначала вайр-паритет (байты на проводе),
> потом поведение, потом данные. Улучшения ghostrs (tokio-актор, zero-copy, кодек-ресник,
> MPQ-парсинг, чистая крипта, WAL, DotaTV) — сохраняются и не противоречат паритету.

---

## 0. Как читать документ

- **Блок A** — маппинг «файл GHost++ → модуль ghostrs → статус».
- **Блок B** — проверенные расхождения, которые **ломают 1:1** (обязательны к исправлению), с планом «как исправить».
- **Блок C** — расхождения «поведенческие» (команды, права, геймплей-логика).
- **Блок D** — расхождения «данные» (БД, статы, баны).
- **Блок E** — что ghostrs уже сделал **лучше** GHost++ (не трогать).
- **Блок F** — итоговый план работ по приоритетам.

---

## A. Маппинг файлов GHost++ → ghostrs

| # | GHost++ файл | ghostrs модуль | Статус |
|---|---|---|---|
| 1 | `ghost.cpp/h` (CGHost: главный цикл, конфиг, реконнекты, CreateGame, spoof-очередь) | `ghostrs/src/supervisor.rs`, `ghostrs/src/config.rs` | ⚠️ частично: нет spoof-check очереди, нет UDP-reconnect-порта, нет глобального счётчика host counter |
| 2 | `bnet.cpp/h` (BNET-клиент + ~45 команд) | `ghost-bnet/src/client.rs`, `ghostrs/src/supervisor.rs` | ⚠️ ~12 из ~45 команд; нет админов из БД; нет spoof; нет rehost |
| 3 | `bnetprotocol.cpp/h` (BNCS пакеты) | `ghost-protocol/src/bncs/*` | ✅ почти всё (auth, logon, chat, startadvex3, getadvlistex). ❌ `notifyjoin`, `checkad`, `logon_response*` — написаны, но не вызываются |
| 4 | `bncsutilinterface.cpp/h` | `ghost-bnet/src/bncsutil/*` | ✅ чистый Rust (без DLL) |
| 5 | `bnlsclient.cpp/h`, `bnlsprotocol.cpp/h` (fallback-авторизация через BNLS) | — | ➖ не нужен: ghostrs сам считает CheckRevision/NLS |
| 6 | `commandpacket.cpp/h` (байтовый поток → команды) | `ghost-net/src/conn.rs` (FramedRead/DualCodec) | ✅ аналог, улучшен |
| 7 | `config.cpp/h` (+ `map.cfg` на карту) | `ghostrs/src/config.rs` | ✅ конфиг есть; `map.cfg` заменён MPQ-парсингом (улучшение) |
| 8 | `crc32.cpp/h` | crc32fast (workspace) | ✅ |
| 9 | `csvparser.cpp/h` | — | ➖ не используется (админы/карты не грузятся из CSV) |
| 10 | `game.cpp/h` (CGame: команды, статы, баны в БД) | `ghost-engine/src/chat.rs`, `actor.rs` | ⚠️ ~35 из ~60 команд; статы/баны в БД не пишутся |
| 11 | `game_admin.cpp/h` | — | ❌ выкинуто осознанно (AdminGame) |
| 12 | `game_base.cpp/h` (CBaseGame — ядро) | `ghost-engine/src/{state,actor,actions,lobby,chat,mapxfer,lagcheck,gproxy}.rs` | ⚠️ см. Блок B — вайр-различия |
| 13 | `gameplayer.cpp/h` (CGamePlayer/CPotentialPlayer) | `ghost-engine/src/players.rs` | ⚠️ нет spoof/whois/checksum/score/reserved-enforce/load-in-game |
| 14 | `gameprotocol.cpp/h` (W3GS пакеты) | `ghost-protocol/src/w3gs/*` | ✅ почти всё. ❌ не обработан `W3GS_MAPPARTNOTOK (0x45)`; не используются CHAT_OTHERS/PING_FROM_OTHERS/PONG_TO_OTHERS/SEARCHGAME/CREATEGAME/REFRESHGAME/DECREATEGAME |
| 15 | `gameslot.cpp/h` (CGameSlot, MAX_SLOTS=24) | `ghost-protocol/src/w3gs/slot.rs`, `ghost-engine/src/slots.rs` | ✅ |
| 16 | `ghostdb.cpp/h`, `ghostdbsqlite.cpp/h` (9 таблиц) | `ghost-store/src/*` | ⚠️ схема есть, пишутся только баны |
| 17 | `ghostdbmysql.cpp/h` | — | ➖ не нужен (SQLite) |
| 18 | `gpsprotocol.cpp/h` | `ghost-protocol/src/gps/mod.rs` | ✅ |
| 19 | `language.cpp/h` (1305 строк, ~250 строк) | `ghost-engine/src/lang.rs` (34 строки) | ⚠️ минимум; GHost++ шлёт осмысленные сообщения на каждом шаге |
| 20 | `map.cpp/h` (CMap + map.cfg) | `ghost-engine/src/map.rs` | ⚠️ layout_style совпадает ✅; game type/flags — GHost++ из map.cfg, ghostrs хардкодит часть |
| 21 | `packed.cpp/h` (CPacked: заголовок, CRC, 8192-блоки) | `ghost-spectator/src/w3g.rs` | ✅ совпадает байт-в-байт (проверено) |
| 22 | `replay.cpp/h` (CReplay: BuildReplay, Timeslot) | `ghost-spectator/src/body.rs` | ⚠️ см. B5–B8 (0x1F vs 0x1E, CRC-байты, loading-блоки, host PID) |
| 23 | `savegame.cpp/h` | `ghost-legacy-attic/src/savegame.rs` | ❌ crate вне workspace — мёртвый |
| 24 | `sha1.cpp/h` | crate `sha1` | ✅ |
| 25 | `socket.cpp/h` (select-цикл) | `ghost-net/*` (tokio async) | ✅ улучшение |
| 26 | `stats.cpp/h`, `statsdota.cpp/h`, `statsw3mmd.cpp/h` | `ghost-engine/src/stats_dota.rs`, `stats_w3mmd.rs` | ⚠️ парсинг ✅; `Save()` в БД ❌ |
| 27 | `util.cpp/h` (UTIL_*) | `ghost-protocol/src/bytes_ext.rs` и помощники | ✅ |
| 28 | `next_combination.h` (балансировка слотов) | — | ❌ `!sp` есть, `BalanceSlots` нет |
| 29 | `w3g_actions.txt`, `w3g_format.txt` | комментарии в `body.rs`/`w3g.rs` | ✅ справочник |
| 30 | `sqlite3.c/h` | rusqlite (bundled) | ✅ |

---

## B. Вайр-паритет: проверенные расхождения, которые ломают 1:1

### B1. MAP_PART / START_DOWNLOAD: `fromPID` = хост, а не 255

- **GHost++**: `game_base.cpp:631` `SEND_W3GS_MAPPART( GetHostPID(), pid, ... )`; `:3210` `SEND_W3GS_STARTDOWNLOAD( GetHostPID() )`.
  `GetHostPID()` (`:3749`): virtual host PID → fake player → владелец → первый игрок.
- **ghostrs**: `mapxfer.rs` `map_part(255, d.pid, ...)` и `start_download(255)`.
- **Как исправить**: в `mapxfer.rs` использовать `self.host_pid()` вместо 255 (уже есть в `state.rs`).

### B2. Порог переполнения INCOMING_ACTION → INCOMING_ACTION2: 1452, а не 1400

- **GHost++**: `game_base.cpp:1373` — `if (SubActionsLength + Action->GetLength() > 1452)` с комментарием «1452 because the INCOMING_ACTION and INCOMING_ACTION2 packets use an extra 8 bytes».
- **ghostrs**: `actions.rs` `MAX_ACTION_PAYLOAD = 1400`.
- **Как исправить**: `MAX_ACTION_PAYLOAD = 1452` в `actions.rs` (комментарий в GHost++ объясняет, почему).

### B3. Countdown: 10 шагов (10…1, 5 с), а не 5 шагов (2,5 с)

- **GHost++**: `m_CountDownCounter = 10` (`game_base.cpp:4497, 4577, 4669`); в апдейте раз в 500 мс шлёт `"N. . ."` и декрементит (`:709-716`), при 0 → `EventGameStarted()`.
- **ghostrs**: `state.rs` `COUNTDOWN_STEPS = 5` → 2,5 с. Комментарий ghostrs «пять шагов = 2,5 с» — неверное прочтение GHost++.
- **Как исправить**: `COUNTDOWN_STEPS = 10` (и `COUNTDOWN_TOTAL` = 5 с). Первый шаг будет «10. . .».

### B4. HCL кодируется при старте загрузки (EventGameStarted), а не при старте отсчёта

- **GHost++**: `EventGameStarted` (`game_base.cpp:3313-3367`) — HCL в handicap'ы слотов, потом `SendAllSlotInfo()`.
- **ghostrs**: `actor.rs::start_countdown` вызывает `encode_hcl_into_slots` сразу при `!start`.
- **Как исправить**: перенести вызов в `begin_loading()` (эквивалент EventGameStarted). Кодировка и `send_all_slot_info` уже совпадают по алгоритму (`hcl.rs` ↔ `:3326-3367` — сверено ✅).

### B5. Replay: основной timeslot = блок 0x1F, overflow = 0x1E

- **GHost++**: `replay.h` `REPLAY_TIMESLOT2 = 0x1E` («corresponds to W3GS_INCOMING_ACTION2»), `REPLAY_TIMESLOT = 0x1F` («corresponds to W3GS_INCOMING_ACTION»). В `game_base.cpp:1384` overflow пишется `AddTimeSlot2` (0x1E), основной `:1402/1415` — `AddTimeSlot` (0x1F).
- **ghostrs**: `body.rs` `REPLAY_TIMESLOTBLOCK = 0x1E` используется **для обоих**.
- **Как исправить**: в `body.rs` завести `REPLAY_TIMESLOT = 0x1F` и `REPLAY_TIMESLOT2 = 0x1E`; `add_timeslot2()` для overflow, `add_timeslot()` для основного.

### B6. Replay: в timeslot не должно быть CRC-байтов

- **GHost++**: `replay.cpp AddTimeSlot` пишет `[pid][u16 len][action]` без CRC (CIncomingAction хранит CRC отдельно и в реплей его не кладёт).
- **ghostrs**: `actions.rs` `rep.add_timeslot(interval, &b[6..])`, а `b[6..8]` — это 2 байта CRC из INCOMING_ACTION. **Лишние 2 байта на каждый timeslot** → реплей рассинхронизируется при чтении клиентом (pid=crc_lo, len=crc_hi…).
- **Как исправить**: парсить payload в ActionBlock'и (pid+len+action) и писать их без CRC, либо слайсить `&b[8..]` и разбирать (лучше — по ActionBlock'ам, как GHost++).

### B7. Replay: host PID — реальный virtual host, а не хардкод 1

- **GHost++**: `BuildReplay` пишет `m_HostPID` (это PID виртуального хоста, `game_base.cpp` `m_Replay->SetHostPID(m_VirtualHostPID)`).
- **ghostrs**: `state.rs::new` `ReplayBody::new(1, ...)` — всегда 1.
- **Как исправить**: создавать `ReplayBody` после создания виртуального хоста или передавать `virtual_host_pid` при старте (в `begin_playing`/`GameState::new` через `cfg`).

### B8. Replay: ливеры во время загрузки пишутся между 0x1B и 0x1C

- **GHost++**: `AddLeaveGameDuringLoading` (`replay.cpp`) — блоки уходят в `m_LoadingBlocks`, которые `BuildReplay` вставляет между SECOND и THIRD start block'ами (`replay.cpp`).
- **ghostrs**: `add_leaver` всегда в основной поток блоков (после 0x1C).
- **Как исправить**: в `reap_left_players` различать фазу `Loading` и писать ливер в loading-блоки (`ReplayBody::add_leaver_loading` → между 0x1B/0x1C).

### B9. STARTDOWNLOAD: у GHost++ карту качают от PIDs хоста — уже в B1.

### B10. LAN GAMEINFO: реальные слоты/тип/entry key

- **GHost++**: `SEND_W3GS_GAMEINFO` (gameprotocol.cpp) получает реальные `slotsTotal`, `slotsOpen`, `mapGameType`, `mapFlags/width/height/crc/path`, `hostCounter`, `entryKey` (entry key = `m_EntryKey = rand()`, `game_base.cpp:46`).
- **ghostrs**: `supervisor.rs::broadcast_lan_game` — хардкод `slots_total=12`, `slots_open=12`, `map_game_type=[1,0,0,0]`, `entry_key=0`.
- **Как исправить**: брать из `map_info.num_players`, свободных слотов лобби, `map.game_type` (с сохранением `MAPGAMETYPE_UNKNOWN0` для LAN-байпаса, если он реально нужен на iCCup — проверить), `entry_key = rand()`.

### B11. Глобальный host counter и random seed

- **GHost++**: `m_HostCounter = m_GHost->m_HostCounter++` (бот-глобальный инкремент, `game_base.cpp:46`); `m_RandomSeed = GetTicks()`.
- **ghostrs**: host counter — случайные 28 бит; random seed — `rand::random()`.
- **Как исправить**: завести в `Supervisor` счётчик `host_counter: u32 += 1` и прокидывать в `GameConfig`; random seed можно оставить (клиенту важен лишь факт рандома) — для 1:1 взять `now.as_millis() as u32`.

### B12. Троттлинг загрузки карты: max_downloaders + max_download_speed

- **GHost++**: `game_base.cpp:586-638`: цикл раз в 100 мс; лимит `m_MaxDownloaders` (глобально), до 100 частей (≈140 КБ) за цикл на игрока, глобальный `m_DownloadCounter` против `m_MaxDownloadSpeed * 1024` в секунду.
- **ghostrs**: `mapxfer.rs::pump_downloads` — 10 частей/тик на игрока, лимитов нет; конфиг `max_downloaders`/`max_download_speed` не читается.
- **Как исправить**: прокинуть `max_downloaders`/`max_download_speed`/`allow_downloads` в `GameConfig`; в `pump_downloads` — глобальный счётчик байт/сек + лимит качальщиков + до 100 частей за цикл (цикл 100 мс, а не каждый тик).

### B13. Ping-пакеты: GHost++ шлёт в лобби+загрузке, останавливает после старта

- **GHost++**: пинги в апдейте (`m_LastPingTime`), прекращаются после загрузки игры.
- **ghostrs**: `actions.rs::on_tick` шлёт ping каждые 5 с **в любой фазе** (включая Playing).
- **Как исправить**: слать ping только в `Lobby | Countdown | Loading` (как у GHost++), иначе клиенты получают лишний пинг во время игры.

---

## C. Поведение: команды, права, геймплей

### C1. `!start`/`!abort` — только для владельца/админов, а не всем

- **GHost++**: `game.cpp:378-396` — команды только если `player->GetSpoofed() && (AdminCheck || RootAdminCheck || IsOwner(User))`, затем `!m_Locked || RootAdminCheck || IsOwner(User)`. `!abort` и `!start` — внутри этого блока.
- **ghostrs**: `chat.rs` — `Start` и `Abort` в `public_cmd` (доступны любому игроку). **Дыра безопасности.**
- **Как исправить**: убрать `Start`/`Abort` из `public_cmd`; оставить `!votestart` публичным. Опционально добавить `!lock`/`!unlock`.

### C2. `!start` должен проверять готовность (без force)

- **GHost++**: `StartCountDown(false)` (`game_base.cpp:4490-4580`):
  1. HCL слишком длинный → отказ + «use force»;
  2. кто-то ещё качает карту → список имён;
  3. (при `m_RequireSpoofChecks`) не прошедшие spoof-check → список;
  4. не набрано ≥3 пингов у не-reserved → список;
  5. если всё чисто — старт.
  Плюс `game.cpp:1496-1516`: если игрок ушёл < 2 с назад — отказ.
  `!start force` — обходит всё (`StartCountDown(true)`).
- **ghostrs**: стартует сразу при `human_count >= 1`, `force` парсится и игнорируется.
- **Как исправить**: реализовать `StartCountDown(force)`-эквивалент: проверки HCL/загрузки/пингов/2 с; `force` — обход.

### C3. Autokick по пингу

- **GHost++**: `EventPlayerPongToHost` (`game_base.cpp:3277-3292`): в лобби (не loading/loaded), не reserved, ≥3 пингов, `GetPing(LCPings) > m_AutoKickPing` → кик с сообщением.
- **ghostrs**: нет; конфиг `autokick_ping`/`lc_pings` не читается.
- **Как исправить**: в `mapxfer.rs::handle_pong` (или отдельной функции) считать пинги; при превышении в лобби — `left = Some(...)` + `send_chat_all`; уважать `reserved`.

### C4. Виртуальный хост: PID, fake player, reserved

- **GHost++**: `GetHostPID()` приоритет: virtual host → **fake player** → владелец → первый игрок (`:3749-3775`). Есть `CreateFakePlayer`/`DeleteFakePlayer` (`!fakeplayer`, `!fppause`/`!fpresume`).
- **ghostrs**: virtual host → первый игрок; fake player нет.
- **Как исправить**: добавить fake player (необязательно для iCCup-сценария, но это часть 1:1); как минимум повторить приоритет host PID без fake player.

### C5. Reserved-слоты (`!hold`) не принуждаются

- **GHost++**: `m_Reserved` (список имён), `GetEmptySlot(reserved)`, `IsReserved(name)` — резерв влияет на выдачу слота и на autokick.
- **ghostrs**: `holds: HashMap<slot, name>` только для вывода; при join не проверяется.
- **Как исправить**: при `handle_req_join` искать сначала слот из `holds` (по имени), потом свободный.

### C6. Причины ухода/кика

- **GHost++**: `OpenSlot(sid, kick=true)` (`game_base.cpp`) — кик через `SetDeleteMe + SetLeftReason + SetLeftCode(PLAYERLEAVE_LOBBY)`, **пакет 0x1C (HOST_KICK_PLAYER) в этом коде не шлётся** — проверено grep'ом. Т.е. ghostrs (без 0x1C) тут совпадает ✅.
- **Как исправить**: ничего; расхождения нет.

### C7. PLAYERINFO: порт в sockaddr

- **GHost++**: `SEND_W3GS_PLAYERINFO` (gameprotocol.cpp) — `packet.push_back(0); // port`. ghostrs тоже пишет 0 ✅. Расхождения нет.

### C8. Список in-game команд GHost++ (game.cpp) → статус в ghostrs

Полный набор GHost++ (`game.cpp` grep по `Command ==`): `abort/a, addban/ban, announce, autosave, autostart, banlast, check, checkban, clearhcl, close, closeall, comp, compcolour, comphandicap, comprace, compteam, dbstatus, download/dl, drop, end, fakeplayer, fppause, fpresume, from, hcl, hold, kick, latency, lock, messages, mute, muteall, open, openall, owner, ping, priv, pub, refresh, say, sendlan, sp, start, swap, synclimit, unhost, unlock, unmute, unmuteall, virtualhost, votecancel, w, checkme, stats, statsdota/sd, version, votekick, yes` — 61 команда.

Отсутствуют в ghostrs: `announce, autosave, autostart, check, checkban→БД, clearhcl, closeall, comp*, dbstatus, download/dl, end, fakeplayer, fppause, fpresume, from, lock, messages, openall, priv, pub, refresh, sendlan, unlock, virtualhost, votecancel, w (реальный whisper), checkme, votekick, yes`. Плюс `!ban`/`!unban`/`!checkban`/`!addadmin`/`!deladmin`/`!checkadmin`/`!stats` — не работают с БД (см. D).

### C9. BNET-команды (bnet.cpp) → статус в ghostrs

Полный набор: `accept, addadmin, addban/ban, autohost, autohostmm, channel, checkadmin, checkban, countadmins, countbans, dbstatus, deladmin, delban/unban, disable, enable, enforcesg, exit/quit, getclan, getfriends, getgame, getgames, grunt, hostsg, invite, load, loadsg, map, motd, peon, priv, privby, pub, pubby, reload, remove, say, saygames, shaman, unhost, wardenstatus, stats, statsdota/sd, version` — 46 команд.

В ghostrs: `pub, priv, map, autohost, unhost, start, say, ban, unban, checkban, stats, status` — 12. `status` вообще нет в GHost++ (своё). Отсутствуют: `accept, addadmin, autohostmm, channel, checkadmin, countadmins, countbans, dbstatus, deladmin, disable, enable, enforcesg, exit/quit, getclan, getfriends, getgame, getgames, grunt, hostsg, invite, load, loadsg, motd, peon, privby, pubby, reload, remove, saygames, shaman, wardenstatus, sd` и весь spoof/админ-контур.

### C10. Spoof-check (/whois) отсутствует

- **GHost++**: `m_SpoofChecks` (0/1/2), `WhoisShouldBeSent`, `/whois` через BNCS, `SetSpoofed`; команды требуют `GetSpoofed()`. LAN-игроки (`JoinedRealm.empty()`) считаются spoofed автоматически (`game_base.cpp:2070-2072`).
- **ghostrs**: нет. Для iCCup (PvPGN с `bnet_spoofchecks=0`?) можно просто считать всех spoofed — но тогда надо явно решить. **Как исправить**: добавить поле `spoofed`/`joined_realm` в `Player`, режим `spoof_checks: u8` в конфиг; при 0 — все spoofed.

---

## D. Данные (БД, статы, баны)

### D1. В БД пишутся только баны

- **GHost++**: `CGame::SaveGameData` → `AddGame/AddGamePlayer/AddDotAPlayer/AddW3MMD...` (`game.cpp`, `ghostdb.cpp`), плюс `record download` при окончании загрузки (`EventPlayerMapSize`/download finished), плюс `AddBan/AddAdmin` из команд.
- **ghostrs**: `Store` имеет `LogGame, LogDotAGame, AddAdmin, RemoveAdmin, RecordDownload`, но **никто их не вызывает** (только `ban/unban` из supervisor). `!stats` на bnet всегда «No stats recorded».
- **Как исправить**: в `actions.rs::on_tick` при `GamePhase::Over` собрать `dota/w3mmd` и игроков и отправить `StoreCmd::LogGame/LogDotAGame`; в `mapxfer.rs` по завершении загрузки — `RecordDownload`; `!ban` в игре — `StoreCmd::AddBan`.

### D2. Проверка банов при join

- **GHost++**: `EventPlayerJoined` (`game_base.cpp:2035-2046`) — проверка банов **по имени и IP** прямо в игре, с сообщениями в лобби.
- **ghostrs**: проверка только IP в `supervisor.rs::handle_new_connection` (до `spawn_conn`); по имени не проверяется.
- **Как исправить**: добавить проверку имени в `handle_req_join` (асинхронно через store или кэш банов), как у GHost++.

### D3. Статы в игре: квота и БД

- **GHost++**: `!stats`/`!statsdota` — квота 5 с на игрока (`GetStatsSentTime`), читает БД.
- **ghostrs**: `!stats` — заглушка; `!statsdota` — только текущая in-memory игра.
- **Как исправить**: после D1 статы появятся в БД; добавить квоту 5 с и чтение из store.

---

## E. Улучшения ghostrs (сохранить, не ломать)

1. Tokio-актор на игру, zero global locks.
2. Deadline-сетка тиков (нет дрейфа), `sleep_until`.
3. Zero-copy `Bytes`-широковещание (один буфер на всех).
4. Двойной кодек W3GS/GPS на одном сокете с ресинком, fuzz-тесты декодеров.
5. Чисто-растовая крипта (xsaha1, cdkey, checkRevision, NLS) — без bncsutil.dll.
6. MPQ-парсинг карт в движке (не нужен map.cfg; CRC/SHA1/слоты из файла).
7. SQLite WAL через отдельный актор.
8. DotaTV-релей с задержкой и историей (в GHost++ нет).
9. Сохранение реплеев в `spawn_blocking` (не блокирует актор).
10. `HostCounter` маска 28 бит, маппинг адреса из SID_GETADVLISTEX.

---

## F. План работ: 1:1 + улучшения

### Фаза 1 — вайр-паритет (критично, меняет байты на проводе)

1. **B1** — `mapxfer.rs`: `host_pid()` вместо 255 в `start_download`/`map_part`. Тест: золотой байт-вектор.
2. **B2** — `actions.rs`: `MAX_ACTION_PAYLOAD = 1452`.
3. **B3** — `state.rs`: `COUNTDOWN_STEPS = 10`; сообщения «10. . . … 1. . .»; обновить тесты.
4. **B4** — HCL: перенести из `start_countdown` в `begin_loading`.
5. **B5+B6** — `body.rs`/`actions.rs`: `add_timeslot` → 0x1F без CRC; `add_timeslot2` → 0x1E без CRC; пересобрать золотые тесты `replay`.
6. **B7** — host PID реплея = `virtual_host_pid`.
7. **B8** — loading-ливеры в окно между 0x1B/0x1C.
8. **B10** — LAN `GAMEINFO` из реальных слотов/типа + `entry_key`.
9. **B11** — глобальный host counter в `Supervisor`; random seed из времени.
10. **B12** — `max_downloaders`/`max_download_speed`/`allow_downloads` в конфиг и `pump_downloads`.
11. **B13** — пинги только в лобби/загрузке.

### Фаза 2 — поведение

12. **C1** — `!start`/`!abort` только для хоста/админов; `public_cmd` — только `votestart/ping/stats/version/draw`.
13. **C2** — `StartCountDown(force)`: проверки HCL/загрузки/пингов/2с после леава.
14. **C3** — autokick по пингу (`autokick_ping`, `lc_pings`).
15. **C5** — reserved-слоты при join.
16. **C8** — добавить команды: `closeall/openall, clearhcl, comp*, dl/download, end, lock/unlock, votecancel, votekick, yes, w (whisper), checkme, check, announce, autostart, priv/pub (rehost), refresh, sendlan, virtualhost`.
17. **C9** — BNET: `addadmin/deladmin/checkadmin/countadmins, delban, dbstatus, getgame/getgames, load/loadsg, enforcesg, motd, saygames, channel, disable/enable, exit/quit, getfriends/getclan, reload, accept, invite, hostsg, privby/pubby, autohostmm, wardenstatus`.
18. **C10** — spoof-check: поле `spoofed`, режим `spoof_checks`; LAN = spoofed.

### Фаза 3 — данные

19. **D1** — `LogGame/LogDotAGame/RecordDownload/AddAdmin` из движка при завершении игры и загрузок.
20. **D2** — проверка бана по имени при join.
21. **D3** — `!stats`/`!statsdota`: квота 5 с + чтение из БД; `!checkban`/`!ban` в игре через store.
22. `lang.rs` — вернуть осмысленные сообщения GHost++ для всех новых команд.

### Фаза 4 — улучшения (сверх 1:1)

23. Per-conn лимит очереди MAPPART + мягкий backpressure (не кикать с одного сбоя).
24. BNCS reconnect: авто-переадверт активной игры после `LoggedIn`.
25. DotaTV: отдельный кодек 0xFD для вьюеров; форвард игрового чата и GameOver в релей.
26. GPS: буфер от первого пакета (уже сделан), reconnect через глобальную очередь с матчингом по ключу (опционально).
27. Регрессионные золотые тесты на каждый пункт фазы 1 (byte-vectors из GHost++).

---
*Каждый пункт — по TDD: сначала падающий тест (для вайр-паритета — золотые байтовые векторы), затем фикс, затем `cargo test --workspace`.*
