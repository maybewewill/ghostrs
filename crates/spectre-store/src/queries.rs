use crate::writer::DotAStatsSummary;
use rusqlite::{Connection, OptionalExtension, Result, params};

pub fn query_dota_stats(conn: &Connection, name: &str) -> Option<DotAStatsSummary> {
    conn.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(kills), 0),
                COALESCE(SUM(deaths), 0),
                COALESCE(SUM(assists), 0),
                COALESCE(SUM(creep_kills), 0),
                COALESCE(SUM(creep_denies), 0),
                COALESCE(SUM(neutral_kills), 0),
                COALESCE(SUM(tower_kills), 0),
                COALESCE(SUM(rax_kills), 0)
         FROM dotaplayers WHERE name = ?1 COLLATE NOCASE",
        params![name],
        |r| {
            let games: u32 = r.get(0)?;
            Ok(DotAStatsSummary {
                games,
                wins: 0,
                losses: 0,
                kills: r.get(1)?,
                deaths: r.get(2)?,
                assists: r.get(3)?,
                creep_kills: r.get(4)?,
                creep_denies: r.get(5)?,
                neutral_kills: r.get(6)?,
                tower_kills: r.get(7)?,
                rax_kills: r.get(8)?,
            })
        },
    )
    .optional()
    .ok()
    .flatten()
    .filter(|s| s.games > 0)
}

#[allow(clippy::too_many_arguments)]
pub fn insert_download(
    conn: &Connection,
    map: &str,
    map_size: u64,
    name: &str,
    ip: &str,
    spoofed: u8,
    downloaded: u64,
    duration_seconds: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO downloads (map, map_size, name, ip, spoofed, downloaded, duration, created)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, strftime('%s', 'now'))",
        params![
            map,
            map_size as i64,
            name,
            ip,
            spoofed,
            downloaded as i64,
            duration_seconds as i64
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::schema::init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn aggregates_dota_stats_across_multiple_games() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO games (id, name, map, started, ended, duration) VALUES (1, 'g1', 'dota', 0, 100, 100)", []).unwrap();
        conn.execute("INSERT INTO dotagames (id, game_id, winner, duration, tree_hp, throne_hp) VALUES (1, 1, 1, 100, 100, 0)", []).unwrap();
        conn.execute(
            "INSERT INTO dotaplayers (game_id, colour, name, hero, kills, deaths, assists, creep_kills, creep_denies, neutral_kills, tower_kills, rax_kills, courier_kills)
             VALUES (1, 1, 'Alice', 'E001', 10, 2, 8, 120, 15, 30, 2, 1, 0)", []).unwrap();

        let s = query_dota_stats(&conn, "alice").expect("alice must have stats");
        assert_eq!(s.games, 1);
        assert_eq!(s.kills, 10);
        assert_eq!(s.deaths, 2);
        assert_eq!(s.assists, 8);
        assert_eq!(s.creep_kills, 120);
        assert_eq!(s.creep_denies, 15);
        assert_eq!(s.tower_kills, 2);
    }

    #[test]
    fn records_and_queries_downloads_table() {
        let conn = setup_test_db();
        insert_download(
            &conn,
            "DotA_v6.83d.w3x",
            8_000_000,
            "Bob",
            "192.168.1.50",
            1,
            8_000_000,
            45,
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM downloads WHERE name = 'Bob'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
