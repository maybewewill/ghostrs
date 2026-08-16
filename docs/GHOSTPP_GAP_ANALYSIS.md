# ghostrs ↔ GHost++: полный анализ различий

> Дата: 2026-08-16. Анализ сделан по фактическому коду в `crates/` (не по планам).
> Цель пользователя: **не менять игровую логику**, а улучшить сетевой код и общее качество кода.
> Отчёт разбит на: (A) потерянная функциональность, (B) отличия в логике/поведении,
> (C) сетевые дефекты и качество кода, (D) мёртвый код и заглушки, (E) план исправлений.

Условные обозначения:
- **[ФУНКЦ]** — потерянная функция GHost++ (нужно добавить).
- **[ЛОГИКА]** — поведение отличается от GHost++ (проверить, нужно ли совпадать).
- **[СЕТЬ]** — сетевая/код-качественная проблема (основная цель пользователя).
- **[МЁРТВОЕ]** — мёртвый код, неиспользуемые конфиги, заглушки.

---

## A. Потерянная функциональность (чего не хватает против GHost++)

### A1. In-game команды (`crates/ghost-engine/src/chat.rs`)

GHost++: ~62 команды в игре (`game.cpp:396-1782`). ghostrs реализует ~35.

| Команда GHost++ | В ghostrs | Статус |
|---|---|---|
| `!start` / `!startforce` | есть (`s`, `sf`) | **[ЛОГИКА]** `force` парсится, но игнорируется (`Start { .. }` в `run_command`); порог старта — 1 человек вместо 2 |
| `!abort` | есть | **[ЛОГИКА]** доступна всем игрокам (`public_cmd`), в GHost++ — только хост |
| `!open/!close/!swap/!hold/!kick/!ping` | есть | ок |
| `!ban/!unban/!checkban/!banlast` | есть | **[ФУНКЦ]** `!ban` не пишет бан в БД и не отвечает деталями; `!unban`/`!checkban` — просто эхо, БД не трогают |
| `!addadmin/!deladmin/!checkadmin` | есть | **[МЁРТВОЕ]** заглушки-эхо; таблица `admins` в store не используется |
| `!mute/!unmute/!muteall/!unmuteall` | есть | ок |
| `!votestart` | есть | **[ЛОГИКА]** нужный порог = n/2+1; `votekick_percentage` из конфига не читается; голоса не сбрасываются при abort |
| `!synclimit/!latency/!version` | есть | ок |
| `!sp` (shuffle) | есть | ок |
| `!say` | есть | ок (в игру) |
| `!w/!whisper` | есть | **[ФУНКЦ]** не шлёт реальный whisper — просто эхо самому себе |
| `!stats/!statsdota` | есть | **[ФУНКЦ]** читают только in-memory статистику текущей игры; из БД ничего не читается (см. A4) |
| `!drop/!draw/!hcl/!owner/!unhost` | есть | ок |
| `!comp`, `!comps`, `!load`, `!save`, `!admin`, `!map` (смена карты), `!closeall`, `!openall`, `!end`, `!rehost`, `!games`, `!last` и др. | **нет** | **[ФУНКЦ]** не реализованы |
| `!load` (сохранённые игры) | в `ghost-legacy-attic` | **[ФУНКЦ]** crate не входит в workspace — мёртвый код |

Ключевое: подсистема администрирования (баны/админы/статы) в игре — **заглушки**, не связана со `Store`.

### A2. BNET-команды (`crates/ghostrs/src/supervisor.rs`)

GHost++: ~45 команд (`bnet.cpp:1191-2103`). ghostrs: 12 (`pub, priv, map, autohost, unhost, start, say, ban, unban, checkban, stats, status`).

**[ФУНКЦ]** Отсутствуют, например: `!host`, `!rehost`, `!close`/`!open` (закрыть игру), `!end`, `!whisper`/`!w`, `!admin`, `!addadmin`/`!deladmin`, `!load`, `!save`, `!games`, `!game`, `!last`, `!relay`, `!replay`, `!dload`/`!dloadprio`, `!down`, `!up`, `!fetchap`, `!motd`, `!version`, `!mute`/`!unmute` (на bnet), `!sm`, `!lock`, `!sw`, `!limit`, `!latency`, `!war3version`…

**[ЛОГИКА]** Команды из канала (не whisper) обрабатываются только если отправитель — root admin. У GHost++ часть команд доступна обычным админам из БД (`admins`), а root'ам — всё.

**[ФУНКЦ]** `!stats`/`!statsdota` на bnet читает `get_dota_stats` из БД, но **ничто никогда не пишет** игры/статы в БД (см. A4) — всегда "No stats recorded".

### A3. Продвинутые системы GHost++

| Система | GHost++ | ghostrs |
|---|---|---|
| Savegame (`!load`, EnforcePID, сохранение матча) | `savegame.cpp` | в attic, не скомпилирован **[ФУНКЦ]** |
| Admin game | `game_admin.cpp` (1088 строк) | нет (явно выкинуто) |
| w3mmd-статистика | пишется в БД (`w3mmd` таблица) | `StatsW3MMD` есть в движке, но **в БД не пишется** **[ФУНКЦ]** |
| DotA-статистика | пишется в `dotagames`/`dotaplayers` | `StatsDotA` живёт в памяти, **в БД не пишется** **[ФУНКЦ]** |
| Загрузки карт | пишутся в таблицу `downloads` | `Store::record_download` есть, но не вызывается **[ФУНКЦ]** |
| MySQL backend | есть | нет (SQLite — осознанный выбор) |
| Локализация | `language.cpp` (1305 строк) | `lang.rs` (34 строки) — минимум |
| W3MMD | `stats_w3mmd.cpp` | есть базовая обработка действий |
| Autohost-таймер и автозапуск | `!autohost` + автозапуск пустых игр | есть только создание лобби, автозапуск без игроков отсутствует |

### A4. Персистентность (`ghost-store`)

`writer.rs` содержит готовые команды: `AddBan, RemoveBan, AddAdmin, RemoveAdmin, LogGame, LogDotAGame, RecordDownload` — **реально вызываются только `AddBan`, `RemoveBan` (из supervisor) и запросы**. `LogGame`/`LogDotAGame`/`RecordDownload`/`AddAdmin`/`RemoveAdmin` не вызываются нигде, кроме тестов.

**[ФУНКЦ]**: конец игры не логируется, DotA-статы не сохраняются (в `ghost.db` будут только баны), таблица `downloads` пустая, `!stats` на bnet всегда пуст.

---

## B. Отличия в логике/поведении ([ЛОГИКА])

1. **`!start` доступен всем игрокам** (`chat.rs`, `public_cmd` включает `Start` и `Abort`). В GHost++ старт/аборт — только хост; для всех остальных есть `!votestart`. **Потенциальная дыра: любой игрок может начать/прервать игру.**
2. **Порог старта**: ghostrs — `human_count >= 1` (даже без `force`), GHost++ — минимум 2 игрока (без `force`). `force` в ghostrs вообще не используется.
3. **`!votestart`**: нужный кворум n/2+1 вместо `votekick_percentage` (100% в конфиге iCCup). Голоса не очищаются после abort/старта.
4. **Слот-запросы (0x11–0x14)**: применяются только в `GamePhase::Lobby`; GHost++ применяет и во время countdown.
5. **Отличия в `is_owner`**: `owner == "BOT"` или пустой → все считаются владельцами — это повторяет GHost++ (когда владелец игры = имя бота), ок.
6. **Причины REJECTJOIN**: ghostrs шлёт `0x09 FULL` для всего (занято имя, нет слота, полная комната) и `0x0A STARTED` вне лобби. GHost++ различает `REJECTJOIN_FULL`, `REJECTJOIN_STARTED`, `REJECTJOIN_WRONGPASSWORD` и т.д. W3-клиент по-разному показывает ошибку — не критично, но отличается.
7. **`!draw`**: голоса считаются от `players.len()` (в игре virtual host уже удалён — ок), но после `!draw` при продолжении игры голоса не сбрасываются. GHost++ требует согласия всех — совпадает.
8. **Таймаут загрузки 60 c** и дроп не загрузившихся — есть, ок. Но нет реакции на **MapCheck-несоответствие** (клиент с другой версией карты): ghostrs только дропает тех, у кого карты нет и загрузки отключены; GHost++ активно кикает по несовпадению CRC/SHA1.
9. **Ping/pong**: `handle_pong` игнорирует `pong == 1` (спецзначение). GHost++ так же трактует 1 как "no ping". Ок.
10. **Хост-пинг раз в 5 c** — есть; GHost++ шлёт чаще/реже в зависимости от таймера — некритично.
11. **`autokick_ping`/`lc_pings`** (конфиг iCCup: autokick 400) — **не реализованы**: игроков с пингом выше лимита никто не кикает. **[ФУНКЦ]** (смежное с логикой).
12. **Countdown-шаг** — 500 мс (совпадает с GHost++), ок.
13. **`begin_loading`**: удаляет virtual host и шлёт `countdown_start/end` — совпадает с `game_base.cpp:3389`.
14. **`begin_playing`**: сбрасывает sync-счётчики; не шлёт финальный `SLOTINFO` — GHost++ тоже не шлёт.
15. **`HCL`**: применяется только при `start_countdown`; флаг `hcl_from_game_name` из конфига **не читается** (всегда парсится из имени). `Hcl::parse_from_gamename` берёт только первое слово с `-`.
16. **`!latency`** меняет период тика на лету — GHost++ аналогично; ок.
17. **`!synclimit`**: clamp 10..200, GHost++ позволяет до 1000 — мелочь.
18. **GameOver-детект по DotA-действиям** — есть (60 c после победы), ок.
19. **`start_players`** фиксируется в `begin_loading` — ок.

---

## C. Сетевые дефекты и качество кода ([СЕТЬ]) — основная цель

### C1. DotaTV-вьюеры: входной кодек не тот (сломан viewer-chat)

`crates/ghost-spectator/src/relay.rs` подключает зрителя через `ghost_net::spawn_conn`, а это `DualCodec`, который принимает только `0xF7` (W3GS) и `0xF8` (GPS). Протокол DotaTV — `0xFD` (`ghost_protocol::dotatv::DotaTvCodec`). Любой байт `0xFD` ресинк-логикой `DualCodec` **выбрасывается**.

Последствия:
- Зрительский чат (0x81 `CLIENT_CHAT`) физически не может дойти до релея: `handle_conn_event` ждёт `AnyFrame::W3gs` c id 0x81, но кодек никогда не выдаст такой фрейм из `0xFD`-байтов. Тест `inbound_client_chat_frame_broadcasts_spectator_chat` подделывает фрейм вручную и реальный путь не проверяет.
- C++ клиент `dotatv_client/` **отсутствует в репозитории** (есть только в планах) — проверить живой поток нечем.

**Фикс:** отдельный `spawn_dtv_conn` с `HeaderCodec<0xFD>` для порта релея (как планировалось в Task 7 parity-v2) или параметризация `spawn_conn` кодеком.

### C2. GProxy: GPS_INIT до REQ_JOIN теряется

`crates/ghost-engine/src/actor.rs` — обработчик `GPS_INIT` отвечает только если игрок уже **занят** (`players.by_conn_mut`). Но GProxy++-клиент шлёт `GPS_INIT` сразу после TCP-connect, **до** `REQ_JOIN` — в этот момент соединение ещё в `pending`, игрока нет.

При этом из `lobby.rs` убрали безусловную отправку `gps::init` при join (diff). Итого: клиент, приславший INIT до join, **не получит ни одного `GPS_INIT`** → reconnect-защита не активируется никогда.

**Фикс:** в `on_gps_frame(INIT)` искать игрока и в `pending` (запомнить и ответить после join), либо вернуть отправку `gps::init` в `handle_req_join` и в INIT-хендлере только выставлять флаг `gproxy = true`. Проверить против реального GProxy++-клиента (Varlock).

### C3. Релей: в игре-чат и GameOver не доходят до зрителей

- `RelayHandle::send_chat` существует, но **нигде не вызывается** — игровой чат (и чат хоста) не попадает в DotaTV-стрим.
- `RelayCmd::GameOver` существует, но **движок его не шлёт** (встречается только в тесте) — зрители никогда не узнают о конце игры, релей продолжает крутить пустой поток и держит `history` вечно.
- При `!draw`/`Over` фаза просто завершается — релею не сообщается.

**Фикс:** в `send_chat_all`/`handle_chat_to_host` слать `relay.send_chat(...)`, в `on_tick` при `GamePhase::Over` слать `RelayCmd::GameOver`.

### C4. Отправка кика: нет `HOST_KICK_PLAYER` (0x1C)

`!kick`/`!ban`/дроп в лобби просто помечают игрока `left` и шлют `PLAYER_LEAVE_OTHERS` остальным. GHost++ дополнительно шлёт самому игроку `W3GS_HOST_KICK_PLAYER` (0x1C) и закрывает его сокет (`SendKickPlayer`). W3-клиент без 0x1C может висеть в лобби с "you have left the game" неопределённо.

### C5. LAN-анонс (`W3GS_GAMEINFO`) с хардкодом

`supervisor.rs::broadcast_lan_game`:
- `slots_total = 12`, `slots_open = 12` — жёстко, не из карты/слотов;
- `map_game_type = [1,0,0,0]` — хардкод, не из `map_info.game_type`;
- `up_time` — ок.
GHost++ шлёт реальные `m_Slots.size()`, число свободных слотов и game type карты. Игроки видят "12 слотов" и неправильный тип даже для 10-слотовой DotA.

### C6. BNCS: после реконнекта игра пропадает с баттл.нета

В `client.rs` цикл `'reconnect_loop` сбрасывает `active_advert = None` при каждом переподключении, и **стартовый адверт не пересылается** после нового логина (только если supervisor заново пошлёт `CreateGame`, а он не шлёт). LAN-анонс продолжается, на BNCS — игра исчезает навсегда.

**Фикс:** после `LoggedIn` переотправить активный адверт (supervisor держит `current_game_advert`, но не пересоздаёт его для bnet-клиента).

### C7. Отсутствие UDP-командного канала

Конфиг содержит `udp_cmd_port = 6969` и `udp_broadcast_target = "13.36.52.2"` — оба **не используются**. GHost++ слушает UDP-порт для удалённого управления (`!`-команды по UDP) и шлёт LAN-анонсы на сконфигурированный broadcast-адрес. Здесь broadcast всегда `255.255.255.255` (`UdpBroadcaster::bind`), поле `loopback` — мёртвое.

### C8. `PlayerLink::try_send` — мгновенный дроп по backpressure

`GameState::broadcast`/`send_to` при **одной** неудачной `try_send` помечают игрока как ушедшего (`left = Some(...)`). Для map-downloads (10×1442 байта за тик на канал 1024) медленный качальщик или короткий burst приведёт к вылету игрока, хотя GHost++ терпит очередь. Это осознанный дизайн ("не тормозить тик"), но:
- нет счётчика последовательных сбоев (drop только после N подряд);
- причина не различается (Backpressure vs Closed) — можно не кикать при Backpressure, а только при Closed.

### C9. PLAYERINFO: порт в sockaddr = 0

`outgoing::player_info` пишет `u16_le(0)` вместо порта игрока в обоих sockaddr. GHost++ заполняет порт (`GetExternalIP` + port). SLOTINFOJOIN порт шлёт правильно, а PLAYERINFO — нет; в лобби с большим числом игроков это может путать NAT-детект клиента. Мелочь, но отличается.

### C10. Мелкие сетевые

- `conn.rs` reader: `Some(Err(BadValue)) => continue` — безопасно (кодек сам двигает буфер), ок.
- Нет per-connection лимита скорости/размера очереди для map download (см. C8).
- Нет idle-таймаута на соединение (положился на игровые пинги) — как у GHost++, ок.
- `spawn_conn` не передаёт `external_ip` в link — ок, передаётся отдельно.
- `handle_new_connection` делает async `is_banned` до `spawn_conn` — ок, но при этом `conn_id` уже выделен, а если игра умерла — соединение молча закрывается (debug-лог), клиент не получает REJECTJOIN — в GHost++ шлётся отказ.

---

## D. Мёртвый код, неиспользуемые конфиги, заглушки ([МЁРТВОЕ])

1. **Конфиг (`ghost.toml` / `default.cfg`)**: `allow_downloads`, `max_downloaders`, `max_download_speed`, `autokick_ping`, `lc_pings`, `votekick_allowed`, `votekick_percentage`, `hcl_from_game_name`, `udp_broadcast_target`, `udp_cmd_port`, `server_alias`, `pvpgn_realm_name`, `exe_version` — **не читаются кодом** (встречаются только в README/ghost.toml). `BotConfig` не имеет полей для большинства из них.
2. **Загрузки**: `map.data` всегда `Some` для распарсенной карты → `allow_downloads` не влияет; `max_downloaders` не ограничивает; `max_download_speed` не ограничивает (всегда 10 чанков/тик/игрок).
3. **`Store`**: `LogGame`, `LogDotAGame`, `RecordDownload`, `AddAdmin`, `RemoveAdmin` — не вызываются.
4. **`ghost-bnet`**: `notifyjoin`, `checkad`, `logon_response`, `logon_response2`, `account_logon`, `account_logon_proof` — не используются (часть дублируется в `client.rs` через `auth_accountlogon*`).
5. **`ghost-net::udp::UdpBroadcaster.loopback`** — поле не используется.
6. **`ghost-legacy-attic`** — crate не в workspace (dead code).
7. **`lang.rs`** — `player_joined`, `player_left`, `countdown` не используются (чаты не шлют эти строки).
8. **`players.rs::next_free_colour`** — не используется.
9. **`ChatCommand::Unknown`** — только debug-лог, игроку не отвечает "неизвестная команда" (GHost++ отвечает).
10. **`SpectatorConfig`/`GameDefaults` поля** — `history_max_mb` используется, остальное ок.
11. **`RelayCmd::ViewerLeft`** — шлётся только из тестов (в бою релей узнаёт о закрытии из `ConnEvent::Closed`).

---

## E. План исправлений (приоритизированный)

Приоритет 1 — сетевые дефекты, ломающие реальные сценарии (главная цель):

1. **[СЕТЬ]** DotaTV-порт релея: `spawn_dtv_conn` с `HeaderCodec<0xFD>` (или параметр кодека в `spawn_conn`). Добавить E2E-тест с реальным TCP-сокетом: зритель шлёт `0xFD 0x81` — релей должен получить чат и разослать.
2. **[СЕТЬ]** GProxy: ответ на `GPS_INIT` из `pending`-очереди (запомнить INIT до join, ответить после join) + вернуть отправку `gps::init` в `handle_req_join`. Тест: INIT до REQ_JOIN → клиент получает init.
3. **[СЕТЬ]** Релей: форвардить игровой чат (`relay.send_chat`) из `send_chat_all` и `handle_chat_to_host`; слать `RelayCmd::GameOver` при `GamePhase::Over`/`finished`.
4. **[СЕТЬ]** `HOST_KICK_PLAYER` (0x1C) при кике/бане/дропе + закрытие сокета кикнутого.
5. **[СЕТЬ]** LAN `GAMEINFO`: реальные `slots.len()`, свободные слоты, `map.game_type`; убрать хардкод 12/12/`[1,0,0,0]`.
6. **[СЕТЬ]** BNCS: переотправка активного адверта после `LoggedIn` (реконнект).
7. **[СЕТЬ]** `broadcast`/`send_to`: различать `Backpressure` (не кикать сразу, счётчик N подряд) и `Closed` (кикать сразу).
8. **[СЕТЬ]** PLAYERINFO: порт игрока в sockaddr вместо 0.

Приоритет 2 — логика/безопасность (без изменения игровой механики):

9. **[ЛОГИКА]** `!start`/`!abort` — только хост (убрать из `public_cmd`), `force` — использовать или убрать парсинг.
10. **[ЛОГИКА]** `!ban/!unban/!checkban/!banlast/!addadmin/!deladmin/!checkadmin` — подключить к `Store` (в игре).
11. **[ФУНКЦ]** В конце игры писать `LogGame`/`LogDotAGame` в store (статы DotA уже парсятся движком — осталось сохранить). `!stats` на bnet заработает.
12. **[ФУНКЦ]** `autokick_ping`/`lc_pings`, `votekick_allowed`/`votekick_percentage`, `hcl_from_game_name` — прочитать из конфига и применить.
13. **[ФУНКЦ]** `allow_downloads`/`max_downloaders`/`max_download_speed` — прокинуть в `GameConfig`/`map.data` и в `pump_downloads`.
14. **[ЛОГИКА]** Причины REJECTJOIN (STARTED/ALREADYINGAME) — различать.
15. **[ФУНКЦ]** Replay: записывать игровой чат игроков (сейчас только чат хоста) и реальный host pid вместо хардкода `1`.

Приоритет 3 — чистота кода:

16. **[МЁРТВОЕ]** Удалить/задействовать: `loopback` в UdpBroadcaster, неиспользуемые функции bnet (`notifyjoin`, `checkad`, `logon_response*`), `lang.rs`-строки, `next_free_colour`, `ghost-legacy-attic` (в workspace или удалить).
17. **[МЁРТВОЕ]** Выпилить из ghost.toml/README неиспользуемые ключи или реализовать их; оставить только реально работающие.
18. **[МЁРТВОЕ]** `!whisper` — реализовать реальный whisper (через BNCS `SID_CHATCOMMAND` `/w`) или убрать.
19. **[ФУНКЦ]** BNET-команды (A2): добавить хотя бы `!host`, `!end`, `!games`, `!close`/`!open`, `!whisper` — минимальный набор для паритета с iCCup-сценарием.
20. **[СЕТЬ]** Решение по нерабочему клиенту `dotatv_client` (в репо его нет) — либо вернуть, либо убрать упоминания из планов.

Каждый пункт — по TDD: сначала падающий тест (для сетевых — с реальными TCP-сокетами), потом фикс, потом `cargo test --workspace`.
