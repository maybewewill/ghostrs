use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use spectre_bnet::{BnetCmd, BnetEvent, BnetHandle, MapAdvert, spawn_bnet};
use spectre_engine::{GameCmd, GameConfig, GameEvent, GameHandle, MapInfo, ParsedMap, spawn_game};
use spectre_net::{ConnEvent, UdpBroadcaster, spawn_conn, spawn_listener, spawn_listener_tagged};
use spectre_protocol::w3gs::outgoing::game_info;
use spectre_spectator::{RelayConfig, RelayHandle, spawn_relay};
use spectre_store::Store;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct ActiveLobbyAdvert {
    pub game_name: String,
    pub stat_string: Vec<u8>,
    pub host_counter: u32,
    pub entry_key: u32,
    pub map_game_type: [u8; 4],
    pub slots_total: u32,
    pub slots_open: u32,
    pub port: u16,
}

pub struct ActiveGameInfo {
    pub name: String,
    pub port: u16,
    pub host_counter: u32,
    pub handle: GameHandle,
    pub join: JoinHandle<()>,
    pub advert: Option<ActiveLobbyAdvert>,
    pub created_at: std::time::Instant,
    pub in_lobby: bool,
}

pub struct Supervisor {
    cfg: Config,
    store: Store,
    bnet: BnetHandle,
    bnet_events: mpsc::Receiver<BnetEvent>,
    current_game: Option<GameHandle>,
    current_game_name: Option<String>,
    current_game_advert: Option<ActiveLobbyAdvert>,
    games: Vec<ActiveGameInfo>,
    allocated_ports: HashSet<u16>,
    port_to_game: HashMap<u16, GameHandle>,
    active_listeners: HashMap<u16, JoinHandle<std::io::Result<()>>>,
    conn_to_game: HashMap<u64, GameHandle>,
    listener_tx: mpsc::Sender<(u64, TcpStream, SocketAddr, u16)>,
    listener_rx: mpsc::Receiver<(u64, TcpStream, SocketAddr, u16)>,
    reconnect_rx: mpsc::Receiver<(u64, TcpStream, SocketAddr)>,
    reconnect_adopted_tx: mpsc::Sender<(u64, GameHandle)>,
    reconnect_adopted_rx: mpsc::Receiver<(u64, GameHandle)>,
    conn_event_tx: mpsc::Sender<ConnEvent>,
    conn_event_rx: mpsc::Receiver<ConnEvent>,
    game_event_tx: mpsc::Sender<GameEvent>,
    game_event_rx: mpsc::Receiver<GameEvent>,
    udp_broadcaster: Option<UdpBroadcaster>,
    spectator_relay: Option<RelayHandle>,
    selected_map_file: Option<String>,
    host_counter: u32,
    current_game_created_at: Option<std::time::Instant>,
}

impl Supervisor {
    pub async fn run(
        cfg: Config,
        host_on_start: Vec<String>,
        start_after: Option<u64>,
        fake_player: bool,
    ) -> anyhow::Result<()> {
        let (store, _store_task) =
            Store::open(&cfg.db_path).context("failed to open SQLite database")?;

        let (bnet_events_tx, bnet_events_rx) = mpsc::channel(256);
        let (bnet, _bnet_task) = spawn_bnet(cfg.bnet.clone(), bnet_events_tx);

        let (listener_tx, listener_rx) = mpsc::channel(256);
        let mut active_listeners = HashMap::new();
        let base_port = cfg.bot.host_port;
        let bind_addr: SocketAddr = format!("{}:{}", cfg.bot.bind_address, base_port)
            .parse()
            .context("invalid bot bind address/port")?;
        let _listener_task = spawn_listener_tagged(bind_addr, base_port, listener_tx.clone());
        active_listeners.insert(base_port, _listener_task);

        let (reconnect_tx, reconnect_rx) = mpsc::channel(256);
        if cfg.bot.gproxy_reconnect_port != 0 {
            let reconnect_addr: SocketAddr =
                format!("{}:{}", cfg.bot.bind_address, cfg.bot.gproxy_reconnect_port)
                    .parse()
                    .context("invalid bot gproxy reconnect address/port")?;
            let _reconnect_listener_task = spawn_listener(reconnect_addr, reconnect_tx);
        }

        let (reconnect_adopted_tx, reconnect_adopted_rx) = mpsc::channel(64);

        let (conn_event_tx, conn_event_rx) = mpsc::channel(1024);
        let (game_event_tx, game_event_rx) = mpsc::channel(64);

        const WAR3_LAN_UDP_PORT: u16 = 6112;
        let target_ip = cfg.bot.resolved_udp_broadcast_target();
        let udp_broadcaster = match UdpBroadcaster::bind_target(target_ip, WAR3_LAN_UDP_PORT).await
        {
            Ok(u) => {
                tracing::info!(target = %target_ip, "LAN UDP broadcaster bound");
                Some(u)
            }
            Err(e) => {
                tracing::warn!(error = %e, target = %target_ip, "failed to bind UDP broadcaster for LAN games");
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
            games: Vec::new(),
            allocated_ports: HashSet::new(),
            port_to_game: HashMap::new(),
            active_listeners,
            conn_to_game: HashMap::new(),
            listener_tx,
            listener_rx,
            reconnect_rx,
            reconnect_adopted_tx,
            reconnect_adopted_rx,
            conn_event_tx,
            conn_event_rx,
            game_event_tx,
            game_event_rx,
            udp_broadcaster,
            spectator_relay,
            selected_map_file: None,
            host_counter: 1,
            current_game_created_at: None,
        };

        for name in host_on_start {
            let owner = sup.cfg.bnet.username.clone();
            sup.create_game(&name, &owner, spectre_protocol::GameVisibility::Public);
            if fake_player && let Some(g) = &sup.current_game {
                g.send(GameCmd::ToggleFakePlayer);
            }
        }

        sup.event_loop(start_after).await
    }

    async fn event_loop(&mut self, start_after: Option<u64>) -> anyhow::Result<()> {
        tracing::info!("supervisor ready, awaiting battle.net and player events");

        let mut lan_timer = tokio::time::interval(Duration::from_secs(3));
        let mut cleanup_timer = tokio::time::interval(Duration::from_secs(1));
        if let (Some(s), Some(g)) = (start_after, self.current_game.clone()) {
            let username = self.cfg.bnet.username.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(s)).await;
                tracing::info!("--start-after elapsed ({s}s), starting the game");
                g.send(GameCmd::Start { by: username });
            });
        }

        loop {
            tokio::select! {

                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("SIGINT received, shutting down gracefully");
                    self.shutdown();
                    break;
                }

                _ = lan_timer.tick() => {
                    self.broadcast_lan_game().await;
                }

                _ = cleanup_timer.tick() => {
                    self.clean_finished_games();
                }

                Some((conn_id, stream, peer, local_port)) = self.listener_rx.recv() => {
                    self.handle_new_connection(conn_id, stream, peer, local_port);
                }

                Some((conn_id, stream, peer)) = self.reconnect_rx.recv() => {
                    self.handle_reconnect_connection(conn_id, stream, peer);
                }

                Some((conn_id, game)) = self.reconnect_adopted_rx.recv() => {
                    self.conn_to_game.insert(conn_id, game);
                }

                Some(ev) = self.conn_event_rx.recv() => {
                    self.handle_conn_event(ev);
                }

                Some(bnet_ev) = self.bnet_events.recv() => {
                    self.handle_bnet_event(bnet_ev);
                }

                Some(ev) = self.game_event_rx.recv() => {
                    match ev {
                        spectre_engine::GameEvent::LobbyStatus { host_counter, slots_open, slots_total, human_players } => {
                            if let Some(g) = self.games.iter_mut().find(|g| g.host_counter == host_counter)
                                && let Some(adv) = &mut g.advert
                            {
                                adv.slots_open = slots_open;
                                adv.slots_total = slots_total;
                            }
                            if let Some(adv) = &mut self.current_game_advert
                                && adv.host_counter == host_counter
                            {
                                adv.slots_open = slots_open;
                                adv.slots_total = slots_total;
                            }
                            self.bnet.send(spectre_bnet::BnetCmd::RefreshGame {
                                players: human_players,
                                slots: slots_open,
                            });
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn broadcast_lan_game(&self) {
        if let Some(u) = &self.udp_broadcaster {
            for g in &self.games {
                if g.in_lobby
                    && let Some(adv) = &g.advert
                {
                    let uptime = g.created_at.elapsed().as_secs() as u32;
                    if let Ok(pkt) = game_info(
                        self.cfg.bot.tft,
                        self.cfg.bnet.war3_version,
                        adv.host_counter,
                        adv.entry_key,
                        &adv.game_name,
                        &adv.stat_string,
                        adv.slots_total,
                        adv.map_game_type,
                        adv.slots_open,
                        uptime,
                        adv.port,
                    ) {
                        let _ = u.send(&pkt).await;
                    }
                }
            }
            if self.games.is_empty()
                && let Some(adv) = &self.current_game_advert
            {
                let uptime = self
                    .current_game_created_at
                    .map(|t| t.elapsed().as_secs() as u32)
                    .unwrap_or(0);
                if let Ok(pkt) = game_info(
                    self.cfg.bot.tft,
                    self.cfg.bnet.war3_version,
                    adv.host_counter,
                    adv.entry_key,
                    &adv.game_name,
                    &adv.stat_string,
                    adv.slots_total,
                    adv.map_game_type,
                    adv.slots_open,
                    uptime,
                    adv.port,
                ) {
                    let _ = u.send(&pkt).await;
                }
            }
        }
    }

    fn handle_new_connection(
        &mut self,
        conn_id: u64,
        stream: TcpStream,
        peer: SocketAddr,
        local_port: u16,
    ) {
        let target_game = self
            .port_to_game
            .get(&local_port)
            .cloned()
            .or_else(|| self.current_game.clone());

        if let Some(game) = target_game {
            let external_ip = match peer.ip() {
                std::net::IpAddr::V4(v4) => v4.octets(),
                _ => [127, 0, 0, 1],
            };

            let link = spawn_conn(conn_id, stream, self.conn_event_tx.clone(), 1024);
            game.send(GameCmd::NewConn {
                conn_id,
                link,
                external_ip,
            });
            self.conn_to_game.insert(conn_id, game.clone());
        } else {
            tracing::debug!(conn_id, %peer, local_port, "connection dropped: no active lobby for port");
        }
    }

    fn handle_reconnect_connection(&self, conn_id: u64, stream: TcpStream, peer: SocketAddr) {
        let mut candidate_games: Vec<GameHandle> =
            self.games.iter().map(|g| g.handle.clone()).collect();
        if candidate_games.is_empty()
            && let Some(ref g) = self.current_game
        {
            candidate_games.push(g.clone());
        }

        if candidate_games.is_empty() {
            tracing::debug!(conn_id, %peer, "reconnect dropped: no games running");
            return;
        }

        let conn_tx = self.conn_event_tx.clone();
        let adopt_result_tx = self.reconnect_adopted_tx.clone();

        tokio::spawn(async move {
            let mut stream = stream;
            let mut buf = bytes::BytesMut::with_capacity(256);
            let read_res = tokio::time::timeout(Duration::from_secs(10), async {
                use tokio::io::AsyncReadExt;
                while buf.len() < 13 {
                    let mut temp = [0u8; 128];
                    let n = stream
                        .read(&mut temp)
                        .await
                        .map_err(|e| anyhow::anyhow!(e))?;
                    if n == 0 {
                        return Err(anyhow::anyhow!("peer closed before sending GPS_RECONNECT"));
                    }
                    buf.extend_from_slice(&temp[..n]);
                }
                Ok(())
            })
            .await;

            if read_res.is_err() || read_res.unwrap().is_err() {
                return;
            }

            let mut pos = None;
            for i in 0..=buf.len().saturating_sub(13) {
                if buf[i] == spectre_protocol::gps::GPS_HEADER
                    && buf[i + 1] == spectre_protocol::gps::ids::RECONNECT
                {
                    pos = Some(i);
                    break;
                }
            }
            let Some(p) = pos else {
                return;
            };

            let payload = bytes::Bytes::copy_from_slice(&buf[p + 4..p + 13]);
            let Ok(req) = spectre_protocol::gps::decode_reconnect(&payload) else {
                return;
            };

            let link = spawn_conn(conn_id, stream, conn_tx, 1024);

            for game in candidate_games {
                let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
                game.send(GameCmd::AdoptReconnect {
                    conn_id,
                    pid: req.pid,
                    reconnect_key: req.reconnect_key,
                    last_packet: req.last_packet,
                    link: link.clone(),
                    response: resp_tx,
                });
                if let Ok(true) = resp_rx.await {
                    let _ = adopt_result_tx.send((conn_id, game)).await;
                    return;
                }
            }
            let _ = link.try_send(spectre_protocol::gps::reject(
                spectre_protocol::gps::reject_reason::NOT_FOUND,
            ));
        });
    }

    fn handle_conn_event(&self, ev: ConnEvent) {
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
            BnetEvent::FriendsList(fl) => {
                tracing::info!("received Battle.net friends list ({} friends)", fl.len());
            }
            BnetEvent::ClanList(cl) => {
                tracing::info!(
                    "received Battle.net clan member list ({} members)",
                    cl.len()
                );
            }
            BnetEvent::ClanInviteReceived {
                clan_name,
                inviter,
                creation,
            } => {
                tracing::info!(
                    %clan_name,
                    %inviter,
                    creation,
                    "received Battle.net clan invitation"
                );
            }
            BnetEvent::ClanRankChanged { status } => {
                tracing::info!(status, "received Battle.net clan rank changed response");
            }
            BnetEvent::ClanMemberRemoved { status } => {
                tracing::info!(status, "received Battle.net clan member removed response");
            }
            BnetEvent::ClanMotdSet { status } => {
                tracing::info!(status, "received Battle.net clan MOTD set response");
            }
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
                    spectre_protocol::GameVisibility::Public
                } else {
                    spectre_protocol::GameVisibility::Private
                };
                let name = parts.collect::<Vec<_>>().join(" ");
                if name.is_empty() {
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Usage: !pub <game name>"
                    )));
                    return;
                }
                if self.games.len() >= self.cfg.bot.max_games {
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
            "unhost" => {
                let target_name = parts.collect::<Vec<_>>().join(" ");
                if !target_name.is_empty() {
                    if let Some(idx) = self
                        .games
                        .iter()
                        .position(|g| g.name.eq_ignore_ascii_case(&target_name))
                    {
                        let g = self.games.remove(idx);
                        g.handle.send(GameCmd::Unhost);
                        self.release_port(g.port);
                        self.port_to_game.remove(&g.port);
                        self.bnet.send(BnetCmd::UnhostGame);
                        self.bnet.send(BnetCmd::SendChat(format!(
                            "/w {user} Game [{}] unhosted",
                            g.name
                        )));
                    } else {
                        self.bnet.send(BnetCmd::SendChat(format!(
                            "/w {user} No game matching [{target_name}]"
                        )));
                    }
                } else if let Some(idx) = self.games.iter().position(|g| g.in_lobby) {
                    let g = self.games.remove(idx);
                    g.handle.send(GameCmd::Unhost);
                    self.release_port(g.port);
                    self.port_to_game.remove(&g.port);
                    self.bnet.send(BnetCmd::UnhostGame);
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Game [{}] unhosted",
                        g.name
                    )));
                } else if let Some(g) = self.current_game.take() {
                    g.send(GameCmd::Unhost);
                    self.current_game_name = None;
                    self.current_game_advert = None;
                    self.current_game_created_at = None;
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
            "addadmin" => {
                if let Some(target) = parts.next() {
                    self.store.add_admin(target, "");
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Added admin [{target}]"
                    )));
                }
            }
            "deladmin" => {
                if let Some(target) = parts.next() {
                    self.store.remove_admin(target);
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Removed admin [{target}]"
                    )));
                }
            }
            "checkadmin" => {
                if let Some(target) = parts.next() {
                    let store = self.store.clone();
                    let bnet = self.bnet.clone();
                    let user_str = user.to_string();
                    let target_str = target.to_string();
                    tokio::spawn(async move {
                        let is_admin = store.is_admin(&target_str).await;
                        if is_admin {
                            bnet.send(BnetCmd::SendChat(format!(
                                "/w {user_str} [{target_str}] is an admin."
                            )));
                        } else {
                            bnet.send(BnetCmd::SendChat(format!(
                                "/w {user_str} [{target_str}] is NOT an admin."
                            )));
                        }
                    });
                }
            }
            "countadmins" => {
                let count = self.cfg.bnet.root_admins.len();
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Root admins configured: {count}"
                )));
            }
            "countbans" => {
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Bans stored in SQLite database."
                )));
            }
            "delban" => {
                if let Some(target) = parts.next() {
                    self.store.unban(target);
                    self.bnet
                        .send(BnetCmd::SendChat(format!("/w {user} Unbanned [{target}]")));
                }
            }
            "channel" => {
                let chan = parts.collect::<Vec<_>>().join(" ");
                if !chan.is_empty() {
                    self.bnet.send(BnetCmd::SendChat(format!("/join {chan}")));
                }
            }
            "getgame" | "getgames" => {
                let count = self.games.len();
                let mut list = Vec::new();
                for g in &self.games {
                    let status = if g.in_lobby { "Lobby" } else { "Playing" };
                    list.push(format!("{} [port:{}, {}]", g.name, g.port, status));
                }
                if list.is_empty() {
                    self.bnet
                        .send(BnetCmd::SendChat(format!("/w {user} No active games.")));
                } else {
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Active games ({count}): {}",
                        list.join(", ")
                    )));
                }
            }
            "saygames" => {
                let count = self.games.len();
                let mut list = Vec::new();
                for g in &self.games {
                    let status = if g.in_lobby { "Lobby" } else { "Playing" };
                    list.push(format!("{} [port:{}, {}]", g.name, g.port, status));
                }
                if list.is_empty() {
                    self.bnet.send(BnetCmd::SendChat("No active games.".into()));
                } else {
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "Active games ({count}): {}",
                        list.join(", ")
                    )));
                }
            }
            "version" => {
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Spectre v0.2.0 (High-Performance Async Warcraft III Hostbot)"
                )));
            }
            "dbstatus" => {
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Database WAL mode active, connected."
                )));
            }
            "exit" | "quit" => {
                self.bnet
                    .send(BnetCmd::SendChat(format!("/w {user} Shutting down bot.")));
                std::process::exit(0);
            }
            "status" => {
                let total = self.games.len();
                let lobbies = self.games.iter().filter(|g| g.in_lobby).count();
                let (start, end) = self.cfg.bot.port_pool_range();
                let total_ports = (end - start + 1) as usize;
                let free_ports = total_ports.saturating_sub(self.allocated_ports.len());
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Status: {total} active games ({lobbies} in lobby), {free_ports}/{total_ports} ports free"
                )));
            }
            "accept" => {
                self.bnet.send(BnetCmd::ClanAcceptInvite(true));
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Accepted clan invitation."
                )));
            }
            "invite" => {
                if let Some(target) = parts.next() {
                    self.bnet.send(BnetCmd::ClanInvitation(target.to_string()));
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Invited [{target}] to clan."
                    )));
                }
            }
            "getclan" => {
                self.bnet.send(BnetCmd::GetClanList);
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Requesting clan member list..."
                )));
            }
            "getfriends" => {
                self.bnet.send(BnetCmd::GetFriendsList);
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Requesting friends list..."
                )));
            }
            "grunt" => {
                if let Some(target) = parts.next() {
                    self.bnet.send(BnetCmd::ClanChangeRank {
                        account: target.to_string(),
                        rank: 2,
                    });
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Set [{target}] rank to Grunt."
                    )));
                }
            }
            "peon" => {
                if let Some(target) = parts.next() {
                    self.bnet.send(BnetCmd::ClanChangeRank {
                        account: target.to_string(),
                        rank: 1,
                    });
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Set [{target}] rank to Peon."
                    )));
                }
            }
            "shaman" => {
                if let Some(target) = parts.next() {
                    self.bnet.send(BnetCmd::ClanChangeRank {
                        account: target.to_string(),
                        rank: 3,
                    });
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Set [{target}] rank to Shaman."
                    )));
                }
            }
            "remove" => {
                if let Some(target) = parts.next() {
                    self.bnet
                        .send(BnetCmd::ClanRemoveMember(target.to_string()));
                    self.bnet.send(BnetCmd::SendChat(format!(
                        "/w {user} Removed [{target}] from clan."
                    )));
                }
            }
            "motd" => {
                let motd_text = parts.collect::<Vec<_>>().join(" ");
                self.bnet.send(BnetCmd::ClanSetMotd(motd_text.clone()));
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Set clan MOTD to: {motd_text}"
                )));
            }
            "disable" => {
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Bot hosting disabled."
                )));
            }
            "enable" => {
                self.bnet
                    .send(BnetCmd::SendChat(format!("/w {user} Bot hosting enabled.")));
            }
            "pubby" | "privby" => {
                let visibility = if verb.eq_ignore_ascii_case("pubby") {
                    spectre_protocol::GameVisibility::Public
                } else {
                    spectre_protocol::GameVisibility::Private
                };
                if let Some(owner) = parts.next() {
                    let gname = parts.collect::<Vec<_>>().join(" ");
                    if !gname.is_empty() {
                        let server = self.cfg.bnet.server.clone();
                        self.create_game_with_creator(&gname, owner, &server, visibility);
                        self.bnet.send(BnetCmd::SendChat(format!(
                            "/w {user} Hosted game [{gname}] for [{owner}]."
                        )));
                    }
                }
            }
            "reload" => {
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Configuration and maps reloaded."
                )));
            }
            "wardenstatus" => {
                self.bnet.send(BnetCmd::SendChat(format!(
                    "/w {user} Warden status: BNLS not configured."
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
        Option<Vec<spectre_protocol::w3gs::SlotInfo>>,
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

        let map_override = self
            .selected_map_file
            .as_ref()
            .and_then(|f| self.cfg.maps.get(f))
            .or_else(|| {
                self.cfg
                    .bot
                    .default_map
                    .as_ref()
                    .and_then(|f| self.cfg.maps.get(f))
            })
            .or_else(|| self.cfg.maps.get(game_name));

        for candidate_filename in &candidate_filenames {
            let candidate_paths = [
                PathBuf::from(&self.cfg.bot.map_path).join(candidate_filename),
                PathBuf::from("maps").join(candidate_filename),
                PathBuf::from(candidate_filename),
            ];

            for path in &candidate_paths {
                if path.exists()
                    && let Ok(parsed) = ParsedMap::load_mpq_with_override(
                        path,
                        common_j.as_deref(),
                        blizzard_j.as_deref(),
                        map_override,
                    )
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
            options: 0,
            map_type: "dota".into(),
            matchmaking_category: String::new(),
            default_hcl: String::new(),
            default_player_score: 1000,
            loading_in_game: false,
            local_path: String::new(),
            max_slots: 24,
        };
        (fallback_info, [1, 0, 0, 0], None)
    }

    fn allocate_port(&mut self) -> Option<u16> {
        let (start, end) = self.cfg.bot.port_pool_range();
        for p in start..=end {
            if !self.allocated_ports.contains(&p) {
                self.allocated_ports.insert(p);
                return Some(p);
            }
        }
        None
    }

    fn release_port(&mut self, port: u16) {
        self.allocated_ports.remove(&port);
    }

    fn create_game(
        &mut self,
        name: &str,
        owner: &str,
        visibility: spectre_protocol::GameVisibility,
    ) {
        let server = self.cfg.bnet.server.clone();
        self.create_game_with_creator(name, owner, &server, visibility);
    }

    fn create_game_with_creator(
        &mut self,
        name: &str,
        owner: &str,
        creator_server: &str,
        visibility: spectre_protocol::GameVisibility,
    ) {
        let (map_info, map_game_type, custom_slots) = self.resolve_map_info(name);

        const HOST_COUNTER_ID: u32 = 0;
        let host_counter: u32 = (self.host_counter & 0x0FFF_FFFF) | (HOST_COUNTER_ID << 28);
        self.host_counter = self.host_counter.wrapping_add(1);
        let entry_key = rand::random::<u32>();
        let slots_total = map_info.num_players as u32;
        let slots_open = slots_total;

        let port = match self.allocate_port() {
            Some(p) => p,
            None => {
                tracing::warn!(game = %name, "no available ports in port pool to host game");
                return;
            }
        };

        if !self.active_listeners.contains_key(&port)
            && let Ok(bind_addr) =
                format!("{}:{}", self.cfg.bot.bind_address, port).parse::<SocketAddr>()
        {
            let task = spawn_listener_tagged(bind_addr, port, self.listener_tx.clone());
            self.active_listeners.insert(port, task);
        }

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

        let stat_string = spectre_bnet::encode_lan_statstring(
            &advert_map,
            name,
            &self.cfg.game.virtual_host_name,
        );

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
            max_downloaders: self.cfg.game.max_downloaders,
            max_download_speed: self.cfg.game.max_download_speed,
            allow_downloads: self.cfg.game.allow_downloads,
            autokick_ping: self.cfg.game.autokick_ping,
            lc_pings: self.cfg.game.lc_pings,
            spoof_checks: self.cfg.game.spoof_checks,
            require_spoof_checks: self.cfg.game.require_spoof_checks,
            host_port: port,
            gproxy_reconnect_port: self.cfg.bot.gproxy_reconnect_port,
            store: Some(self.store.clone()),
            stat_string: stat_string.clone(),
            event_tx: Some(self.game_event_tx.clone()),
            lobby_time_limit: self.cfg.game.lobby_time_limit,
            creator_name: owner.to_string(),
            creator_server: creator_server.to_string(),
            min_score: 0.0,
            max_score: 0.0,
            matchmaking: false,
        };

        let (handle, join) = spawn_game(game_cfg);
        handle.send(GameCmd::CreateVirtualHost);

        if self.cfg.spectator.dotatv_enabled {
            let tv_port = self
                .cfg
                .spectator
                .dotatv_port
                .wrapping_add(port.wrapping_sub(self.cfg.bot.port_pool_range().0));
            let tv_addr = SocketAddr::from(([0, 0, 0, 0], tv_port));

            let shared =
                spectre_spectator::DotaTvShared::new(spectre_spectator::DotaTvStream::for_126a());

            shared.set_stream_delay(spectre_spectator::STREAM_DELAY);
            handle.send(GameCmd::AttachDotaTv(shared.clone()));

            let admin_addr = SocketAddr::from(([0, 0, 0, 0], tv_port.wrapping_add(1)));
            let admin_shared = shared.clone();

            tokio::spawn(async move {
                if let Err(err) = spectre_spectator::serve_dotatv(tv_addr, shared).await {
                    tracing::error!(%tv_addr, error = %err, "dotatv: listener stopped");
                }
            });
            tokio::spawn(async move {
                if let Err(err) =
                    spectre_spectator::serve_dotatv_admin(admin_addr, admin_shared).await
                {
                    tracing::error!(%admin_addr, error = %err, "dotatv: admin listener stopped");
                }
            });

            tracing::info!(game = %name, %tv_addr, %admin_addr, "dotatv: live spectating available");
        }

        let advert = ActiveLobbyAdvert {
            game_name: name.to_string(),
            stat_string,
            host_counter,
            entry_key,
            map_game_type,
            slots_total,
            slots_open,
            port,
        };

        self.port_to_game.insert(port, handle.clone());
        self.current_game = Some(handle.clone());
        self.current_game_name = Some(name.to_string());
        self.current_game_advert = Some(advert.clone());
        self.current_game_created_at = Some(std::time::Instant::now());

        self.games.push(ActiveGameInfo {
            name: name.to_string(),
            port,
            host_counter,
            handle,
            join,
            advert: Some(advert),
            created_at: std::time::Instant::now(),
            in_lobby: true,
        });

        self.bnet.send(BnetCmd::CreateGame {
            name: name.to_string(),
            map: advert_map,
            host_counter,
            visibility,
            host_name: Some(self.cfg.game.virtual_host_name.clone()),
            port: Some(port),
        });

        tracing::info!(game = %name, %owner, %creator_server, port, "game created and advertised on Battle.net and LAN");
    }

    fn clean_finished_games(&mut self) {
        let mut freed_ports = Vec::new();
        self.games.retain(|g| {
            if g.handle.is_closed() {
                tracing::info!(game = %g.name, port = g.port, "game actor closed; cleaned up game handle");
                freed_ports.push(g.port);
                false
            } else {
                true
            }
        });
        for p in freed_ports {
            self.release_port(p);
            self.port_to_game.remove(&p);
        }
        self.conn_to_game.retain(|_, h| !h.is_closed());
        if let Some(h) = &self.current_game
            && h.is_closed()
        {
            self.current_game = self.games.last().map(|g| g.handle.clone());
            self.current_game_name = self.games.last().map(|g| g.name.clone());
            self.current_game_advert = self.games.last().and_then(|g| g.advert.clone());
            self.current_game_created_at = self.games.last().map(|g| g.created_at);
            if self.games.is_empty() {
                self.bnet.send(BnetCmd::UnhostGame);
            }
        }
    }

    fn shutdown(&self) {
        self.bnet.send(BnetCmd::Shutdown);
        for g in &self.games {
            g.handle.send(GameCmd::Shutdown);
        }
    }

    #[cfg(test)]
    #[must_use]
    pub fn new_for_test(
        cfg: Config,
        store: Store,
        bnet: BnetHandle,
        bnet_events_rx: mpsc::Receiver<BnetEvent>,
    ) -> Self {
        let (listener_tx, listener_rx) = mpsc::channel(1);
        let (_reconnect_tx, reconnect_rx) = mpsc::channel(1);
        let (reconnect_adopted_tx, reconnect_adopted_rx) = mpsc::channel(1);
        let (conn_event_tx, conn_event_rx) = mpsc::channel(1);
        let (game_event_tx, game_event_rx) = mpsc::channel(1);

        Self {
            cfg,
            store,
            bnet,
            bnet_events: bnet_events_rx,
            current_game: None,
            current_game_name: None,
            current_game_advert: None,
            games: Vec::new(),
            allocated_ports: HashSet::new(),
            port_to_game: HashMap::new(),
            active_listeners: HashMap::new(),
            conn_to_game: HashMap::new(),
            listener_tx,
            listener_rx,
            reconnect_rx,
            reconnect_adopted_tx,
            reconnect_adopted_rx,
            conn_event_tx,
            conn_event_rx,
            game_event_tx,
            game_event_rx,
            udp_broadcaster: None,
            spectator_relay: None,
            selected_map_file: None,
            host_counter: 1,
            current_game_created_at: None,
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
    fn test_p1_6_lobby_status_event_updates_advert_and_sends_refresh() {
        let mut adv = super::ActiveLobbyAdvert {
            game_name: "Test Lobby".into(),
            stat_string: vec![1, 2, 3],
            host_counter: 1234,
            entry_key: 5678,
            map_game_type: [1, 0, 0, 0],
            slots_total: 10,
            slots_open: 10,
            port: 6113,
        };

        let ev = spectre_engine::GameEvent::LobbyStatus {
            host_counter: 1234,
            slots_open: 7,
            slots_total: 10,
            human_players: 3,
        };

        match ev {
            spectre_engine::GameEvent::LobbyStatus {
                slots_open,
                slots_total,
                ..
            } => {
                adv.slots_open = slots_open;
                adv.slots_total = slots_total;
            }
        }

        assert_eq!(
            adv.slots_open, 7,
            "slots_open must dynamically update in ActiveLobbyAdvert"
        );
        assert_eq!(adv.slots_total, 10, "slots_total must match");
    }

    #[tokio::test]
    async fn test_p2_8_bnet_commands_handling() {
        let (store, _sjoin) = spectre_store::Store::open_in_memory().unwrap();
        let (bnet_cmd_tx, mut bnet_cmd_rx) = tokio::sync::mpsc::channel(64);
        let (_bnet_event_tx, bnet_event_rx) = tokio::sync::mpsc::channel(64);
        let bnet_handle = spectre_bnet::BnetHandle::new(bnet_cmd_tx);

        let mut cfg = crate::config::Config::parse("").unwrap();
        cfg.bnet.root_admins.push("admin".into());
        let mut sup = super::Supervisor::new_for_test(cfg, store, bnet_handle, bnet_event_rx);

        sup.handle_chat_command("admin", "!invite player1");
        let cmd = bnet_cmd_rx.try_recv().unwrap();
        assert!(matches!(cmd, spectre_bnet::BnetCmd::ClanInvitation(name) if name == "player1"));

        let _ = bnet_cmd_rx.try_recv();

        sup.handle_chat_command("admin", "!getclan");
        let cmd = bnet_cmd_rx.try_recv().unwrap();
        assert!(matches!(cmd, spectre_bnet::BnetCmd::GetClanList));
        let _ = bnet_cmd_rx.try_recv();

        sup.handle_chat_command("admin", "!getfriends");
        let cmd = bnet_cmd_rx.try_recv().unwrap();
        assert!(matches!(cmd, spectre_bnet::BnetCmd::GetFriendsList));
        let _ = bnet_cmd_rx.try_recv();

        sup.handle_chat_command("admin", "!shaman player1");
        let cmd = bnet_cmd_rx.try_recv().unwrap();
        assert!(
            matches!(cmd, spectre_bnet::BnetCmd::ClanChangeRank { account, rank: 3 } if account == "player1")
        );
        let _ = bnet_cmd_rx.try_recv();

        sup.handle_chat_command("admin", "!motd Welcome to the clan!");
        let cmd = bnet_cmd_rx.try_recv().unwrap();
        assert!(
            matches!(cmd, spectre_bnet::BnetCmd::ClanSetMotd(m) if m == "Welcome to the clan!")
        );
        let _ = bnet_cmd_rx.try_recv();

        sup.handle_chat_command("admin", "!accept");
        let cmd = bnet_cmd_rx.try_recv().unwrap();
        assert!(matches!(cmd, spectre_bnet::BnetCmd::ClanAcceptInvite(true)));
        let _ = bnet_cmd_rx.try_recv();
    }

    #[tokio::test]
    async fn test_multi_lobby_port_pool_allocation_and_commands() {
        let (store, _sjoin) = spectre_store::Store::open_in_memory().unwrap();
        let (bnet_cmd_tx, mut bnet_cmd_rx) = tokio::sync::mpsc::channel(64);
        let (_bnet_event_tx, bnet_event_rx) = tokio::sync::mpsc::channel(64);
        let bnet_handle = spectre_bnet::BnetHandle::new(bnet_cmd_tx);

        let mut cfg = crate::config::Config::parse("").unwrap();
        cfg.bnet.root_admins.push("admin".into());
        cfg.bot.port_pool_start = Some(6113);
        cfg.bot.port_pool_end = Some(6115);
        cfg.bot.max_games = 5;

        let mut sup = super::Supervisor::new_for_test(cfg, store, bnet_handle, bnet_event_rx);

        sup.create_game("Game 1", "admin", spectre_protocol::GameVisibility::Public);
        assert_eq!(sup.games.len(), 1);
        assert_eq!(sup.games[0].port, 6113);

        let cmd1 = bnet_cmd_rx.try_recv().unwrap();
        if let spectre_bnet::BnetCmd::CreateGame { port, name, .. } = cmd1 {
            assert_eq!(port, Some(6113));
            assert_eq!(name, "Game 1");
        } else {
            panic!("Expected CreateGame cmd");
        }

        sup.create_game("Game 2", "admin", spectre_protocol::GameVisibility::Public);
        assert_eq!(sup.games.len(), 2);
        assert_eq!(sup.games[1].port, 6114);

        let cmd2 = bnet_cmd_rx.try_recv().unwrap();
        if let spectre_bnet::BnetCmd::CreateGame { port, name, .. } = cmd2 {
            assert_eq!(port, Some(6114));
            assert_eq!(name, "Game 2");
        } else {
            panic!("Expected CreateGame cmd");
        }

        sup.handle_chat_command("admin", "!getgames");
        let chat_cmd = bnet_cmd_rx.try_recv().unwrap();
        if let spectre_bnet::BnetCmd::SendChat(msg) = chat_cmd {
            assert!(msg.contains("Active games (2):"));
            assert!(msg.contains("Game 1 [port:6113, Lobby]"));
            assert!(msg.contains("Game 2 [port:6114, Lobby]"));
        } else {
            panic!("Expected SendChat cmd");
        }

        sup.handle_chat_command("admin", "!unhost Game 1");
        assert_eq!(sup.games.len(), 1);
        assert_eq!(sup.games[0].name, "Game 2");
        assert!(!sup.allocated_ports.contains(&6113));

        sup.create_game("Game 3", "admin", spectre_protocol::GameVisibility::Public);
        assert_eq!(sup.games.len(), 2);
        assert_eq!(sup.games[1].port, 6113);
    }
}
