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
"#;

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
