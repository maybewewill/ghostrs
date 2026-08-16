# ghostrs ↔ GHost++: спецификация исправлений

> Составлена 2026-08-16 по результатам построчной сверки исходников
> `C:\Users\slash\iccwc3_work\ref\ghostpp\ghost\` против `crates/`.
> Каждый пункт подтверждён чтением обеих сторон; номера строк указаны.
>
> **Метод работы: TDD.** На каждый пункт сначала падающий тест
> (для вайр-паритета — золотой байтовый вектор), затем фикс, затем
> `cargo test --workspace` целиком зелёный.
>
> **Не ломать существующие улучшения ghostrs**: tokio-актор на игру,
> zero-copy `Bytes`, deadline-сетка тиков, MPQ-парсинг карт, чистая крипта
> без bncsutil.dll, SQLite WAL через актор, DotaTV-релей.

## Принятые решения (не пересматривать)

1. **Порты**: `gproxy_reconnect_port` = 6114 (дефолт, как в ghostpp),
   `spectator_port` = 6115 (дефолт). Оба конфигурируемые в `ghost.toml`.
2. **Схема БД**: остаётся собственная (`game_players`, `game_id`, `started`).
   Совместимость с `update_dota_elo.exe` не требуется. Пункт N24 — won't-fix.
3. **Параметры карты**: парсить из w3x (MPQ), плюс необязательный override
   в `ghost.toml` на карту. `map.cfg` из ghostpp не возвращать.

---

# Фаза P0 — рвёт реальную игру

## P0-1. Детект десинка отсутствует (N11)

- **GHost++**: `game_base.cpp:2762-2890`. Каждый игрок копит очередь чек-сумм
  (`CGamePlayer::m_CheckSums`, `gameplayer.h:89`). Когда у всех непустая очередь,
  берётся `FirstCheckSum` от первого игрока; все, у кого `front()` отличается,
  бинуются по значению (`Bins[checksum].push_back(pid)`); объявляется десинк,
  меньшинство дропается. В конце цикла у всех `pop()`.
- **ghostrs**: `ghost-engine/src/actions.rs:28-35` — `handle_keepalive` декодирует
  пакет и **выбрасывает** чек-сумму, инкрементя только `sync_counter`.
  `ghost-protocol/src/w3gs/incoming.rs:131-135` `decode_keepalive` уже возвращает
  чек-сумму — её просто никто не использует.
- **Сделать**: добавить `checksums: VecDeque<u32>` в `Player` (`players.rs`),
  складывать туда значение из `decode_keepalive`. В `on_tick` (или отдельной
  `check_desync`) повторить алгоритм GHost++: пока у всех очередь непуста —
  сравнить головы, при расхождении сгруппировать по значению, залогировать,
  сообщить в чат, дропнуть меньшинство, затем `pop_front()` у всех.
  Ограничить длину очереди (GHost++ по факту не ограничивает — взять разумный
  предел, напр. 512, и при переполнении считать игрока отставшим).
- **Тест**: два игрока шлют одинаковые чек-суммы → никто не дропнут; третий шлёт
  другую → он помечен `left`, остальные живы.

## P0-2. Лаг-скрин не переподнимается (N12)

- **GHost++**: `game_base.cpp:923` — пока идёт лаг, каждые **60 секунд**
  (`m_LastLagScreenResetTime`) заново шлётся STOP_LAG по каждому лаггеру и
  затем START_LAG. При `m_LoadInGame` — интервал 30 с (`:770`). Причина: клиент
  Warcraft III сам рвёт соединение, если лаг-экран висит без обновления.
  Точки обновления таймера: `:851`, `:896`, `:974`, `:3379`.
- **ghostrs**: `ghost-engine/src/lagcheck.rs` — START_LAG шлётся один раз при
  подъёме экрана, переподъёма нет.
- **Сделать**: поле `last_lag_screen_reset: Instant` в `GameState`. Пока
  `self.lagging`, раз в 60 с: на каждого лаггера `stop_lag(pid, elapsed)`, затем
  один `start_lag(&laggers)` с **накопленным** временем лага на каждого
  (`GetTicks() - StartedLaggingTicks`, см. `gameprotocol.cpp:595`), не с нулём.
- **Тест**: игрок лагает >60 с (время подменяемое) → в поток ушла вторая пара
  STOP_LAG/START_LAG, причём в START_LAG время лага != 0.

## P0-3. GProxy++ не работает: нет reconnect-листенера (N1)

- **GHost++**: `ghost.cpp:824-1023`. Отдельный `CTCPServer m_ReconnectSocket` на
  `bot_reconnectport` (6114), принимает **новые** сокеты, читает `GPS_RECONNECT`,
  ищет игру по ключу/PID, отдаёт сокет в игру. Неподобранные висят в буфере
  pending и чистятся по таймауту (`:1023`). Конфиг: `bot_reconnect`,
  `bot_reconnectport`, `bot_reconnectwaittime` (`ghost.cpp:484-485, 1249`).
- **ghostrs**: `ghost-engine/src/actor.rs:157` обрабатывает `gps::RECONNECT`
  только на **уже существующем** сокете игры. При реальном обрыве такого сокета
  нет → реконнект невозможен в принципе.
- **Сделать**: в `ghostrs/src/supervisor.rs` поднять TCP-листенер на
  `gproxy_reconnect_port`. На новом соединении ждать `GPS_RECONNECT`
  (pid + reconnect_key + last_packet_ack). Найти игру, у которой есть игрок с
  таким pid и совпадающим `reconnect_key` и `disconnected_since.is_some()`.
  Передать соединение в актор игры новой командой (напр.
  `GameCmd::AdoptReconnect { pid, key, link, last_ack }`), там: заменить
  `link` игрока, снять `disconnected_since`, отдать `GPS_RECONNECT_OK`
  и дослать из `GProxyBuffer` всё после `last_ack`. Не подобранные за
  `reconnect_wait` соединения закрывать.
- **Порт**: `gproxy_reconnect_port` в `[bot]`, дефолт 6114. Одновременно
  перенести `spectator_port` на дефолт 6115 (`ghostrs/src/config.rs:114,459,595`).
- **Тест**: интеграционный на реальных TCP-сокетах — игрок с gproxy отключается,
  новое соединение на порт реконнекта с верным ключом получает `RECONNECT_OK`
  и буферизованные пакеты; с неверным ключом — соединение закрыто.

## P0-4. GProxy empty actions = 0 (N13)

- **GHost++**: `game_base.cpp:61-66` — `m_GProxyEmptyActions = m_ReconnectWaitTime - 1`,
  клемп сверху до 9. Число уходит клиенту в GPS_INIT. На лаг-циклах шлётся
  столько же пустых `INCOMING_ACTION` (`:804-848`, `:944`), и
  `m_SyncCounter += m_GProxyEmptyActions` (`:848`). Ожидание при лаге считается
  как `( m_GProxyEmptyActions + 1 ) * 60` (`:913`).
- **ghostrs**: `ghost-engine/src/actor.rs:140-145` — `gps::init(6113, pid, key, 0)`,
  пустые действия не шлются вовсе.
- **Сделать**: вычислить `gproxy_empty_actions` из `reconnect_wait` по формуле
  GHost++, передавать в `gps::init` вместо 0, слать соответствующее число пустых
  `incoming_action(&[], 0)` в тех же точках, что GHost++, и увеличивать
  `sync_counter` на то же число. Первым аргументом `gps::init` идёт порт —
  проверить, что это реально `host_port` из конфига, а не литерал 6113.
- **Внимание**: `bot_reconnectwaittime` у GHost++ задан в **минутах** (дефолт 3),
  а `reconnect_wait_sec` у нас — в секундах. 180 с = 3 мин → **2** пустых действия,
  не 9. Клемп 9 срабатывает от 10 минут.
- **Тест**: при `reconnect_wait = 180 с` в GPS_INIT уходит 2; при 600 с — 9.

## P0-5. DotaTV-зрители: неверный входной кодек (N2)

- **ghostrs**: `ghost-spectator/src/relay.rs:240` вешает на сокет зрителя
  `ghost_net::spawn_conn`, а это `DualCodec` (`ghost-net/src/conn.rs:29-38`),
  который принимает только `0xF7`/`0xF8` и **ресинком выбрасывает** любой `0xFD`.
  Протокол DotaTV — `0xFD` (`ghost-protocol/src/dotatv.rs`). Зрительский чат
  (`0xFD 0x81`) физически не может дойти до релея. Тест
  `inbound_client_chat_frame_broadcasts_spectator_chat` подделывает фрейм вручную
  и реальный путь не проверяет.
- **Сделать**: параметризовать `spawn_conn` кодеком (или добавить
  `spawn_dtv_conn` с `DotaTvCodec`/`HeaderCodec<0xFD>`) и использовать его в
  листенере релея.
- **Тест**: E2E на реальном TCP — зритель шлёт байты `0xFD 0x81 ...`, релей
  получает чат и рассылает его остальным зрителям.

## P0-6. Мгновенный дроп игрока по backpressure (N7)

- **ghostrs**: `ghost-engine/src/state.rs:233-238` и `:265-269` — **одна**
  неудачная `try_send` помечает игрока ушедшим. При скачивании карты
  (несколько пакетов по 1442 байта за цикл в канал ёмкостью 1024) короткий
  всплеск выкидывает нормального игрока.
- **Эталон уже есть в репо**: `ghost-spectator/src/relay.rs:196-210` —
  счётчик подряд идущих сбоев `MAX_CONSECUTIVE_DROPS`.
- **Сделать**: различать `LinkError::Backpressure` и `LinkError::Closed`.
  `Closed` — дроп сразу (как сейчас). `Backpressure` — инкремент
  `consecutive_send_failures` у игрока, дроп только после N подряд (взять 
  ту же константу, что в релее); любая успешная отправка обнуляет счётчик.
- **Тест**: заполнить канал, N-1 сбоев подряд → игрок жив; N-й → помечен `left`;
  успешная отправка между сбоями сбрасывает счётчик.

---

# Фаза P1 — неверные байты и данные на проводе

## P1-1. LAN GAMEINFO несёт лишние 20 байт SHA1 (N21)

- **GHost++** строит **два разных** stat string:
  - BNCS `SID_STARTADVEX3` (`bnetprotocol.cpp:683-692`):
    `flags(4) + 0 + width(2) + height(2) + crc(4) + path\0 + hostName\0 + 0 + SHA1(20)`
  - LAN `W3GS_GAMEINFO` (`gameprotocol.cpp:669-678`): **то же самое без SHA1**.
- **ghostrs**: одна функция `ghost-bnet/src/advert.rs:18-32` (всегда с SHA1)
  используется для обоих; `ghostrs/src/supervisor.rs:204` отдаёт BNCS-вариант
  в `game_info(...)`.
- **Сделать**: две функции — `encode_bnet_statstring` (с SHA1) и
  `encode_lan_statstring` (без SHA1). Кодирование `UTIL_EncodeStatString`
  (`bytes_ext.rs:74-95`) уже совпадает 1:1 — не трогать.
- **Тест**: золотые векторы обоих вариантов; длина LAN-варианта строго меньше.

## P1-2. Stat string реплея — заглушка (N22)

- **GHost++**: `replay.cpp:157` кладёт настоящий `m_StatString`, сохранённый на
  старте игры.
- **ghostrs**: `ghost-engine/src/state.rs:151` —
  `replay.set_game(&cfg.name, &[0u8; 4], cfg.map.game_type)`, то есть 4 нулевых
  байта. Кодированный stat string нулей не содержит, поэтому ридер упирается в
  первый же 0 → секция карты в реплее пустая.
- **Сделать**: прокинуть реальный stat string (LAN-вариант, как у GHost++ в
  `BuildReplay`) в `GameConfig` и передавать в `set_game`.
- **Тест**: в собранном теле реплея после game name лежит непустой stat string
  без нулевых байтов.

## P1-3. Аргументы leave-блока реплея перепутаны (N23)

- **GHost++**: `replay.h:92` — `AddLeaveGame( uint32_t reason, unsigned char PID,
  uint32_t result )`, вызов `game_base.cpp:1609-1611`:
  `AddLeaveGame( 1, player->GetPID( ), player->GetLeftCode( ) )` →
  reason = **1**, result = **реальный left code**.
- **ghostrs**: `ghost-engine/src/state.rs:347` — `add_leaver(pid, 13, 0)` →
  reason = 13, result = 0. Оба значения неверны.
- **Сделать**: reason = 1, result = реальный left code игрока (см. P1-4).
  То же для `add_leaver_loading`.

## P1-4. `left_code` захардкожен 13 (N14)

- **GHost++** (`gameprotocol.h:38-45`): `PLAYERLEAVE_DISCONNECT 1`,
  `LOST 7`, `LOSTBUILDINGS 8`, `WON 9`, `DRAW 10`, `OBSERVER 11`, `LOBBY 13`,
  `GPROXY 100`.
- **ghostrs**: `ghost-engine/src/state.rs:353` и `:450` — обе отправки
  `player_leave_others(pid, 13)`.
- **Сделать**: хранить `left_code` у игрока и выставлять по ситуации: выход из
  лобби → 13; выход/дроп во время игры → 1; разрыв gproxy-соединения → 100.
  Использовать это значение и в `player_leave_others`, и в блоке реплея.

## P1-5. Параметры карты захардкожены (N15)

- **GHost++** читает `map_speed`, `map_visibility`, `map_observers`, `map_flags`,
  `map_filter`, `map_options`, `map_type` (список ключей — `map.cpp`).
- **ghostrs**: `ghost-engine/src/map.rs:386-394` — константы
  `MAKERUSER|SIZELARGE|OBSNONE` и `MAPSPEED_FAST|MAPVIS_DEFAULT|TEAMSTOGETHER|FIXEDTEAMS`.
- **Сделать** (по принятому решению): доставать реальные значения из w3x
  (`war3map.w3i` уже частично разбирается в `map.rs`), плюс необязательный
  per-map override в `ghost.toml`. Приоритет: override > w3i > текущий дефолт.
- **Тест**: карта с включёнными обсерверами даёт `game_type` с битом обсерверов,
  а не `OBSNONE`; override из конфига перекрывает распарсенное.

## P1-6. `slots_open` замораживается при создании игры (N5)

- **GHost++** шлёт актуальное число свободных слотов при каждом refresh.
- **ghostrs**: `ghostrs/src/supervisor.rs:634` — `slots_open = slots_total`,
  дальше не обновляется; `ghost-bnet/src/client.rs:210`
  `BnetCmd::RefreshGame { players: _, slots: _ }` **игнорирует оба аргумента** и
  переотправляет старый stat string. В итоге и LAN, и BNCS всегда показывают
  игру пустой.
- **Сделать**: актор игры сообщает супервизору текущее число занятых/свободных
  слотов; `RefreshGame` использует аргументы; LAN-анонс берёт свежее значение.

## P1-7. Replay host PID захардкожен (N10)

- **ghostrs**: `ghost-engine/src/state.rs:150` — `ReplayBody::new(1, ...)`, при
  этом `virtual_host_pid` инициализируется 255 (`state.rs:195`).
  Дополнительно `ghost-engine/src/actions.rs:83` ставит
  `set_host(host_pid, &self.cfg.virtual_host_name)` — pid реального игрока с
  именем виртуального хоста, что противоречиво.
- **Сделать**: писать реальный `virtual_host_pid` и соответствующее ему имя.

## P1-8. Релей не получает игровой чат и конец игры (N3)

- **ghostrs**: `RelayHandle::send_chat` не вызывается из движка ни разу;
  `RelayCmd::GameOver` встречается только в тестах — движок его не шлёт
  (`ghost-engine/src/actions.rs:477-480` шлёт только GameStart/PlayerInfo/GameBlock).
  Зрители не видят чат и не узнают о конце игры, `history` держится вечно.
- **Сделать**: форвардить чат из `send_chat_all`/`handle_chat_to_host` в релей;
  при переходе в `GamePhase::Over` слать `RelayCmd::GameOver`.

## P1-9. Чат в реплее теряет scope (N25)

- **GHost++**: `game_base.cpp:2950` пишет реальные `GetFlag()` и `ExtraFlags`.
- **ghostrs**: `ghost-engine/src/state.rs:305` — `rep.add_chat(from, 0x20, 0, msg)`,
  флаг и extra захардкожены.
- **Сделать**: передавать настоящие флаг и extra из входящего `CHAT_TO_HOST`.

---

# Фаза P2 — потерянные функции

## P2-1. `HOST_KICK_PLAYER` (0x1C) не шлётся никогда (N4)
Константа есть (`ghost-protocol/src/w3gs/ids.rs:15`), отправки нет. При
`!kick`/`!ban`/дропе слать кикнутому 0x1C и закрывать его сокет.

## P2-2. UDP: конфиг не применяется (N6)
`udp_broadcast_target` и `udp_cmd_port` есть в `ghost.toml`, но **ни одного
вхождения в Rust-коде**. `ghost-net/src/udp.rs:19` жёстко шлёт на
`255.255.255.255`. GHost++: `SetBroadcastTarget` + `SetDontRoute`
(`ghost.cpp:372-373`). Прокинуть таргет в `UdpBroadcaster`; при пустом значении
оставить широковещательный дефолт. Реализовать UDP-командный порт либо убрать
ключ из конфига и README — не оставлять мёртвым.

## P2-3. Лобби висит вечно (N16)
GHost++ `game_base.cpp:726-738`: при `m_AutoStartPlayers == 0` и
`m_LobbyTimeLimit > 0` лобби закрывается через N минут без reserved-игрока
(`m_LastReservedSeen`). В ghostrs нет ни таймаута, ни отслеживания.

## P2-4. `W3GS_MAPPARTNOTOK (0x45)` не объявлен и не обработан (N9)
`gameprotocol.h:99`. Клиент с битой частью карты молча зависает в загрузке.
Добавить id и обработчик — перезапуск отдачи с текущей позиции.

## P2-5. Валидация размера stat string (N26)
GHost++ отбрасывает advert при `StatString.size( ) >= 128`
(`bnetprotocol.cpp:694`). Добавить проверку с внятной ошибкой.

## P2-6. BNCS: необработанные SID (N27)
`ids.rs` знает 57 SID, `ghost-bnet/src/client.rs` обрабатывает 15.
Не обрабатываются `SID_WARDEN`, `SID_FRIENDSLIST`, все `SID_CLAN*`,
`SID_CHECKAD`. Из-за этого невозможны `!getfriends`, `!getclan`, `!invite`,
`!accept`, `grunt`/`peon`/`shaman`.

## P2-7. Отсутствующие подсистемы (N17-N19)
`LoadInGame` (`m_LoadInGame` + `m_LoadInGameData`), `AutoSave`, `MatchMaking`
(`m_Score`, min/max score), `MuteLobby`, `LocalAdminMessages`, периодический
`AnnounceMessage`/`AnnounceInterval`, квота 5 с на `!stats`/`!statsdota`
(`m_StatsSentTime`), permission-based downloads (`m_DownloadAllowed`),
rehost (`m_LastGameName`, `m_RefreshRehosted`), `CreatorName`/`CreatorServer`,
GProxy wait-notice в чат.

## P2-8. Недостающие команды
In-game: `autosave`, `dbstatus`, `fakeplayer`/`fppause`/`fpresume`, `from`
(ip-to-country), `messages`, `sendlan`, `pub`/`priv` (rehost из игры).
BNET: `accept`, `invite`, `getclan`, `getfriends`, `getgame`, `grunt`/`peon`/`shaman`,
`disable`/`enable`, `enforcesg`, `hostsg`, `loadsg`, `motd`, `privby`/`pubby`,
`reload`, `remove`, `autohostmm`, `wardenstatus`.

## P2-9. Мёртвый код
`ghost-legacy-attic` вне workspace; неиспользуемые BNCS-пакеты
`notifyjoin`/`checkad`/`logon_response*`; `players.rs::next_free_colour`;
`ChatCommand::Unknown` молчит (GHost++ отвечает); ключи `server_alias`,
`pvpgn_realm_name` в конфиге без применения. Либо задействовать, либо удалить.

---

# Что сверено и совпадает — НЕ ТРОГАТЬ

Подтверждено побайтово: `PING_FROM_HOST`, `SLOTINFO`/`SLOTINFOJOIN` (+`EncodeSlotInfo`),
`REJECTJOIN`, `PLAYERINFO` (порт 0 — у GHost++ тоже 0, `gameprotocol.cpp:378-379`),
`PLAYERLEAVE_OTHERS`, `GAMELOADED_OTHERS`, `COUNTDOWN_START/END`, `CHAT_FROM_HOST`,
`START_LAG`/`STOP_LAG`, `MAPCHECK`, `STARTDOWNLOAD`, `MAPPART`,
`INCOMING_ACTION`/`INCOMING_ACTION2` (включая срез CRC до 2 байт и отсутствие CRC
у пустого тика), порядок полей `GAMEINFO`, `SID_STARTADVEX3` (включая реверс
hex-строки host counter и байт 98), приём `REQJOIN`/`CHAT_TO_HOST`/`MAPSIZE`/
`MAPPARTOK`/`PONG`/`KEEPALIVE`/`OUTGOING_ACTION`, `UTIL_EncodeStatString`,
CRC32 (стандартный IEEE = `crc32fast`), структура заголовка реплея в `finish()`.

Также уже приведены в соответствие ранее: `MAX_ACTION_PAYLOAD = 1452`,
`COUNTDOWN_STEPS = 10`, host_pid в MAP_PART/START_DOWNLOAD, раздельные
0x1F/0x1E в реплее, HCL в `begin_loading`, пинги только в Lobby/Countdown/Loading,
глобальный host counter, `entry_key`, троттлинг загрузок, autokick по пингу,
spoof-checks, reserved-слоты, запись игр и DotA-статов в БД.
