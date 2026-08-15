use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::schema::init_schema;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ban {
    pub name: String,
    pub ip: String,
    pub admin: String,
    pub reason: String,
    pub created: i64,
}

pub enum StoreQuery {
    IsBanned {
        name: String,
        ip: String,
        reply: oneshot::Sender<Option<Ban>>,
    },
    IsAdmin {
        name: String,
        reply: oneshot::Sender<bool>,
    },
    JournalMode {
        reply: oneshot::Sender<String>,
    },
    GamePlayerCount {
        game_name: String,
        reply: oneshot::Sender<usize>,
    },
}

pub enum StoreCmd {
    AddBan {
        name: String,
        ip: String,
        admin: String,
        reason: String,
    },
    RemoveBan {
        name: String,
    },
    LogGame {
        name: String,
        map: String,
        started: i64,
        ended: i64,
        players: Vec<String>,
    },
    Query(StoreQuery),
}

#[derive(Debug, Clone)]
pub struct Store {
    tx: mpsc::Sender<StoreCmd>,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<(Self, JoinHandle<()>)> {
        let conn = Connection::open(path)?;
        init_schema(&conn)?;
        let (tx, rx) = mpsc::channel(1024);
        let join = tokio::task::spawn_blocking(move || {
            run_worker(conn, rx);
        });
        Ok((Self { tx }, join))
    }

    pub fn open_in_memory() -> anyhow::Result<(Self, JoinHandle<()>)> {
        let conn = Connection::open_in_memory()?;
        init_schema(&conn)?;
        let (tx, rx) = mpsc::channel(1024);
        let join = tokio::task::spawn_blocking(move || {
            run_worker(conn, rx);
        });
        Ok((Self { tx }, join))
    }

    pub fn ban(&self, name: &str, ip: &str, admin: &str, reason: &str) {
        if let Err(e) = self.tx.try_send(StoreCmd::AddBan {
            name: name.to_string(),
            ip: ip.to_string(),
            admin: admin.to_string(),
            reason: reason.to_string(),
        }) {
            tracing::warn!(error = %e, "failed to send AddBan command");
        }
    }

    pub fn unban(&self, name: &str) {
        if let Err(e) = self.tx.try_send(StoreCmd::RemoveBan {
            name: name.to_string(),
        }) {
            tracing::warn!(error = %e, "failed to send RemoveBan command");
        }
    }

    pub fn log_game(
        &self,
        name: &str,
        map: &str,
        started: i64,
        ended: i64,
        players: Vec<String>,
    ) {
        if let Err(e) = self.tx.try_send(StoreCmd::LogGame {
            name: name.to_string(),
            map: map.to_string(),
            started,
            ended,
            players,
        }) {
            tracing::warn!(error = %e, "failed to send LogGame command");
        }
    }

    pub async fn is_banned(&self, name: &str, ip: &str) -> Option<Ban> {
        let (reply, rx) = oneshot::channel();
        let query = StoreQuery::IsBanned {
            name: name.to_string(),
            ip: ip.to_string(),
            reply,
        };
        let _ = self.tx.send(StoreCmd::Query(query)).await;
        rx.await.ok().flatten()
    }

    pub async fn is_admin(&self, name: &str) -> bool {
        let (reply, rx) = oneshot::channel();
        let query = StoreQuery::IsAdmin {
            name: name.to_string(),
            reply,
        };
        let _ = self.tx.send(StoreCmd::Query(query)).await;
        rx.await.unwrap_or(false)
    }

    pub async fn journal_mode(&self) -> String {
        let (reply, rx) = oneshot::channel();
        let query = StoreQuery::JournalMode { reply };
        let _ = self.tx.send(StoreCmd::Query(query)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn game_player_count(&self, game_name: &str) -> usize {
        let (reply, rx) = oneshot::channel();
        let query = StoreQuery::GamePlayerCount {
            game_name: game_name.to_string(),
            reply,
        };
        let _ = self.tx.send(StoreCmd::Query(query)).await;
        rx.await.unwrap_or(0)
    }
}

fn run_worker(mut conn: Connection, mut rx: mpsc::Receiver<StoreCmd>) {
    while let Some(cmd) = rx.blocking_recv() {
        match cmd {
            StoreCmd::AddBan { name, ip, admin, reason } => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                if let Err(e) = conn.execute(
                    "INSERT INTO bans (name, ip, admin, reason, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![name, ip, admin, reason, now],
                ) {
                    tracing::warn!(error = %e, "failed to insert ban");
                }
            }
            StoreCmd::RemoveBan { name } => {
                if let Err(e) = conn.execute(
                    "DELETE FROM bans WHERE name = ?1 COLLATE NOCASE",
                    rusqlite::params![name],
                ) {
                    tracing::warn!(error = %e, "failed to remove ban");
                }
            }
            StoreCmd::LogGame { name, map, started, ended, players } => {
                let tx_res = conn.transaction();
                if let Ok(tx) = tx_res {
                    let insert_res = tx.execute(
                        "INSERT INTO games (name, map, started, ended) VALUES (?1, ?2, ?3, ?4)",
                        rusqlite::params![name, map, started, ended],
                    );
                    if insert_res.is_ok() {
                        let game_id = tx.last_insert_rowid();
                        for player in players {
                            let _ = tx.execute(
                                "INSERT INTO game_players (game_id, name) VALUES (?1, ?2)",
                                rusqlite::params![game_id, player],
                            );
                        }
                        let _ = tx.commit();
                    }
                }
            }
            StoreCmd::Query(q) => match q {
                StoreQuery::IsBanned { name, ip, reply } => {
                    let res: rusqlite::Result<Option<Ban>> = (|| {
                        let mut stmt = conn.prepare(
                            "SELECT name, ip, admin, reason, created FROM bans WHERE (name = ?1 COLLATE NOCASE AND name != '') OR (ip = ?2 AND ip != '') LIMIT 1"
                        )?;
                        let mut rows = stmt.query(rusqlite::params![name, ip])?;
                        if let Some(row) = rows.next()? {
                            Ok(Some(Ban {
                                name: row.get(0)?,
                                ip: row.get(1)?,
                                admin: row.get(2)?,
                                reason: row.get(3)?,
                                created: row.get(4)?,
                            }))
                        } else {
                            Ok(None)
                        }
                    })();
                    let _ = reply.send(res.ok().flatten());
                }
                StoreQuery::IsAdmin { name, reply } => {
                    let res: rusqlite::Result<bool> = (|| {
                        let mut stmt = conn.prepare("SELECT 1 FROM admins WHERE name = ?1 COLLATE NOCASE LIMIT 1")?;
                        let mut rows = stmt.query(rusqlite::params![name])?;
                        Ok(rows.next()?.is_some())
                    })();
                    let _ = reply.send(res.unwrap_or(false));
                }
                StoreQuery::JournalMode { reply } => {
                    let mode: String = conn
                        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
                        .unwrap_or_default();
                    let _ = reply.send(mode);
                }
                StoreQuery::GamePlayerCount { game_name, reply } => {
                    let count: usize = conn
                        .query_row(
                            "SELECT count(*) FROM game_players JOIN games ON games.id = game_players.game_id WHERE games.name = ?1",
                            rusqlite::params![game_name],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);
                    let _ = reply.send(count);
                }
            },
        }
    }
}

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
