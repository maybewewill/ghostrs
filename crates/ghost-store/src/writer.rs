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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DotAPlayerRecord {
    pub colour: u32,
    pub name: String,
    pub hero: String,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_kills: u32,
    pub creep_denies: u32,
    pub neutral_kills: u32,
    pub tower_kills: u32,
    pub rax_kills: u32,
    pub courier_kills: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DotAStatsSummary {
    pub games: u32,
    pub wins: u32,
    pub losses: u32,
    pub kills: u32,
    pub deaths: u32,
    pub assists: u32,
    pub creep_kills: u32,
    pub creep_denies: u32,
    pub neutral_kills: u32,
    pub tower_kills: u32,
    pub rax_kills: u32,
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
    GetDotAStats {
        name: String,
        reply: oneshot::Sender<Option<DotAStatsSummary>>,
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
    AddAdmin {
        name: String,
        server: String,
    },
    RemoveAdmin {
        name: String,
    },
    LogGame {
        name: String,
        map: String,
        started: i64,
        ended: i64,
        players: Vec<String>,
    },
    LogDotAGame {
        game_name: String,
        winner: u32,
        duration: u32,
        tree_hp: u32,
        throne_hp: u32,
        players: Vec<DotAPlayerRecord>,
    },
    RecordDownload {
        map: String,
        map_size: u64,
        name: String,
        ip: String,
        spoofed: u8,
        downloaded: u64,
        duration: u64,
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
        let _ = self.tx.try_send(StoreCmd::AddBan {
            name: name.to_string(),
            ip: ip.to_string(),
            admin: admin.to_string(),
            reason: reason.to_string(),
        });
    }

    pub fn unban(&self, name: &str) {
        let _ = self.tx.try_send(StoreCmd::RemoveBan {
            name: name.to_string(),
        });
    }

    pub fn add_admin(&self, name: &str, server: &str) {
        let _ = self.tx.try_send(StoreCmd::AddAdmin {
            name: name.to_string(),
            server: server.to_string(),
        });
    }

    pub fn remove_admin(&self, name: &str) {
        let _ = self.tx.try_send(StoreCmd::RemoveAdmin {
            name: name.to_string(),
        });
    }

    pub fn log_game(&self, name: &str, map: &str, started: i64, ended: i64, players: Vec<String>) {
        let _ = self.tx.try_send(StoreCmd::LogGame {
            name: name.to_string(),
            map: map.to_string(),
            started,
            ended,
            players,
        });
    }

    pub fn log_dota_game(
        &self,
        game_name: &str,
        winner: u32,
        duration: u32,
        tree_hp: u32,
        throne_hp: u32,
        players: Vec<DotAPlayerRecord>,
    ) {
        let _ = self.tx.try_send(StoreCmd::LogDotAGame {
            game_name: game_name.to_string(),
            winner,
            duration,
            tree_hp,
            throne_hp,
            players,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_download(
        &self,
        map: &str,
        map_size: u64,
        name: &str,
        ip: &str,
        spoofed: u8,
        downloaded: u64,
        duration: u64,
    ) {
        let _ = self.tx.try_send(StoreCmd::RecordDownload {
            map: map.to_string(),
            map_size,
            name: name.to_string(),
            ip: ip.to_string(),
            spoofed,
            downloaded,
            duration,
        });
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

    pub async fn get_dota_stats(&self, name: &str) -> Option<DotAStatsSummary> {
        let (reply, rx) = oneshot::channel();
        let query = StoreQuery::GetDotAStats {
            name: name.to_string(),
            reply,
        };
        let _ = self.tx.send(StoreCmd::Query(query)).await;
        rx.await.ok().flatten()
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
            StoreCmd::AddBan {
                name,
                ip,
                admin,
                reason,
            } => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64;
                let _ = conn.execute(
                    "INSERT INTO bans (name, ip, admin, reason, created) VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![name, ip, admin, reason, now],
                );
            }
            StoreCmd::RemoveBan { name } => {
                let _ = conn.execute(
                    "DELETE FROM bans WHERE name = ?1 COLLATE NOCASE",
                    rusqlite::params![name],
                );
            }
            StoreCmd::AddAdmin { name, server } => {
                let _ = conn.execute(
                    "INSERT OR REPLACE INTO admins (name, server) VALUES (?1, ?2)",
                    rusqlite::params![name, server],
                );
            }
            StoreCmd::RemoveAdmin { name } => {
                let _ = conn.execute(
                    "DELETE FROM admins WHERE name = ?1 COLLATE NOCASE",
                    rusqlite::params![name],
                );
            }
            StoreCmd::LogGame {
                name,
                map,
                started,
                ended,
                players,
            } => {
                let duration = ended.saturating_sub(started);
                if let Ok(tx) = conn.transaction()
                    && tx.execute(
                        "INSERT INTO games (name, map, started, ended, duration) VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![name, map, started, ended, duration],
                    ).is_ok()
                {
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
            StoreCmd::LogDotAGame {
                game_name,
                winner,
                duration,
                tree_hp,
                throne_hp,
                players,
            } => {
                if let Ok(tx) = conn.transaction() {
                    let game_id: i64 = tx
                        .query_row(
                            "SELECT id FROM games WHERE name = ?1 ORDER BY id DESC LIMIT 1",
                            rusqlite::params![game_name],
                            |r| r.get(0),
                        )
                        .unwrap_or(0);

                    let _ = tx.execute(
                        "INSERT INTO dotagames (game_id, winner, duration, tree_hp, throne_hp) VALUES (?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![game_id, winner, duration, tree_hp, throne_hp],
                    );

                    for p in players {
                        let _ = tx.execute(
                            "INSERT INTO dotaplayers (game_id, colour, name, hero, kills, deaths, assists, creep_kills, creep_denies, neutral_kills, tower_kills, rax_kills, courier_kills)
                             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                            rusqlite::params![game_id, p.colour, p.name, p.hero, p.kills, p.deaths, p.assists, p.creep_kills, p.creep_denies, p.neutral_kills, p.tower_kills, p.rax_kills, p.courier_kills],
                        );
                    }
                    let _ = tx.commit();
                }
            }
            StoreCmd::RecordDownload {
                map,
                map_size,
                name,
                ip,
                spoofed,
                downloaded,
                duration,
            } => {
                let _ = crate::queries::insert_download(
                    &conn, &map, map_size, &name, &ip, spoofed, downloaded, duration,
                );
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
                        let mut stmt = conn.prepare(
                            "SELECT 1 FROM admins WHERE name = ?1 COLLATE NOCASE LIMIT 1",
                        )?;
                        let mut rows = stmt.query(rusqlite::params![name])?;
                        Ok(rows.next()?.is_some())
                    })();
                    let _ = reply.send(res.unwrap_or(false));
                }
                StoreQuery::GetDotAStats { name, reply } => {
                    let res = crate::queries::query_dota_stats(&conn, &name);
                    let _ = reply.send(res);
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
    async fn admin_management_works() {
        let (store, _join) = Store::open_in_memory().unwrap();
        store.add_admin("RootAdmin", "pvpgn");
        assert!(store.is_admin("rootadmin").await);
        store.remove_admin("RootAdmin");
        assert!(!store.is_admin("rootadmin").await);
    }

    #[tokio::test]
    async fn dota_stats_logging_and_query() {
        let (store, _join) = Store::open_in_memory().unwrap();
        store.log_game("dota_1", "dota.w3x", 100, 200, vec!["Slash".into()]);
        store.log_dota_game(
            "dota_1",
            1,
            1500,
            100,
            0,
            vec![DotAPlayerRecord {
                colour: 1,
                name: "Slash".into(),
                hero: "E001".into(),
                kills: 10,
                deaths: 2,
                assists: 8,
                creep_kills: 150,
                creep_denies: 20,
                neutral_kills: 30,
                tower_kills: 2,
                rax_kills: 1,
                courier_kills: 0,
            }],
        );

        let stats = store.get_dota_stats("slash").await.expect("stats");
        assert_eq!(stats.games, 1);
        assert_eq!(stats.kills, 10);
        assert_eq!(stats.deaths, 2);
    }
}
