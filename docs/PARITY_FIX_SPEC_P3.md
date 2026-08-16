# ghostrs ↔ GHost++: фаза P3 — карта, статы, добор

> Составлена 2026-08-16 после построчной сверки `map.cpp` (995 строк),
> `map.h`, `gameslot.cpp`, `statsdota.cpp` против `crates/`.
> Фазы P0, P1, P2 уже реализованы, 251 тест зелёный. Это добор.
>
> **Метод: TDD.** Сначала падающий тест, затем фикс, затем
> `cargo test --workspace` целиком зелёный (сумма по всем крейтам, не строка одного).

## Что уже совпадает — НЕ ТРОГАТЬ

- **CRC/SHA1-конвейер карты** (`map.rs:153-228` ↔ `map.cpp:264-443`): перекрытие
  `Scripts\common.j` / `Scripts\blizzard.j` копиями из MPQ, двойная ротация
  `val.rotate_left(3)` → `(val ^ 0x03F1379E).rotate_left(3)`, подмешивание
  `9E 37 F1 03` в SHA1, точный список из 10 файлов, логика `found_script`.
  Сверено байт-в-байт. Любое изменение здесь ломает совместимость карт.
- **9-байтовый формат слота** (`w3gs/slot.rs:26-36` ↔ `gameslot.cpp:59-70`):
  pid, download_status, slot_status, computer, team, colour, race,
  computer_type, handicap. Совпадает.
- Весь список из раздела «НЕ ТРОГАТЬ» в `docs/PARITY_FIX_SPEC.md`.

---

# Группа M — `map.cpp`

## M1. `map_flags` трактуется как сырое wire-значение (баг override)

- **GHost++**: `m_MapFlags` — маленькая битовая маска `MAPFLAG_*` (1/2/4/8/16),
  дефолт `MAPFLAG_TEAMSTOGETHER | MAPFLAG_FIXEDTEAMS` = **3** (`map.cpp:745`).
  `GetMapGameFlags()` (`map.cpp:125-134`) раскладывает каждый бит в свой
  wire-бит: TEAMSTOGETHER→`0x00004000`, FIXEDTEAMS→`0x00060000`,
  UNITSHARE→`0x01000000`, RANDOMHERO→`0x02000000`, RANDOMRACES→`0x04000000`.
- **ghostrs**: `map.rs:437` — `map_flags` берётся из override и **OR-ится в
  wire-флаги напрямую**, дефолт `0x0006_4000`. Дефолт даёт верный результат
  случайно, но override сломан: константы `MAPFLAG_*` объявлены как 1/2/4/8/16
  (`map.rs:27-31`), и пользователь, задав `flags = 3`, получит на проводе
  `0x3` — это биты скорости, а не команд.
- **Сделать**: хранить `map_flags` как маску `MAPFLAG_*` и раскладывать в
  wire-биты ровно как `map.cpp:125-134`.
- **Тест**: `flags = MAPFLAG_TEAMSTOGETHER | MAPFLAG_FIXEDTEAMS` даёт
  `0x00064000`; `MAPFLAG_RANDOMRACES` даёт `0x04000000`.

## M2. `game_type` собирается неверно: size из размеров, obs из observers

- **GHost++** (`map.cpp:177-211`): `GetMapGameType()` строится из **четырёх
  независимых** полей-масок, каждое допускает несколько бит:
  `m_MapFilterMaker` (дефолт `MAPFILTER_MAKER_USER`, `:746`),
  `m_MapFilterType` (дефолт `MAPFILTER_TYPE_SCENARIO`, `:460`; melee-карта
  переставляет в `MAPFILTER_TYPE_MELEE`, `:667`),
  `m_MapFilterSize` (дефолт `MAPFILTER_SIZE_LARGE`, `:756`),
  `m_MapFilterObs` (дефолт `MAPFILTER_OBS_NONE`, `:757`).
- **ghostrs** (`map.rs:457-480`): maker и type — примерно так же, но
  **size выводится из width/height** (выдумка, у GHost++ этого нет), а
  **obs берётся из `m_MapObservers`** — это другое поле, отвечающее за
  игровые флаги, а не за фильтр в списке игр. Поля `filter_size` и
  `filter_obs` в `MapOverride` объявлены (`map.rs:112-113`), но **не читаются**.
- **Сделать**: собирать `game_type` из четырёх полей-масок с дефолтами GHost++,
  как `map.cpp:181-209`. Убрать вывод размера из width/height.
- **Тест**: дефолтная карта → `MAKERUSER|TYPESCENARIO|SIZELARGE|OBSNONE`;
  melee-карта → `TYPEMELEE`; `filter_size = SMALL|MEDIUM` → оба бита.

## M3. Отсутствуют константы `MAPFILTER_OBS_FULL` / `MAPFILTER_OBS_ONDEATH`

- **GHost++** `map.h:66-68`: `OBS_FULL=1`, `OBS_ONDEATH=2`, `OBS_NONE=4`.
- **ghostrs** `map.rs:50`: объявлен только `MAPFILTER_OBS_NONE = 4`.
- **Сделать**: добавить обе константы, использовать в M2.

## M4. Нет melee-инициализации слотов

- **GHost++** `map.cpp:653-668`: при `MAPOPT_MELEE` каждому слоту выдаётся
  **свой** номер команды (`Team++`, то есть 0,1,2,…), раса — `SLOTRACE_RANDOM`,
  и `MapFilterType` переставляется в `MAPFILTER_TYPE_MELEE`.
- **ghostrs**: ветки нет. Команда назначается как `i / 6` (`map.rs:299, 373`) —
  фиксированное деление по 6, что для melee-карты неверно.
- **Тест**: melee-карта на 4 слота → команды 0,1,2,3 и раса RANDOM у всех.

## M5. Нет принудительного random races

- **GHost++** `map.cpp:851-857`: при `m_MapFlags & MAPFLAG_RANDOMRACES` всем
  слотам ставится `SLOTRACE_RANDOM`.
- **ghostrs**: отсутствует.

## M6. Нет слотов наблюдателей и `EditorVersion`

- **GHost++** `map.cpp:861-871`: при `m_MapObservers == MAPOBS_ALLOWED` или
  `MAPOBS_REFEREES` слоты добиваются до `map_maxslots` наблюдательскими:
  `CGameSlot( 0, 255, SLOTSTATUS_OPEN, 0, MAX_SLOTS, MAX_SLOTS, SLOTRACE_RANDOM )`.
  Дефолт `map_maxslots` = `MAX_SLOTS`, но **12, если `EditorVersion < 6060`**
  (`:863-866`). `EditorVersion` читается из w3i (`:497`).
- **ghostrs**: `EditorVersion` не читается вообще, слоты наблюдателей не
  добавляются, `map_maxslots` нет.
- **Сделать**: читать editor version при разборе w3i, добавить override
  `max_slots`, реализовать добивку слотов.
- **Тест**: карта с `observers = MAPOBS_ALLOWED` и 10 игровыми слотами →
  итого 24 слота (или 12 при editor version < 6060), добавленные имеют
  team = colour = MAX_SLOTS.

## M7. Не вычитаются закрытые слоты из `num_players`

- **GHost++** `map.cpp:566-601, 640`: слоты со `Status` не 1 и не 2 считаются
  `SLOTSTATUS_CLOSED`, **в список слотов не попадают**, и
  `MapNumPlayers = RawMapNumPlayers - ClosedSlots`.
- **ghostrs**: проверить `map.rs:236-300` — убедиться, что закрытые слоты
  исключаются и `num_players` уменьшается. Если нет — исправить.

## M8. Потерянные поля карты

`map.cpp:790-795` читает и хранит: `map_type` (выбирает класс статистики —
dota или w3mmd), `map_matchmakingcategory`, `map_statsw3mmdcategory`,
`map_defaulthcl`, `map_defaultplayerscore` (дефолт 1000),
`map_loadingame`, `map_localpath`, `map_maxslots`.

В ghostrs нет ни одного. Особо важны два:
- **`map_defaulthcl`**: GHost++ инициализирует `m_HCLCommandString(
  nMap->GetMapDefaultHCL( ) )` — HCL по умолчанию берётся **из карты**.
  ghostrs парсит HCL только из имени игры.
- **`map_type`**: выбирает, какой парсер статистики включать. ghostrs всегда
  создаёт и `StatsDotA`, и `StatsW3MMD` (`state.rs:220-225`).

**Сделать**: добавить эти поля в `MapOverride` / `MapInfo` и задействовать
`map_defaulthcl` и `map_type` по назначению. `map_defaultplayerscore` и
`map_matchmakingcategory` — только хранить (matchmaking всё равно заглушка,
схему БД менять нельзя).

## M9. Нет валидации карты (`CheckValid`)

`map.cpp:876-937` проверяет: `map_path` непустой и ≤ 53 символов; предупреждение
если начинается с `\` (реплей будет непроигрываемым) или содержит `/`;
размеры `map_size`/`map_info`/`map_crc` = 4 байта, `map_sha1` = 20;
`map_size` совпадает с фактическим размером данных; `map_speed`,
`map_visibility`, `map_observers` — только допустимые значения.

В ghostrs валидации нет вообще. Добавить эквивалент, возвращающий внятную
ошибку (не панику) при невалидной карте.

---

# Группа S — статистика

## S1. DotA-парсер не пишет героев, курьеров и позиции строений

- **GHost++** `statsdota.cpp` разбирает ключи: `CK` (courier kills), `Hero`,
  `License`, и позиционные строки `top` / `mid` / `bottom` / `melee` /
  `ranged` / `unknown` для классификации башен и бараков.
- **ghostrs** `stats_dota.rs`: ключей `CK`, `Hero`, `License` и позиционных
  строк **нет**. То есть колонки `hero`, `courierkills`, `towerkills`,
  `raxkills` в `dotaplayers` заполняются неполно или нулями.
- **Сделать**: доразобрать эти ключи по образцу `statsdota.cpp`, класть в уже
  существующие поля записи. **Схему БД не менять** — колонки уже есть.
- **Тест**: поток действий с `Hero`/`CK`/`Tower` разной позиции → корректные
  значения в записи игрока.

## S2. W3MMD — сверить парсинг

`statsw3mmd.cpp` (453 строки) против `stats_w3mmd.rs` (129 строк) построчно я
не сверял. Сверить и дописать недостающее, не меняя схему БД.

---

# Группа R — добор

## R1. Разное host name в LAN и BNCS stat string

`supervisor.rs:782` кладёт в LAN stat string `virtual_host_name`, а
`client.rs:193` в BNCS — `bnet.username`. У GHost++ в обоих адвертах одно и то
же имя. Привести к одному значению.

## R2. Три clan-SID остались без обработчиков

`SID_CLANCHANGERANK`, `SID_CLANREMOVEMEMBER`, `SID_CLANSETMOTD` — команды
`grunt`/`peon`/`shaman`/`remove` доходят до `BnetCmd`, но входящие ответы
сервера не разбираются, так что результат операции не подтверждается.
Добавить обработку ответов.

## R3. `language.cfg` не сверялся

`language.cpp` (1535 строк) / `language.cfg` — какие именно сообщения потеряны,
не выяснено. Пройтись и добавить недостающие строки в `lang.rs` для всех
команд и событий, которые уже реализованы. На провод не влияет, влияет на UX.
