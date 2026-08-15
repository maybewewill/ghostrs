use std::collections::HashMap;
use std::net::SocketAddr;

use anyhow::Context;
use ghost_bnet::{BnetCmd, BnetEvent, BnetHandle, MapAdvert, spawn_bnet};
use ghost_engine::{GameCmd, GameConfig, GameHandle, MapInfo, spawn_game};
use ghost_net::{ConnEvent, spawn_conn, spawn_listener};
use ghost_store::Store;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config::Config;

pub struct Supervisor {
    cfg: Config,
    _store: Store,
    bnet: BnetHandle,
    bnet_events: mpsc::Receiver<BnetEvent>,
    current_game: Option<GameHandle>,
    _current_game_name: Option<String>,
    running_games: Vec<(String, GameHandle, JoinHandle<()>)>,
    conn_to_game: HashMap<u64, GameHandle>,
    listener_rx: mpsc::Receiver<(u64, TcpStream, SocketAddr)>,
    conn_event_tx: mpsc::Sender<ConnEvent>,
    conn_event_rx: mpsc::Receiver<ConnEvent>,
}

impl Supervisor {
    pub async fn run(cfg: Config) -> anyhow::Result<()> {
        let (store, _store_task) = Store::open(&cfg.db_path)
            .context("failed to open SQLite database")?;

        let (bnet_events_tx, bnet_events_rx) = mpsc::channel(256);
        let (bnet, _bnet_task) = spawn_bnet(cfg.bnet.clone(), bnet_events_tx);

        let (listener_tx, listener_rx) = mpsc::channel(256);
        let bind_addr: SocketAddr = format!("{}:{}", cfg.bot.bind_address, cfg.bot.host_port)
            .parse()
            .context("invalid bot bind address/port")?;
        let _listener_task = spawn_listener(bind_addr, listener_tx);

        let (conn_event_tx, conn_event_rx) = mpsc::channel(1024);

        let mut sup = Self {
            cfg,
            _store: store,
            bnet,
            bnet_events: bnet_events_rx,
            current_game: None,
            _current_game_name: None,
            running_games: Vec::new(),
            conn_to_game: HashMap::new(),
            listener_rx,
            conn_event_tx,
            conn_event_rx,
        };

        sup.event_loop().await
    }

    async fn event_loop(&mut self) -> anyhow::Result<()> {
        tracing::info!("supervisor ready, awaiting battle.net and player events");

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    tracing::info!("SIGINT received, shutting down gracefully");
                    self.shutdown().await;
                    break;
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

            self.clean_finished_games();
        }

        Ok(())
    }

    fn handle_new_connection(&mut self, conn_id: u64, stream: TcpStream, peer: SocketAddr) {
        if let Some(game) = &self.current_game {
            let link = spawn_conn(conn_id, stream, self.conn_event_tx.clone(), 1024);
            let external_ip = match peer.ip() {
                std::net::IpAddr::V4(v4) => v4.octets(),
                _ => [127, 0, 0, 1],
            };
            game.send(GameCmd::NewConn { conn_id, link, external_ip });
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
            BnetEvent::LoggedIn => tracing::info!("logged in to Battle.net"),
            BnetEvent::ChatMessage { user, text } => self.handle_chat_command(&user, &text),
            BnetEvent::Whisper { user, text } => self.handle_chat_command(&user, &text),
            BnetEvent::Disconnected(reason) => tracing::warn!(%reason, "disconnected from Battle.net"),
        }
    }

    fn handle_chat_command(&mut self, user: &str, text: &str) {
        let is_root_admin = self.cfg.bnet.root_admins.iter().any(|a| a.eq_ignore_ascii_case(user));
        if !is_root_admin {
            return;
        }

        let trigger = self.cfg.bnet.command_trigger;
        let Some(cmd_text) = text.strip_prefix(trigger) else { return };
        let mut parts = cmd_text.split_whitespace();
        let Some(verb) = parts.next() else { return };

        match verb.to_lowercase().as_str() {
            "pub" | "priv" => {
                let name = parts.collect::<Vec<_>>().join(" ");
                if name.is_empty() {
                    self.bnet.send(BnetCmd::SendChat(format!("/w {user} Usage: !pub <game name>")));
                    return;
                }
                if self.running_games.len() >= self.cfg.bot.max_games {
                    self.bnet.send(BnetCmd::SendChat(format!("/w {user} Error: maximum games reached")));
                    return;
                }
                self.create_game(&name, user);
            }
            "unhost" => {
                if let Some(g) = self.current_game.take() {
                    g.send(GameCmd::Unhost);
                    self._current_game_name = None;
                    self.bnet.send(BnetCmd::UnhostGame);
                    self.bnet.send(BnetCmd::SendChat(format!("/w {user} Game unhosted")));
                }
            }
            "start" => {
                if let Some(g) = &self.current_game {
                    g.send(GameCmd::Start { by: user.to_string() });
                }
            }
            "say" => {
                let msg = parts.collect::<Vec<_>>().join(" ");
                self.bnet.send(BnetCmd::SendChat(msg));
            }
            _ => {}
        }
    }

    fn create_game(&mut self, name: &str, owner: &str) {
        let map_info = MapInfo {
            path: format!("Maps\\Download\\{name}.w3x"),
            size: 1000,
            info: 1,
            crc: 0x1234_5678,
            sha1: [0; 20],
            num_players: 12,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0,
            data: None,
        };

        let game_cfg = GameConfig {
            name: name.to_string(),
            owner: owner.to_string(),
            host_counter: rand::random(),
            num_slots: 12,
            latency: self.cfg.game.latency,
            sync_limit: self.cfg.game.sync_limit,
            map: map_info,
            virtual_host_name: self.cfg.game.virtual_host_name.clone(),
            reconnect_wait: self.cfg.game.reconnect_wait,
        };

        let host_counter = game_cfg.host_counter;
        let (handle, join) = spawn_game(game_cfg);

        self.current_game = Some(handle.clone());
        self._current_game_name = Some(name.to_string());
        self.running_games.push((name.to_string(), handle, join));

        let advert = MapAdvert {
            path: format!("Maps\\Download\\{name}.w3x"),
            size: 1000,
            info: 1,
            crc: 0x1234_5678,
            sha1: [0; 20],
            num_players: 12,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0,
        };

        self.bnet.send(BnetCmd::CreateGame {
            name: name.to_string(),
            map: advert,
            host_counter,
        });

        tracing::info!(game = %name, %owner, "game created and advertised on Battle.net");
    }

    fn clean_finished_games(&mut self) {
        self.running_games.retain(|(_, h, _)| !h.is_closed());
        self.conn_to_game.retain(|_, h| !h.is_closed());
        if let Some(h) = &self.current_game
            && h.is_closed()
        {
            self.current_game = None;
            self._current_game_name = None;
            self.bnet.send(BnetCmd::UnhostGame);
        }
    }

    async fn shutdown(&mut self) {
        self.bnet.send(BnetCmd::Shutdown);
        for (_, h, _) in &self.running_games {
            h.send(GameCmd::Shutdown);
        }
    }
}
