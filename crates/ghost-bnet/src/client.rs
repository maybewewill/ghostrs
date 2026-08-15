use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ghost_protocol::bncs::{BncsCodec, ids, incoming, outgoing};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::advert::{MapAdvert, encode_game_statstring};
use crate::auth::{create_key_info, hash_password_double};

#[derive(Debug, Clone)]
pub struct BnetConfig {
    pub server: String,
    pub port: u16,
    pub host_port: u16,
    pub username: String,
    pub password: String,
    pub cdkey_roc: String,
    pub cdkey_tft: String,
    pub first_channel: String,
    pub root_admins: Vec<String>,
    pub command_trigger: char,
    pub war3_version: u8,
    pub exe_version: [u8; 4],
    pub exe_version_hash: [u8; 4],
    pub reconnect_delay: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BnetEvent {
    Connected,
    LoggedIn,
    ChatMessage { user: String, text: String },
    Whisper { user: String, text: String },
    Disconnected(String),
}

#[derive(Debug)]
pub enum BnetCmd {
    CreateGame { name: String, map: MapAdvert, host_counter: u32 },
    RefreshGame { players: u32, slots: u32 },
    UnhostGame,
    SendChat(String),
    Shutdown,
}

#[derive(Debug, Clone)]
pub struct BnetHandle {
    tx: mpsc::Sender<BnetCmd>,
}

impl BnetHandle {
    pub fn new(tx: mpsc::Sender<BnetCmd>) -> Self {
        Self { tx }
    }

    pub fn send(&self, cmd: BnetCmd) {
        let _ = self.tx.try_send(cmd);
    }
}

pub fn spawn_bnet(
    cfg: BnetConfig,
    events: mpsc::Sender<BnetEvent>,
) -> (BnetHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(256);
    let join = tokio::spawn(async move {
        run(cfg, events, rx).await;
    });
    (BnetHandle::new(tx), join)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    AwaitAuthInfo,
    AwaitAuthCheck,
    AwaitLogonResponse,
    AwaitLogonProof,
    InChat,
}

struct ActiveAdvert {
    name: String,
    map: MapAdvert,
    host_counter: u32,
    stat_string: Vec<u8>,
}

async fn run(
    cfg: BnetConfig,
    events: mpsc::Sender<BnetEvent>,
    mut rx: mpsc::Receiver<BnetCmd>,
) {
    'reconnect_loop: loop {
        let addr = format!("{}:{}", cfg.server, cfg.port);
        tracing::info!(%addr, "connecting to battle.net server");

        let mut stream = match TcpStream::connect(&addr).await {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("connect failed: {e}");
                tracing::warn!(%msg);
                let _ = events.send(BnetEvent::Disconnected(msg)).await;
                tokio::select! {
                    _ = tokio::time::sleep(cfg.reconnect_delay) => continue 'reconnect_loop,
                    cmd = rx.recv() => match cmd {
                        Some(BnetCmd::Shutdown) | None => break 'reconnect_loop,
                        _ => continue 'reconnect_loop,
                    }
                }
            }
        };

        // 1. Send protocol selector byte 0x01 for BNCS
        if let Err(e) = stream.write_all(&[0x01]).await {
            let msg = format!("failed to send protocol selector: {e}");
            let _ = events.send(BnetEvent::Disconnected(msg)).await;
            tokio::time::sleep(cfg.reconnect_delay).await;
            continue 'reconnect_loop;
        }

        let (read_half, write_half) = stream.into_split();
        let mut framed_read = FramedRead::new(read_half, BncsCodec::default());
        let mut framed_write = FramedWrite::new(write_half, BncsCodec::default());

        let _ = events.send(BnetEvent::Connected).await;

        // 2. Send SID_AUTH_INFO
        let auth_info_pkt = match outgoing::auth_info(cfg.war3_version, true, 1033, "USA", "United States") {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(error = %e, "failed to build auth_info");
                break 'reconnect_loop;
            }
        };
        tracing::info!("--> [SEND] SID_AUTH_INFO (0x50) [war3_ver={}, platform=IX86, locale=1033]", cfg.war3_version);
        if let Err(e) = framed_write.send(auth_info_pkt).await {
            tracing::warn!(error = %e, "failed to send auth_info");
            continue 'reconnect_loop;
        }

        let mut stage = Stage::AwaitAuthInfo;
        let mut active_advert: Option<ActiveAdvert> = None;
        let mut cur_client_token = 0u32;
        let mut cur_server_token = 0u32;

        let mut null_timer = tokio::time::interval(Duration::from_secs(30));
        let mut adv_timer = tokio::time::interval(Duration::from_secs(3));

        'session: loop {
            tokio::select! {
                cmd = rx.recv() => {
                    match cmd {
                        Some(BnetCmd::Shutdown) | None => break 'reconnect_loop,
                        Some(BnetCmd::SendChat(msg)) => {
                            if stage == Stage::InChat
                                && let Ok(p) = outgoing::chat_command(&msg)
                            {
                                tracing::info!("--> [SEND] SID_CHATCOMMAND (0x0E): \"{}\"", msg);
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::CreateGame { name, map, host_counter }) => {
                            let stat_string = encode_game_statstring(&map, &name, &cfg.username);
                            if stage == Stage::InChat
                                && let Ok(p) = outgoing::startadvex3(
                                    0,
                                    map.game_type.to_le_bytes(),
                                    &name,
                                    &cfg.username,
                                    0,
                                    &stat_string,
                                    host_counter,
                                )
                            {
                                tracing::info!("--> [SEND] SID_STARTADVEX3 (0x1C) [game=\"{}\", host_counter={}]", name, host_counter);
                                let _ = framed_write.send(p).await;
                            }
                            active_advert = Some(ActiveAdvert { name, map, host_counter, stat_string });
                        }
                        Some(BnetCmd::RefreshGame { players: _, slots: _ }) => {
                            if let (Stage::InChat, Some(adv)) = (stage, &active_advert)
                                && let Ok(p) = outgoing::startadvex3(
                                    0,
                                    adv.map.game_type.to_le_bytes(),
                                    &adv.name,
                                    &cfg.username,
                                    0,
                                    &adv.stat_string,
                                    adv.host_counter,
                                )
                            {
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::UnhostGame) => {
                            if stage == Stage::InChat {
                                tracing::info!("--> [SEND] SID_STOPADV (0x02)");
                                let _ = framed_write.send(outgoing::stopadv()).await;
                            }
                            active_advert = None;
                        }
                    }
                }

                _ = null_timer.tick() => {
                    if stage == Stage::InChat {
                        let _ = framed_write.send(outgoing::null()).await;
                    }
                }

                _ = adv_timer.tick() => {
                    if let (Stage::InChat, Some(adv)) = (stage, &active_advert)
                        && let Ok(p) = outgoing::startadvex3(
                            0,
                            adv.map.game_type.to_le_bytes(),
                            &adv.name,
                            &cfg.username,
                            0,
                            &adv.stat_string,
                            adv.host_counter,
                        )
                    {
                        let _ = framed_write.send(p).await;
                    }
                }

                frame = framed_read.next() => {
                    let frame = match frame {
                        Some(Ok(f)) => f,
                        Some(Err(e)) => {
                            let msg = format!("protocol error: {e}");
                            tracing::warn!(%msg);
                            let _ = events.send(BnetEvent::Disconnected(msg)).await;
                            break 'session;
                        }
                        None => {
                            let msg = "connection closed by server".to_string();
                            tracing::warn!(%msg);
                            let _ = events.send(BnetEvent::Disconnected(msg)).await;
                            break 'session;
                        }
                    };

                    match (stage, frame.id) {
                        (Stage::AwaitAuthInfo, ids::SID_AUTH_INFO) => {
                            if let Ok(info) = incoming::AuthInfo::decode(&frame.payload) {
                                cur_server_token = info.server_token;
                                cur_client_token = rand::random();
                                tracing::info!(
                                    "<-- [RECV] SID_AUTH_INFO (0x50) [server_token=0x{:08X}, mpq=\"{}\"]",
                                    info.server_token,
                                    info.ix86_ver_file_name
                                );
                                let key_info_roc = create_key_info(&cfg.cdkey_roc, cur_client_token, cur_server_token, false);
                                let key_info_tft = create_key_info(&cfg.cdkey_tft, cur_client_token, cur_server_token, true);

                                if let Ok(check_pkt) = outgoing::auth_check(
                                    true,
                                    cur_client_token.to_le_bytes(),
                                    cfg.exe_version,
                                    cfg.exe_version_hash,
                                    &key_info_roc,
                                    &key_info_tft,
                                    &info.ix86_ver_file_name,
                                    "GHost",
                                ) {
                                    tracing::info!(
                                        "--> [SEND] SID_AUTH_CHECK (0x51) [client_token=0x{:08X}, ver=1.26a]",
                                        cur_client_token
                                    );
                                    let _ = framed_write.send(check_pkt).await;
                                    stage = Stage::AwaitAuthCheck;
                                }
                            }
                        }

                        (Stage::AwaitAuthCheck, ids::SID_AUTH_CHECK) => {
                            if let Ok(check) = incoming::AuthCheck::decode(&frame.payload) {
                                tracing::info!(
                                    "<-- [RECV] SID_AUTH_CHECK (0x51) [key_state={}, description=\"{}\"]",
                                    check.key_state,
                                    check.key_state_description
                                );
                                if check.key_state == 0 {
                                    let password_hash = hash_password_double(&cfg.password, cur_client_token, cur_server_token);
                                    if let Ok(logon_pkt) = outgoing::logon_response(
                                        cur_client_token.to_le_bytes(),
                                        cur_server_token.to_le_bytes(),
                                        &password_hash,
                                        &cfg.username,
                                    ) {
                                        tracing::info!(
                                            "--> [SEND] SID_LOGONRESPONSE (0x29) [account=\"{}\"]",
                                            cfg.username
                                        );
                                        let _ = framed_write.send(logon_pkt).await;
                                        stage = Stage::AwaitLogonProof;
                                    }
                                } else {
                                    let msg = format!("CD keys rejected: {}", check.key_state_description);
                                    tracing::error!(%msg);
                                    let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                    break 'session;
                                }
                            }
                        }

                        (Stage::AwaitLogonProof, ids::SID_LOGONRESPONSE2) | (Stage::AwaitLogonProof, ids::SID_LOGONRESPONSE) | (Stage::AwaitLogonProof, ids::SID_AUTH_ACCOUNTLOGONPROOF) => {
                            if let Ok(proof) = incoming::LogonProof::decode(&frame.payload) {
                                tracing::info!(
                                    "<-- [RECV] SID_LOGONRESPONSE2 (0x3A) [status={} (SUCCESS)]",
                                    proof.status
                                );
                                if proof.status == 0 || proof.status == 0x0E {
                                    // Enter chat
                                    if let Ok(enter_pkt) = outgoing::enter_chat() {
                                        tracing::info!("--> [SEND] SID_ENTERCHAT (0x0A)");
                                        let _ = framed_write.send(enter_pkt).await;
                                        stage = Stage::InChat;
                                        tracing::info!("--> [SEND] SID_NETGAMEPORT (0x45) [port={}]", cfg.host_port);
                                        let _ = framed_write.send(outgoing::netgameport(cfg.host_port)).await;
                                        if !cfg.first_channel.is_empty()
                                            && let Ok(join_pkt) = outgoing::join_channel(&cfg.first_channel)
                                        {
                                            tracing::info!("--> [SEND] SID_JOINCHANNEL (0x0C) [channel=\"{}\"]", cfg.first_channel);
                                            let _ = framed_write.send(join_pkt).await;
                                        }
                                        let _ = events.send(BnetEvent::LoggedIn).await;
                                        tracing::info!(user = %cfg.username, "successfully logged into battle.net as userbot");
                                    }
                                } else {
                                    let msg = format!("Password rejected: status {}", proof.status);
                                    tracing::error!(%msg);
                                    let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                    break 'session;
                                }
                            }
                        }

                        (Stage::InChat, ids::SID_CHATEVENT) => {
                            if let Ok(ev) = incoming::ChatEvent::decode(&frame.payload) {
                                match ev.event_id {
                                    0x04 => {
                                        let _ = events.send(BnetEvent::Whisper { user: ev.user, text: ev.message }).await;
                                    }
                                    0x05 => {
                                        let _ = events.send(BnetEvent::ChatMessage { user: ev.user, text: ev.message }).await;
                                    }
                                    _ => {}
                                }
                            }
                        }

                        (Stage::InChat, ids::SID_PING) => {
                            if let Ok(ping_val) = incoming::decode_ping(&frame.payload) {
                                let _ = framed_write.send(outgoing::ping(ping_val)).await;
                            }
                        }

                        _ => {}
                    }
                }
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(cfg.reconnect_delay) => continue 'reconnect_loop,
            cmd = rx.recv() => match cmd {
                Some(BnetCmd::Shutdown) | None => break 'reconnect_loop,
                _ => continue 'reconnect_loop,
            }
        }
    }
}
