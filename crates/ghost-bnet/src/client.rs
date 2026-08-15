use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ghost_protocol::bncs::{BncsCodec, ids, incoming, outgoing};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::advert::{MapAdvert, encode_game_statstring};
use crate::auth::create_key_info;
use crate::bncsutil::NlsSession;

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
    pub password_hash_type: String,
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
    AwaitAccountLogon,
    AwaitAccountLogonProof,
    AwaitEnterChat,
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
        let mut cur_nls: Option<NlsSession> = None;
        // The advert is re-sent every 3 s, so only log the transition, not every ack.
        let mut advert_listed = false;

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

                    tracing::info!(
                        "<-- [RECV] BNCS packet id=0x{:02X} len={}",
                        frame.id,
                        frame.payload.len()
                    );

                    if frame.id == ids::SID_PING {
                        if let Ok(ping_val) = incoming::decode_ping(&frame.payload) {
                            let _ = framed_write.send(outgoing::ping(ping_val)).await;
                        }
                        continue;
                    }

                    match (stage, frame.id) {
                        (Stage::AwaitAuthInfo, ids::SID_AUTH_INFO) => {
                            if let Ok(info) = incoming::AuthInfo::decode(&frame.payload) {
                                let cur_server_token = info.server_token;
                                let cur_client_token: u32 = rand::random();
                                tracing::info!(
                                    "<-- [RECV] SID_AUTH_INFO (0x50) [server_token=0x{:08X}, mpq=\"{}\"]",
                                    info.server_token,
                                    info.ix86_ver_file_name
                                );
                                let key_info_roc = match create_key_info(&cfg.cdkey_roc, cur_client_token, cur_server_token, false) {
                                    Ok(k) => k,
                                    Err(e) => {
                                        let msg = format!("invalid ROC CD-key: {e}");
                                        tracing::error!(%msg);
                                        let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                        break 'session;
                                    }
                                };
                                let key_info_tft = match create_key_info(&cfg.cdkey_tft, cur_client_token, cur_server_token, true) {
                                    Ok(k) => k,
                                    Err(e) => {
                                        let msg = format!("invalid TFT CD-key: {e}");
                                        tracing::error!(%msg);
                                        let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                        break 'session;
                                    }
                                };

                                let war3_files = if std::path::Path::new("war3/warcraft.exe").exists() {
                                    Some((
                                        std::path::PathBuf::from("war3/warcraft.exe"),
                                        std::path::PathBuf::from("war3/Storm.dll"),
                                        std::path::PathBuf::from("war3/game.dll"),
                                    ))
                                } else if std::path::Path::new("war3/Warcraft III.exe").exists() {
                                    Some((
                                        std::path::PathBuf::from("war3/Warcraft III.exe"),
                                        std::path::PathBuf::from("war3/Storm.dll"),
                                        std::path::PathBuf::from("war3/game.dll"),
                                    ))
                                } else {
                                    None
                                };

                                let (exe_info, exe_version, exe_version_hash) = if let Some((war3_exe, storm_dll, game_dll)) = war3_files {
                                    let formula = info.value_string_formula.clone();
                                    let mpq_name = info.ix86_ver_file_name.clone();
                                    let res = tokio::task::spawn_blocking(move || -> Result<(String, [u8; 4], [u8; 4]), std::io::Error> {
                                        let mpq_num = crate::bncsutil::extract_mpq_number(&mpq_name);
                                        let exe_info = crate::bncsutil::get_exe_info(&war3_exe, 1)?;
                                        let hash = crate::bncsutil::check_revision_flat(&formula, &war3_exe, &storm_dll, &game_dll, mpq_num)?;
                                        Ok((exe_info.exe_info_string, exe_info.version.to_le_bytes(), hash.to_le_bytes()))
                                    }).await;

                                    match res {
                                        Ok(Ok(vals)) => vals,
                                        Ok(Err(e)) => {
                                            let msg = format!("CheckRevision / ExeInfo failed: {e}");
                                            tracing::error!(%msg);
                                            let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                            break 'session;
                                        }
                                        Err(e) => {
                                            let msg = format!("spawn_blocking failed: {e}");
                                            tracing::error!(%msg);
                                            let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                            break 'session;
                                        }
                                    }
                                } else {
                                    (info.ix86_ver_file_name.clone(), cfg.exe_version, cfg.exe_version_hash)
                                };

                                if let Ok(check_pkt) = outgoing::auth_check(
                                    true,
                                    cur_client_token.to_le_bytes(),
                                    exe_version,
                                    exe_version_hash,
                                    &key_info_roc,
                                    &key_info_tft,
                                    &exe_info,
                                    "GHost",
                                ) {
                                    tracing::info!(
                                        "--> [SEND] SID_AUTH_CHECK (0x51) [client_token=0x{:08X}, exe_info=\"{}\"]",
                                        cur_client_token,
                                        exe_info
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
                                    tracing::info!("cd keys accepted");
                                    // Both logon types go through SID_AUTH_ACCOUNTLOGON here, exactly
                                    // as `bnet.cpp:845-846` does. They diverge only at the proof
                                    // (`bnet.cpp:883-897`): pvpgn proves with the password hash,
                                    // battle.net with the SRP M1. Sending the old SID_LOGONRESPONSE
                                    // (0x29) instead makes PvPGN answer status=0 and then ignore
                                    // SID_ENTERCHAT forever, because the connection never leaves the
                                    // new-auth state machine it entered at SID_AUTH_INFO.
                                    let nls = NlsSession::new(&cfg.username, &cfg.password);
                                    let client_key = nls.client_public_key();
                                    cur_nls = Some(nls);

                                    if let Ok(logon_pkt) = outgoing::auth_accountlogon(
                                        &client_key,
                                        &cfg.username,
                                    ) {
                                        tracing::info!(
                                            "--> [SEND] SID_AUTH_ACCOUNTLOGON (0x53) [account=\"{}\", logon_type={}]",
                                            cfg.username,
                                            cfg.password_hash_type
                                        );
                                        let _ = framed_write.send(logon_pkt).await;
                                        stage = Stage::AwaitAccountLogon;
                                    }
                                } else {
                                    let msg = format!("CD keys rejected: {}", check.key_state_description);
                                    tracing::error!(%msg);
                                    let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                    break 'session;
                                }
                            }
                        }

                        (Stage::AwaitAccountLogon, ids::SID_AUTH_ACCOUNTLOGON) => {
                            if let Ok(acc) = incoming::AccountLogon::decode(&frame.payload) {
                                tracing::info!(
                                    "<-- [RECV] SID_AUTH_ACCOUNTLOGON (0x53) [status={}]",
                                    acc.status
                                );
                                if acc.status == 0 {
                                    tracing::info!("username {} accepted", cfg.username);
                                    // `bnet.cpp:883-897`: pvpgn proves with the raw password hash,
                                    // battle.net with the SRP-6a M1 derived from the server's salt
                                    // and public key.
                                    let proof_bytes: Vec<u8> = if cfg.password_hash_type.eq_ignore_ascii_case("pvpgn") {
                                        crate::auth::hash_password_pvpgn(&cfg.password).to_vec()
                                    } else if let Some(ref nls) = cur_nls {
                                        match nls.compute_m1(&acc.server_public_key, &acc.salt) {
                                            Ok(m1) => m1.to_vec(),
                                            Err(e) => {
                                                let msg = format!("NLS compute_m1 failed: {e}");
                                                tracing::error!(%msg);
                                                let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                                break 'session;
                                            }
                                        }
                                    } else {
                                        Vec::new()
                                    };

                                    if let Ok(proof_pkt) = outgoing::auth_accountlogonproof(&proof_bytes) {
                                        tracing::info!("--> [SEND] SID_AUTH_ACCOUNTLOGONPROOF (0x54)");
                                        let _ = framed_write.send(proof_pkt).await;
                                        stage = Stage::AwaitAccountLogonProof;
                                    }
                                } else {
                                    let msg = format!("Logon failed - invalid username (status {})", acc.status);
                                    tracing::error!(%msg);
                                    let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                    break 'session;
                                }
                            }
                        }

                        (Stage::AwaitAccountLogonProof, ids::SID_AUTH_ACCOUNTLOGONPROOF) | (Stage::AwaitAccountLogonProof, ids::SID_LOGONRESPONSE2) | (Stage::AwaitAccountLogonProof, ids::SID_LOGONRESPONSE) => {
                            if let Ok(proof) = incoming::LogonProof::decode(&frame.payload) {
                                tracing::info!(
                                    "<-- [RECV] SID_AUTH_ACCOUNTLOGONPROOF (0x54) [status={}]",
                                    proof.status
                                );
                                if proof.status == 0 || proof.status == 0x0E {
                                    tracing::info!("logon successful");
                                    if !proof.message.is_empty() {
                                        tracing::info!("[BNET SERVER INFO] {}", proof.message);
                                    }
                                    stage = Stage::AwaitEnterChat;
                                    tracing::info!("--> [SEND] SID_NETGAMEPORT (0x45) [port={}]", cfg.host_port);
                                    let _ = framed_write.send(outgoing::netgameport(cfg.host_port)).await;
                                    if let Ok(enter_pkt) = outgoing::enter_chat() {
                                        tracing::info!("--> [SEND] SID_ENTERCHAT (0x0A)");
                                        let _ = framed_write.send(enter_pkt).await;
                                    }
                                    if let Ok(f_pkt) = outgoing::friendslist() {
                                        let _ = framed_write.send(f_pkt).await;
                                    }
                                    if let Ok(c_pkt) = outgoing::clanmemberlist() {
                                        let _ = framed_write.send(c_pkt).await;
                                    }
                                } else {
                                    let msg = format!("Logon failed - invalid password (status {})", proof.status);
                                    tracing::error!(%msg);
                                    let _ = events.send(BnetEvent::Disconnected(msg)).await;
                                    break 'session;
                                }
                            }
                        }

                        (Stage::AwaitEnterChat, ids::SID_ENTERCHAT) | (Stage::InChat, ids::SID_ENTERCHAT) => {
                            let text = String::from_utf8_lossy(&frame.payload);
                            tracing::info!(raw = %text, "<-- [RECV] SID_ENTERCHAT (0x0A)");
                            if !cfg.first_channel.is_empty()
                                && let Ok(join_pkt) = outgoing::join_channel(&cfg.first_channel)
                            {
                                tracing::info!("--> [SEND] SID_JOINCHANNEL (0x0C) [channel=\"{}\"]", cfg.first_channel);
                                let _ = framed_write.send(join_pkt).await;
                            }
                            stage = Stage::InChat;
                            let _ = events.send(BnetEvent::LoggedIn).await;
                            tracing::info!(user = %cfg.username, "successfully logged into battle.net as userbot");
                        }

                        (Stage::InChat, ids::SID_CHATEVENT) => {
                            match incoming::ChatEvent::decode(&frame.payload) {
                                Ok(ev) => {
                                    match ev.event_id {
                                        0x01 => {
                                            tracing::info!("[BNET] channel user: {} (ping: {}ms)", ev.user, ev.ping);
                                        }
                                        0x02 => {
                                            tracing::info!("[BNET] joined channel: {}", ev.user);
                                        }
                                        0x03 => {
                                            tracing::info!("[BNET] left channel: {}", ev.user);
                                        }
                                        0x04 => {
                                            tracing::info!("[BNET WHISPER] <{}> {}", ev.user, ev.message);
                                            let _ = events.send(BnetEvent::Whisper { user: ev.user, text: ev.message }).await;
                                        }
                                        0x05 => {
                                            tracing::info!("[BNET CHAT] <{}> {}", ev.user, ev.message);
                                            let _ = events.send(BnetEvent::ChatMessage { user: ev.user, text: ev.message }).await;
                                        }
                                        0x06 => {
                                            tracing::info!("[BNET BROADCAST] {}", ev.message);
                                        }
                                        0x07 => {
                                            tracing::info!("[BNET CHANNEL] joined channel \"{}\"", ev.message);
                                        }
                                        0x0A => {
                                            tracing::info!("[BNET WHISPER SENT] -> <{}> {}", ev.user, ev.message);
                                        }
                                        0x12 => {
                                            tracing::info!("[BNET SERVER INFO] {}", ev.message);
                                        }
                                        0x13 => {
                                            tracing::warn!("[BNET SERVER ERROR] {}", ev.message);
                                        }
                                        0x17 => {
                                            tracing::info!("[BNET EMOTE] <{}> {}", ev.user, ev.message);
                                        }
                                        other => {
                                            tracing::info!(event_id = other, user = %ev.user, msg = %ev.message, "[BNET EVENT]");
                                        }
                                    }
                                }
                                Err(e) => {
                                    let raw_text = String::from_utf8_lossy(&frame.payload);
                                    tracing::warn!(error = %e, raw = %raw_text, "failed to decode SID_CHATEVENT payload");
                                }
                            }
                        }

                        (Stage::InChat, ids::SID_STARTADVEX3) => {
                            // `bnetprotocol.cpp:174-191`: a u32 status of 0 means the
                            // game is listed. Anything else means it is not — most often
                            // a duplicate game name already on the server.
                            let status = if frame.payload.len() >= 4 {
                                u32::from_le_bytes([
                                    frame.payload[0],
                                    frame.payload[1],
                                    frame.payload[2],
                                    frame.payload[3],
                                ])
                            } else {
                                u32::MAX
                            };
                            if status == 0 {
                                if !advert_listed {
                                    advert_listed = true;
                                    let name = active_advert.as_ref().map(|a| a.name.clone()).unwrap_or_default();
                                    tracing::info!(game = %name, "<-- [RECV] SID_STARTADVEX3 (0x1C) [status=0] game is listed on battle.net");
                                }
                            } else {
                                advert_listed = false;
                                tracing::warn!(status, "startadvex3 failed — the game is NOT listed (duplicate game name?)");
                            }
                        }

                        (Stage::InChat, ids::SID_PING) => {
                            if let Ok(ping_val) = incoming::decode_ping(&frame.payload) {
                                let _ = framed_write.send(outgoing::ping(ping_val)).await;
                            }
                        }

                        (st, pkt_id) => {
                            let text = String::from_utf8_lossy(&frame.payload);
                            tracing::info!(stage = ?st, pkt_id = format!("0x{:02X}", pkt_id), len = frame.payload.len(), raw = %text, "unhandled BNCS packet");
                        }
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
