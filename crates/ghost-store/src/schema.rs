use rusqlite::{Connection, Result};

pub const SCHEMA: &str = r#"
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
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL UNIQUE,
    server TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_admins_name ON admins(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS games (
    id       INTEGER PRIMARY KEY,
    name     TEXT NOT NULL,
    map      TEXT NOT NULL,
    started  INTEGER NOT NULL,
    ended    INTEGER NOT NULL,
    duration INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_games_name ON games(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS game_players (
    id           INTEGER PRIMARY KEY,
    game_id      INTEGER NOT NULL REFERENCES games(id),
    name         TEXT NOT NULL,
    ip           TEXT NOT NULL DEFAULT '',
    spoofed      INTEGER NOT NULL DEFAULT 0,
    loading_time INTEGER NOT NULL DEFAULT 0,
    left_reason  TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_game_players_game ON game_players(game_id);
CREATE INDEX IF NOT EXISTS idx_game_players_name ON game_players(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS dotagames (
    id               INTEGER PRIMARY KEY,
    game_id          INTEGER NOT NULL REFERENCES games(id),
    winner           INTEGER NOT NULL DEFAULT 0,
    duration         INTEGER NOT NULL DEFAULT 0,
    tree_hp          INTEGER NOT NULL DEFAULT 100,
    throne_hp        INTEGER NOT NULL DEFAULT 100
);
CREATE INDEX IF NOT EXISTS idx_dotagames_game ON dotagames(game_id);

CREATE TABLE IF NOT EXISTS dotaplayers (
    id            INTEGER PRIMARY KEY,
    game_id       INTEGER NOT NULL REFERENCES games(id),
    colour        INTEGER NOT NULL,
    name          TEXT NOT NULL,
    hero          TEXT NOT NULL DEFAULT '',
    kills         INTEGER NOT NULL DEFAULT 0,
    deaths        INTEGER NOT NULL DEFAULT 0,
    assists       INTEGER NOT NULL DEFAULT 0,
    creep_kills   INTEGER NOT NULL DEFAULT 0,
    creep_denies  INTEGER NOT NULL DEFAULT 0,
    neutral_kills INTEGER NOT NULL DEFAULT 0,
    tower_kills   INTEGER NOT NULL DEFAULT 0,
    rax_kills     INTEGER NOT NULL DEFAULT 0,
    courier_kills INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_dotaplayers_game ON dotaplayers(game_id);
CREATE INDEX IF NOT EXISTS idx_dotaplayers_name ON dotaplayers(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS downloads (
    id         INTEGER PRIMARY KEY,
    map        TEXT NOT NULL,
    map_size   INTEGER NOT NULL,
    name       TEXT NOT NULL,
    ip         TEXT NOT NULL DEFAULT '',
    spoofed    INTEGER NOT NULL DEFAULT 0,
    downloaded INTEGER NOT NULL DEFAULT 0,
    duration   INTEGER NOT NULL DEFAULT 0,
    created    INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_downloads_name ON downloads(name COLLATE NOCASE);
"#;

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
