use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use ghost_bnet::{BnetCmd, BnetEvent, BnetHandle, MapAdvert, encode_game_statstring, spawn_bnet};
use ghost_engine::{GameCmd, GameConfig, GameHandle, MapInfo, ParsedMap, spawn_game};
use ghost_net::{ConnEvent, UdpBroadcaster, spawn_conn, spawn_listener};
use ghost_protocol::w3gs::outgoing::game_info;
use ghost_spectator::{RelayConfig, RelayHandle, spawn_relay};
use ghost_store::Store;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct AutohostConfig {
    pub map_file: String,
    pub game_prefix: String,
    pub max_games: usize,
}

pub struct Supervisor {
    cfg: Config,
    store: Store,
    bnet: BnetHandle,
    bnet_events: mpsc::Receiver<BnetEvent>,
    current_game: Option<GameHandle>,
    current_game_name: Option<String>,
    current_game_advert: Option<ActiveLobbyAdvert>,
    running_games: Vec<(String, GameHandle, JoinHandle<()>)>,
    conn_to_game: HashMap<u64, GameHandle>,
    listener_rx: mpsc::Receiver<(u64, TcpStream, SocketAddr)>,
    conn_event_tx: mpsc::Sender<ConnEvent>,
    conn_event_rx: mpsc::Receiver<ConnEvent>,
    udp_broadcaster: Option<UdpBroadcaster>,
    #[allow(dead_code)]
    spectator_relay: Option<RelayHandle>,
    selected_map_file: Option<String>,
    autohost: Option<AutohostConfig>,
    autohost_counter: u32,
}

struct ActiveLobbyAdvert {
    game_name: String,
    stat_string: Vec<u8>,
    host_counter: u32,
    map_game_type: [u8; 4],
}

impl Supervisor {
    pub async fn run(
        cfg: Config,
        host_on_start: Option<String>,
        start_after: Option<u64>,
    ) -> anyhow::Result<()> {
        let (store, _store_task) =
            Store::open(&cfg.db_path).context("failed to open SQLite database")?;

        let (bnet_events_tx, bnet_events_rx) = mpsc::channel(256);
        let (bnet, _bnet_task) = spawn_bnet(cfg.bnet.clone(), bnet_events_tx);

        let (listener_tx, listener_rx) = mpsc::channel(256);
        let bind_addr: SocketAddr = format!("{}:{}", cfg.bot.bind_address, cfg.bot.host_port)
            .parse()
            .context("invalid bot bind address/port")?;
        let _listener_task = spawn_listener(bind_addr, listener_tx);

        let (conn_event_tx, conn_event_rx) = mpsc::channel(1024);

        let udp_broadcaster = match UdpBroadcaster::bind(cfg.bot.host_port).await {
            Ok(u) => Some(u),
            Err(e) => {
                tracing::warn!(error = %e, "failed to bind UDP broadcaster for LAN games");
                None
            }
        };

        let spectator_relay = if cfg.spectator.enabled {
            let (handle, _join) = spawn_relay(RelayConfig {
                port: cfg.spectator.port,
                delay: cfg.spectator.delay,
                max_viewers: cfg.spectator.max_viewers,
                game_name: "DotaTV".into(),
                history_max_mb: cfg.spectator.history_max_mb,
            });
            Some(handle)
        } else {
            None
        };

        let mut sup = Self {
            cfg,
            store,
            bnet,
            bnet_events: bnet_events_rx,
            current_game: None,
            current_game_name: None,
            current_game_advert: None,
            running_games: Vec::new(),
            conn_to_game: HashMap::new(),
            listener_rx,
            conn_event_tx,
            conn_event_rx,
            udp_broadcaster,
            spectator_relay,
            selected_map_file: None,
            autohost: None,
            autohost_counter: 1,
        };

        if let Some(name) = host_on_start {
            let owner = sup.cfg.bnet.username.clone();
            sup.create_game(&name, &owner, ghost_protocol::GameVisibility::Public);
        }

        sup.event_loop(start_after).await
    }

    async fn event_loop(&mut self, start_after: Option<u64>) -> anyhow::Result<()> {
        tracing::info!("supervisor ready, awaiting battle.net and player events");

        let mut lan_timer = tokio::time::interval(Duration::from_secs(3));
        let mut cleanup_timer = tokio::time::interval(Duration::from_secs(1));
        let auto_start = start_after.map(|s| {
            Box::pin(tokio::time::sleep(Duration::from_secs(s)))
                as std::pin::Pin<Box<tokio::time::Sleep>>
        });
        let mut auto_start = auto_start;

        loop {
            tokio::select! {
                () = async {
                    match auto_start.as_mut() {
                        Some(s) => s.await,
                        None => std::future::pending().await,
                    }
                } => {
                    auto_start = None;
                    if let Some(g) = &self.current_game {
                        tracing::info!("--start-after elapsed, starting the game");
                        g.send(GameCmd::Start { by: self.cfg.bnet.username.clone() });
                    } else {
                        tracing::warn!("--start-after elapsed but there is no lobby to start");
                    }
                }

                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("SIGINT received, shutting down gracefully");
                    self.shutdown().await;
                    break;
                }

                _ = lan_timer.tick() => {
                    self.broadcast_lan_game().await;
                }

                _ = cleanup_timer.tick() => {
                    self.clean_finished_games();
                    self.check_autohost();
                }

                Some((conn_id, stream, peer)) = self.listener_rx.recv() => {
                    self.handle_new_connection(conn_id, stream, peer);
                }

                Some(ev) = self.conn_event_rx.recv() => {
                    self.handle_conn_event(ev);
                }

                Some(bnet_ev) = self.bnet_events.recv() => {
                    self.handle_bnet_event(bnet_ev);
                }
            }
        }

        Ok(())
    }

    async fn broadcast_lan_game(&self) {
        if let (Some(u), Some(adv)) = (&self.udp_broadcaster, &self.current_game_advert)
            && let Ok(pkt) = game_info(
                self.cfg.bot.tft,
                self.cfg.bnet.war3_version,
                adv.host_counter,
                0,
                &adv.game_name,
                &adv.stat_string,
                12,
                adv.map_game_type,
                12,
                0,
                self.cfg.bot.host_port,
            )
        {
            let _ = u.send(&pkt).await;
        }
    }

    fn handle_new_connection(&mut self, conn_id: u64, stream: TcpStream, peer: SocketAddr) {
        if let Some(game) = &self.current_game {
            let external_ip = match peer.ip() {
                std::net::IpAddr::V4(v4) => v4.octets(),
                _ => [127, 0, 0, 1],
            };

            let ip_str = format!(
                "{}.{}.{}.{}",
                external_ip[0], external_ip[1], external_ip[2], external_ip[3]
            );
            let store = self.store.clone();
            let game_handle = game.clone();
            let conn_tx = self.conn_event_tx.clone();

            tokio::spawn(async move {
                if let Some(ban) = store.is_banned("", &ip_str).await {
                    tracing::info!(%ip_str, reason = %ban.reason, "rejected banned IP");
                    return;
                }
                let link = spawn_conn(conn_id, stream, conn_tx, 1024);
                game_handle.send(GameCmd::NewConn {
                    conn_id,
                    link,
                    external_ip,
                });
            });

            self.conn_to_game.insert(conn_id, game.clone());
        } else {
            tracing::debug!(conn_id, %peer, "connection dropped: no active lobby");
        }
    }

    fn handle_conn_event(&mut self, ev: ConnEvent) {
        let conn_id = ev.conn_id;
        if let Some(game) = self.conn_to_game.get(&conn_id) {
            game.send(GameCmd::Conn(ev));
        }
    }

    fn handle_bnet_event(&mut self, ev: BnetEvent) {
        match ev {
            BnetEvent::Connected => tracing::info!("connected to Battle.net"),
            BnetEvent::LoggedIn => {
                tracing::info!("logged in to Battle.net, standing by in channel");
                self.bnet.send(BnetCmd::SendChat("/motd".into()));
                self.bnet.send(BnetCmd::SendChat("/who".into()));
            }
            BnetEvent::ChatMessage { user, text } => self.handle_chat_command(&user, &text),
            BnetEvent::Whisper { user, text } => self.handle_chat_command(&user, &text),
            BnetEvent::Disconnected(reason) => {
                tracing::warn!(%reason, "disconnected from Battle.net")
            }
        }
    }

    fn handle_chat_command(&mut self, user: &str, text: &str) {
        let is_root_admin = self
            .cfg
            .bnet
            .root_admins
            .iter()
            .any(|a| a.eq_ignore_ascii_case(user));
        if !is_root_admin {
            return;
        }

        let trigger = self.cfg.bnet.command_trigger;
        let Some(cmd_text) = text.strip_prefix(trigger) else {
            return;
        };
        let mut parts = cmd_text.split_whitespace();
        let Some(verb) = parts.next() else { return };

        match verb.to_lowercase().as_str() {
            "pub" | "priv" => {
                let visibility = if verb.eq_ignore_ascii_case("pub") {
                    ghost_protocol::GameVisibility::Public
                } else {
                    ghost_protocol::GameVisibility::Private
                };
                let name = parts.collect::<Vec<_>>().join(" ");
                if name.is_empty() {
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Usage: !pub <game name>"
                    )));
                    return;
                }
                if self.running_games.len() >= self.cfg.bot.max_games {
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Error: maximum games reached"
                    )));
                    return;
                }
                self.create_game(&name, user, visibility);
            }
            "map" | "load" => {
                let map_name = parts.collect::<Vec<_>>().join(" ");
                if map_name.is_empty() {
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Usage: !map <filename>"
                    )));
                } else {
                    self.selected_map_file = Some(map_name.clone());
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Map set to [{map_name}]"
                    )));
                }
            }
            "autohost" => {
                let args: Vec<&str> = parts.collect();
                if args.first() == Some(&"off") {
                    self.autohost = None;
                    self.bnet
                        .send(BnetCmd::SendChat(format!("/w {user} Autohost disabled.")));
                } else if args.len() >= 2 {
                    let map_file = args[0].to_string();
                    let game_prefix = args[1..].join(" ");
                    self.autohost = Some(AutohostConfig {
                        map_file: map_file.clone(),
                        game_prefix: game_prefix.clone(),
                        max_games: self.cfg.bot.max_games,
                    });
                    self.selected_map_file = Some(map_file.clone());
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Autohost enabled for map [{map_file}] prefix [{game_prefix}]."
                    )));
                } else {
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Usage: !autohost <mapfile> <prefix> | !autohost off"
                    )));
                }
            }
            "unhost" => {
                if let Some(g) = self.current_game.take() {
                    g.send(GameCmd::Unhost);
                    self.current_game_name = None;
                    self.current_game_advert = None;
                    self.bnet.send(BnetCmd::UnhostGame);
                    self.bnet
                        .send(BnetCmd::SendChat(format!("/w {user} Game unhosted")));
                }
            }
            "start" => {
                if let Some(g) = &self.current_game {
                    g.send(GameCmd::Start {
                        by: user.to_string(),
                    });
                }
            }
            "say" => {
                let msg = parts.collect::<Vec<_>>().join(" ");
                self.bnet.send(BnetCmd::SendChat(msg));
            }
            "ban" => {
                let args: Vec<&str> = parts.collect();
                if let Some(target) = args.first() {
                    let reason = args
                        .get(1..)
                        .map(|r| r.join(" "))
                        .unwrap_or_else(|| "banned by admin".into());
                    self.store.ban(target, "", user, &reason);
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Banned [{target}]: {reason}"
                    )));
                }
            }
            "unban" => {
                if let Some(target) = parts.next() {
                    self.store.unban(target);
                    self.bnet
                        .send(BnetCmd::SendChat(format!("/w {user} Unbanned [{target}]")));
                }
            }
            "checkban" => {
                if let Some(target) = parts.next() {
                    let store = self.store.clone();
                    let bnet = self.bnet.clone();
                    let user_str = user.to_string();
                    let target_str = target.to_string();
                    tokio::spawn(async move {
                        if let Some(b) = store.is_banned(&target_str, "").await {
                            bnet.send(BnetCmd::SendChat(format!(
                                "/w {user_str} [{target_str}] was banned by [{}] for [{}]",
                                b.admin, b.reason
                            )));
                        } else {
                            bnet.send(BnetCmd::SendChat(format!(
                                "/w {user_str} [{target_str}] is not banned"
                            )));
                        }
                    });
                }
            }
            "statsdota" | "stats" => {
                let target = parts.next().unwrap_or(user).to_string();
                let store = self.store.clone();
                let bnet = self.bnet.clone();
                let user_str = user.to_string();
                tokio::spawn(async move {
                    if let Some(stats) = store.get_dota_stats(&target).await {
                        bnet.send(BnetCmd::SendChat(format!(
                            "/w {user_str} [{target}] Games: {}, K/D/A: {}/{}/{}, CS: {}/{}",
                            stats.games,
                            stats.kills,
                            stats.deaths,
                            stats.assists,
                            stats.creep_kills,
                            stats.creep_denies
                        )));
                    } else {
                        bnet.send(BnetCmd::SendChat(format!(
                            "/w {user_str} No stats recorded for [{target}]."
                        )));
                    }
                });
            }
            "status" => {
                let running = self.running_games.len();
                let active = if self.current_game.is_some() {
                    "1 lobby"
                } else {
                    "none"
                };
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Status: {running} active games, current lobby: {active}"
                )));
            }
            _ => {}
        }
    }

    fn resolve_map_info(
        &self,
        game_name: &str,
    ) -> (
        MapInfo,
        [u8; 4],
        Option<Vec<ghost_protocol::w3gs::SlotInfo>>,
    ) {
        let mut candidate_filenames = Vec::new();
        if let Some(sel) = &self.selected_map_file {
            candidate_filenames.push(sel.clone());
            if !sel.ends_with(".w3x") && !sel.ends_with(".w3m") {
                candidate_filenames.push(format!("{sel}.w3x"));
                candidate_filenames.push(format!("{sel}.w3m"));
            }
        }
        if let Some(def) = &self.cfg.bot.default_map {
            candidate_filenames.push(def.clone());
            if !def.ends_with(".w3x") && !def.ends_with(".w3m") {
                candidate_filenames.push(format!("{def}.w3x"));
                candidate_filenames.push(format!("{def}.w3m"));
            }
        }
        candidate_filenames.push(format!("{game_name}.w3x"));
        candidate_filenames.push(format!("{game_name}.w3m"));
        candidate_filenames.push(game_name.to_string());

        let common_j = std::fs::read("war3/common.j")
            .or_else(|_| std::fs::read("maps/common.j"))
            .ok();
        let blizzard_j = std::fs::read("war3/blizzard.j")
            .or_else(|_| std::fs::read("maps/blizzard.j"))
            .ok();

        for candidate_filename in &candidate_filenames {
            let candidate_paths = [
                PathBuf::from(&self.cfg.bot.map_path).join(candidate_filename),
                PathBuf::from("maps").join(candidate_filename),
                PathBuf::from(candidate_filename),
            ];

            for path in &candidate_paths {
                if path.exists()
                    && let Ok(parsed) =
                        ParsedMap::load_mpq(path, common_j.as_deref(), blizzard_j.as_deref())
                {
                    tracing::info!(
                        path = %path.display(),
                        crc = format!("0x{:08X}", parsed.info.crc),
                        size = parsed.info.size,
                        players = parsed.info.num_players,
                        "loaded map MPQ successfully"
                    );
                    let game_type = parsed.info.game_type.to_le_bytes();
                    return (parsed.info, game_type, Some(parsed.slots));
                }
            }
        }

        tracing::warn!(
            game = %game_name,
            "no valid MPQ map file found, using fallback map info"
        );

        let fallback_info = MapInfo {
            path: format!("Maps\\Download\\{game_name}.w3x"),
            size: 1000,
            info: 1,
            crc: 0x1234_5678,
            sha1: [0; 20],
            num_players: 12,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0x0000_0002 | 0x0000_0800 | 0x0000_4000 | 0x0006_0000,
            data: None,
            layout_style: 0,
        };
        (fallback_info, [1, 0, 0, 0], None)
    }

    fn create_game(&mut self, name: &str, owner: &str, visibility: ghost_protocol::GameVisibility) {
        let (map_info, map_game_type, custom_slots) = self.resolve_map_info(name);
        // `bnet.cpp:2247` splits the host counter: the low 28 bits identify the game and
        // the top nibble identifies which battle.net connection hosts it. We advertise on a
        // single connection, so that nibble is 0.
        const HOST_COUNTER_ID: u32 = 0;
        let host_counter: u32 = (rand::random::<u32>() & 0x0FFF_FFFF) | (HOST_COUNTER_ID << 28);

        let advert_map = MapAdvert {
            path: map_info.path.clone(),
            size: map_info.size,
            info: map_info.info,
            crc: map_info.crc,
            sha1: map_info.sha1,
            num_players: map_info.num_players,
            num_teams: map_info.num_teams,
            width: map_info.width,
            height: map_info.height,
            game_type: map_info.game_type,
            flags: map_info.flags,
        };

        let stat_string = encode_game_statstring(&advert_map, name, &self.cfg.bnet.username);

        let game_cfg = GameConfig {
            name: name.to_string(),
            owner: owner.to_string(),
            host_counter,
            num_slots: map_info.num_players as usize,
            latency: self.cfg.game.latency,
            sync_limit: self.cfg.game.sync_limit,
            map: map_info,
            virtual_host_name: self.cfg.game.virtual_host_name.clone(),
            reconnect_wait: self.cfg.game.reconnect_wait,
            custom_slots,
            replay_path: std::path::PathBuf::from(format!("replays/{}.w3g", name)),
            relay: self.spectator_relay.clone(),
        };

        let (handle, join) = spawn_game(game_cfg);
        handle.send(GameCmd::CreateVirtualHost);

        self.current_game = Some(handle.clone());
        self.current_game_name = Some(name.to_string());
        self.current_game_advert = Some(ActiveLobbyAdvert {
            game_name: name.to_string(),
            stat_string,
            host_counter,
            map_game_type,
        });

        self.running_games.push((name.to_string(), handle, join));

        self.bnet.send(BnetCmd::CreateGame {
            name: name.to_string(),
            map: advert_map,
            host_counter,
            visibility,
        });

        tracing::info!(game = %name, %owner, "game created and advertised on Battle.net and LAN");
    }

    fn clean_finished_games(&mut self) {
        self.running_games.retain(|(name, h, _)| {
            if h.is_closed() {
                tracing::info!(game = %name, "game actor closed; cleaned up game handle");
                false
            } else {
                true
            }
        });
        self.conn_to_game.retain(|_, h| !h.is_closed());
        if let Some(h) = &self.current_game
            && h.is_closed()
        {
            self.current_game = None;
            self.current_game_name = None;
            self.current_game_advert = None;
            self.bnet.send(BnetCmd::UnhostGame);
        }
    }

    fn check_autohost(&mut self) {
        let Some(auto) = &self.autohost else { return };
        if self.current_game.is_some() || self.running_games.len() >= auto.max_games {
            return;
        }
        let name = format!("{} #{}", auto.game_prefix, self.autohost_counter);
        let map_file = auto.map_file.clone();
        self.autohost_counter += 1;
        self.selected_map_file = Some(map_file);
        let bot_name = self.cfg.bnet.username.clone();
        self.create_game(&name, &bot_name, ghost_protocol::GameVisibility::Public);
    }

    async fn shutdown(&mut self) {
        self.bnet.send(BnetCmd::Shutdown);
        for (_, h, _) in &self.running_games {
            h.send(GameCmd::Shutdown);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    #[test]
    fn host_counter_top_nibble_is_the_connection_id() {
        const HOST_COUNTER_ID: u32 = 0;
        let mut seen = HashSet::new();
        for _ in 0..1000 {
            let counter: u32 = (rand::random::<u32>() & 0x0FFF_FFFF) | (HOST_COUNTER_ID << 28);
            assert_eq!(counter >> 28, 0, "top nibble must equal connection ID 0");
            seen.insert(counter);
        }
        assert!(
            seen.len() > 1,
            "host_counter must produce distinct random values, got {}",
            seen.len()
        );
    }

    #[test]
    fn autohost_config_stores_settings() {
        let auto = super::AutohostConfig {
            map_file: "dota.w3x".into(),
            game_prefix: "Dota AP".into(),
            max_games: 5,
        };
        assert_eq!(auto.map_file, "dota.w3x");
        assert_eq!(auto.game_prefix, "Dota AP");
        assert_eq!(auto.max_games, 5);
    }
}
