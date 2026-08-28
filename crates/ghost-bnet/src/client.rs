use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use ghost_protocol::bncs::{BncsCodec, ids, incoming, outgoing};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::codec::{FramedRead, FramedWrite};

use crate::advert::{MapAdvert, encode_bnet_statstring};
use crate::auth::create_key_info;
use crate::bncsutil::NlsSession;

#[derive(Debug, Clone)]
pub struct BnetConfig {
    pub server: String,
    pub server_alias: String,
    pub pvpgn_realm_name: String,
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
    FriendsList(Vec<ghost_protocol::bncs::incoming::FriendListEntry>),
    ClanList(Vec<ghost_protocol::bncs::incoming::ClanMemberEntry>),
    ClanInviteReceived {
        clan_name: String,
        inviter: String,
        creation: bool,
    },
    ClanRankChanged {
        status: u8,
    },
    ClanMemberRemoved {
        status: u8,
    },
    ClanMotdSet {
        status: u8,
    },
    Disconnected(String),
}

#[derive(Debug)]
pub enum BnetCmd {
    CreateGame {
        name: String,
        map: MapAdvert,
        host_counter: u32,
        visibility: ghost_protocol::GameVisibility,
        host_name: Option<String>,
        port: Option<u16>,
    },
    RefreshGame {
        players: u32,
        slots: u32,
    },
    UnhostGame,
    SendChat(String),
    GetFriendsList,
    GetClanList,
    ClanInvitation(String),
    ClanRemoveMember(String),
    ClanChangeRank { account: String, rank: u8 },
    ClanSetMotd(String),
    ClanAcceptInvite(bool),
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
    visibility: ghost_protocol::GameVisibility,
    host_name: String,
    port: Option<u16>,
}

/// `bnet.cpp:2284` ORs MAPGAMETYPE_PRIVATEGAME into the game type for a private
/// game, so it stays out of the public game list.
const MAPGAMETYPE_PRIVATEGAME: u32 = 0x0000_0800;

fn advert_game_type(map: &MapAdvert, visibility: ghost_protocol::GameVisibility) -> [u8; 4] {
    let mut t = map.game_type;
    if visibility == ghost_protocol::GameVisibility::Private {
        t |= MAPGAMETYPE_PRIVATEGAME;
    }
    t.to_le_bytes()
}

async fn run(cfg: BnetConfig, events: mpsc::Sender<BnetEvent>, mut rx: mpsc::Receiver<BnetCmd>) {
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

        let auth_info_pkt =
            match outgoing::auth_info(cfg.war3_version, true, 1033, "USA", "United States") {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!(error = %e, "failed to build auth_info");
                    break 'reconnect_loop;
                }
            };
        tracing::info!(
            "--> [SEND] SID_AUTH_INFO (0x50) [war3_ver={}, platform=IX86, locale=1033]",
            cfg.war3_version
        );
        if let Err(e) = framed_write.send(auth_info_pkt).await {
            tracing::warn!(error = %e, "failed to send auth_info");
            continue 'reconnect_loop;
        }

        let mut stage = Stage::AwaitAuthInfo;
        let mut active_adverts: Vec<ActiveAdvert> = Vec::new();
        let mut cur_nls: Option<NlsSession> = None;
        // The advert is re-sent every 3 s, so only log the transition, not every ack.
        let mut advert_listed = false;

        let mut _friends: Vec<incoming::FriendListEntry> = Vec::new();
        let mut _clan_members: Vec<incoming::ClanMemberEntry> = Vec::new();
        let mut last_clan_invite_tag = [0u8; 4];
        let mut last_clan_invite_name = String::new();
        let mut last_invite_creation = false;

        let mut null_timer = tokio::time::interval(Duration::from_secs(30));
        let mut adv_timer = tokio::time::interval(Duration::from_secs(3));
        let mut probe_timer = tokio::time::interval(Duration::from_secs(10));

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
                        Some(BnetCmd::GetFriendsList) => {
                            if let Ok(p) = outgoing::friendslist() {
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::GetClanList) => {
                            if let Ok(p) = outgoing::clanmemberlist() {
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::ClanInvitation(name)) => {
                            if let Ok(p) = outgoing::claninvitation(&name) {
                                let _ = framed_write.send(p).await;
                            }
                            if let Ok(p) = outgoing::clanmemberlist() {
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::ClanRemoveMember(name)) => {
                            if let Ok(p) = outgoing::clanremovemember(&name) {
                                let _ = framed_write.send(p).await;
                            }
                            if let Ok(p) = outgoing::clanmemberlist() {
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::ClanChangeRank { account, rank }) => {
                            if let Ok(p) = outgoing::clanchangerank(&account, rank) {
                                let _ = framed_write.send(p).await;
                            }
                            if let Ok(p) = outgoing::clanmemberlist() {
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::ClanSetMotd(motd)) => {
                            if let Ok(p) = outgoing::clansetmotd(&motd) {
                                let _ = framed_write.send(p).await;
                            }
                        }
                        Some(BnetCmd::ClanAcceptInvite(accept)) => {
                            if last_invite_creation {
                                if let Ok(p) = outgoing::clancreationinvitation(&last_clan_invite_tag, &last_clan_invite_name, accept) {
                                    let _ = framed_write.send(p).await;
                                }
                            } else {
                                if let Ok(p) = outgoing::claninvitationresponse(&last_clan_invite_tag, &last_clan_invite_name, accept) {
                                    let _ = framed_write.send(p).await;
                                }
                            }
                        }
                        Some(BnetCmd::CreateGame { name, map, host_counter, visibility, host_name, port }) => {
                            let host_user = host_name.unwrap_or_else(|| cfg.username.clone());
                            let stat_string = encode_bnet_statstring(&map, &name, &host_user);
                            if stage == Stage::InChat {
                                if let Some(p) = port {
                                    let _ = framed_write.send(outgoing::netgameport(p)).await;
                                }
                                match outgoing::startadvex3(
                                    visibility,
                                    advert_game_type(&map, visibility),
                                    &name,
                                    &host_user,
                                    0,
                                    &stat_string,
                                    host_counter,
                                ) {
                                    Ok(p) => {
                                        tracing::info!("--> [SEND] SID_STARTADVEX3 (0x1C) [game=\"{}\", host_counter={}, visibility={:?}, port={:?}]", name, host_counter, visibility, port);
                                        let _ = framed_write.send(p).await;
                                    }
                                    Err(e) => {
                                        tracing::error!(error = %e, game = %name, "failed to build startadvex3 packet for Battle.net advert");
                                    }
                                }
                            }
                            active_adverts.retain(|a| a.name != name);
                            active_adverts.push(ActiveAdvert { name, map, host_counter, stat_string, visibility, host_name: host_user, port });
                        }
                        Some(BnetCmd::RefreshGame { players: _, slots: _ }) => {
                            if stage == Stage::InChat {
                                for adv in &active_adverts {
                                    if let Some(p) = adv.port {
                                        let _ = framed_write.send(outgoing::netgameport(p)).await;
                                    }
                                    if let Ok(p) = outgoing::startadvex3(
                                        adv.visibility,
                                        advert_game_type(&adv.map, adv.visibility),
                                        &adv.name,
                                        &adv.host_name,
                                        0,
                                        &adv.stat_string,
                                        adv.host_counter,
                                    ) {
                                        let _ = framed_write.send(p).await;
                                    }
                                }
                            }
                        }
                        Some(BnetCmd::UnhostGame) => {
                            if stage == Stage::InChat {
                                tracing::info!("--> [SEND] SID_STOPADV (0x02)");
                                let _ = framed_write.send(outgoing::stopadv()).await;
                            }
                            active_adverts.clear();
                        }
                    }
                }

                _ = null_timer.tick() => {
                    if stage == Stage::InChat {
                        let _ = framed_write.send(outgoing::null()).await;
                    }
                }

                _ = adv_timer.tick() => {
                    if stage == Stage::InChat {
                        for adv in &active_adverts {
                            if let Some(p) = adv.port {
                                let _ = framed_write.send(outgoing::netgameport(p)).await;
                            }
                            if let Ok(p) = outgoing::startadvex3(
                                adv.visibility,
                                advert_game_type(&adv.map, adv.visibility),
                                &adv.name,
                                &adv.host_name,
                                0,
                                &adv.stat_string,
                                adv.host_counter,
                            ) {
                                let _ = framed_write.send(p).await;
                            }
                        }
                    }
                }

                _ = probe_timer.tick() => {
                    if stage == Stage::InChat {
                        for adv in &active_adverts {
                            if let Ok(p) = outgoing::getadvlistex(&adv.name) {
                                let _ = framed_write.send(p).await;
                            }
                        }
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

                    if frame.id == ids::SID_ICCUP_CHALLENGE {
                        tracing::info!(
                            len = frame.payload.len(),
                            "<-- [RECV] SID_ICCUP_CHALLENGE (0xF9) [len={}], sending iccup_challenge_reply (0xF7)",
                            frame.payload.len()
                        );
                        if let Ok(reply_pkt) = outgoing::iccup_challenge_reply(&frame.payload) {
                            tracing::info!("--> [SEND] SID_ICCUP_ANTIHACK (0xF7) [len={}]", reply_pkt.len());
                            let _ = framed_write.send(reply_pkt).await;
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

                        (Stage::AwaitAuthInfo, ids::SID_ICCUP_CHALLENGE) => {
                            tracing::info!(
                                len = frame.payload.len(),
                                "<-- [RECV] SID_ICCUP_CHALLENGE (0xF9) - replying with Anti-Hack proof"
                            );
                            if let Ok(reply_pkt) = outgoing::iccup_challenge_reply(&frame.payload) {
                                tracing::info!("--> [SEND] SID_ICCUP_ANTIHACK (0xF7)");
                                let _ = framed_write.send(reply_pkt).await;
                            }
                        }

                        (_, ids::SID_ICCUP_ANTIHACK) => {
                            tracing::info!(
                                len = frame.payload.len(),
                                "<-- [RECV] SID_ICCUP_ANTIHACK (0xF7)"
                            );
                            if let Ok(reply_pkt) = outgoing::iccup_challenge_reply(&frame.payload) {
                                let _ = framed_write.send(reply_pkt).await;
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
                            for adv in &active_adverts {
                                if let Some(p) = adv.port {
                                    let _ = framed_write.send(outgoing::netgameport(p)).await;
                                }
                                if let Ok(p) = outgoing::startadvex3(
                                    adv.visibility,
                                    advert_game_type(&adv.map, adv.visibility),
                                    &adv.name,
                                    &adv.host_name,
                                    0,
                                    &adv.stat_string,
                                    adv.host_counter,
                                ) {
                                    tracing::info!("--> [SEND] SID_STARTADVEX3 (0x1C) [game=\"{}\", host_counter={}, visibility={:?}, port={:?}]", adv.name, adv.host_counter, adv.visibility, adv.port);
                                    let _ = framed_write.send(p).await;
                                }
                            }
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
                                    let names: Vec<String> = active_adverts.iter().map(|a| a.name.clone()).collect();
                                    tracing::info!(games = ?names, "<-- [RECV] SID_STARTADVEX3 (0x1C) [status=0] game(s) listed on battle.net");
                                }
                            } else {
                                advert_listed = false;
                                tracing::warn!(status, "startadvex3 failed — the game is NOT listed (duplicate game name?)");
                            }
                        }

                        (Stage::InChat, ids::SID_GETADVLISTEX) => {
                            match incoming::decode_getadvlistex(&frame.payload) {
                                Ok(Some(entry)) => {
                                    let ip_str = format!("{}.{}.{}.{}", entry.ip[0], entry.ip[1], entry.ip[2], entry.ip[3]);
                                    let hc_str = match entry.host_counter {
                                        Some(hc) => format!("{:#010x}", hc),
                                        None => "unparseable".to_string(),
                                    };
                                    tracing::info!(
                                        game = %entry.game_name,
                                        ip = %ip_str,
                                        port = entry.port,
                                        host_counter = %hc_str,
                                        "<-- [RECV] SID_GETADVLISTEX (0x09) — server WILL hand joiners this address"
                                    );
                                }
                                Ok(None) => {
                                    let names: Vec<String> = active_adverts.iter().map(|a| a.name.clone()).collect();
                                    tracing::warn!(
                                        games = ?names,
                                        "<-- [RECV] SID_GETADVLISTEX (0x09) [games_found=0] — the server does NOT have our game; joins will fail"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        len = frame.payload.len(),
                                        "failed to decode SID_GETADVLISTEX payload"
                                    );
                                }
                            }
                        }

                        (Stage::InChat, ids::SID_PING) => {
                            if let Ok(ping_val) = incoming::decode_ping(&frame.payload) {
                                let _ = framed_write.send(outgoing::ping(ping_val)).await;
                            }
                        }

                        (Stage::InChat, ids::SID_FRIENDSLIST)
                        | (Stage::AwaitEnterChat, ids::SID_FRIENDSLIST) => {
                            if let Ok(fl) = incoming::decode_friendslist(&frame.payload) {
                                _friends = fl.clone();
                                let _ = events.send(BnetEvent::FriendsList(fl)).await;
                            }
                        }

                        (Stage::InChat, ids::SID_CLANMEMBERLIST)
                        | (Stage::AwaitEnterChat, ids::SID_CLANMEMBERLIST) => {
                            if let Ok(cl) = incoming::decode_clanmemberlist(&frame.payload) {
                                _clan_members = cl.clone();
                                let _ = events.send(BnetEvent::ClanList(cl)).await;
                            }
                        }

                        (Stage::InChat, ids::SID_CLANCREATIONINVITATION) => {
                            if let Ok(invite) =
                                incoming::decode_clancreationinvitation(&frame.payload)
                            {
                                last_clan_invite_tag = invite.tag;
                                last_clan_invite_name = invite.inviter_name.clone();
                                last_invite_creation = true;
                                tracing::info!(
                                    clan = %invite.clan_name,
                                    inviter = %invite.inviter_name,
                                    "[BNET: {}] Invited (creation) to clan {}, !accept to accept",
                                    cfg.server_alias,
                                    invite.clan_name
                                );
                                let _ = events
                                    .send(BnetEvent::ClanInviteReceived {
                                        clan_name: invite.clan_name,
                                        inviter: invite.inviter_name,
                                        creation: true,
                                    })
                                    .await;
                            }
                        }

                        (Stage::InChat, ids::SID_CLANINVITATIONRESPONSE) => {
                            if let Ok(invite) =
                                incoming::decode_claninvitationresponse(&frame.payload)
                            {
                                last_clan_invite_tag = invite.tag;
                                last_clan_invite_name = invite.inviter_name.clone();
                                last_invite_creation = false;
                                tracing::info!(
                                    clan = %invite.clan_name,
                                    inviter = %invite.inviter_name,
                                    "[BNET: {}] Invited to clan {}, !accept to accept",
                                    cfg.server_alias,
                                    invite.clan_name
                                );
                                let _ = events
                                    .send(BnetEvent::ClanInviteReceived {
                                        clan_name: invite.clan_name,
                                        inviter: invite.inviter_name,
                                        creation: false,
                                    })
                                    .await;
                            }
                        }

                        (_, ids::SID_WARDEN) => {
                            if let Ok(w_data) = incoming::decode_warden(&frame.payload) {
                                tracing::warn!(
                                    len = w_data.len(),
                                    "[BNET: {}] warning - received warden packet but no BNLS server is available, you will be kicked from battle.net soon",
                                    cfg.server_alias
                                );
                            }
                        }

                        (_, ids::SID_CHECKAD) => {
                            if incoming::decode_checkad(&frame.payload).is_ok() {
                                let _ = framed_write.send(outgoing::checkad()).await;
                            }
                        }

                        (Stage::InChat, ids::SID_CLANCHANGERANK) => {
                            let status = if frame.payload.len() >= 5 {
                                frame.payload[4]
                            } else {
                                frame.payload.first().copied().unwrap_or(0)
                            };
                            tracing::info!(
                                "[BNET: {}] Received SID_CLANCHANGERANK response, status: {}",
                                cfg.server_alias,
                                status
                            );
                            let _ = events.send(BnetEvent::ClanRankChanged { status }).await;
                        }

                        (Stage::InChat, ids::SID_CLANREMOVEMEMBER) => {
                            let status = if frame.payload.len() >= 5 {
                                frame.payload[4]
                            } else {
                                frame.payload.first().copied().unwrap_or(0)
                            };
                            tracing::info!(
                                "[BNET: {}] Received SID_CLANREMOVEMEMBER response, status: {}",
                                cfg.server_alias,
                                status
                            );
                            let _ = events.send(BnetEvent::ClanMemberRemoved { status }).await;
                        }

                        (Stage::InChat, ids::SID_CLANSETMOTD) => {
                            let status = if frame.payload.len() >= 5 {
                                frame.payload[4]
                            } else {
                                frame.payload.first().copied().unwrap_or(0)
                            };
                            tracing::info!(
                                "[BNET: {}] Received SID_CLANSETMOTD response, status: {}",
                                cfg.server_alias,
                                status
                            );
                            let _ = events.send(BnetEvent::ClanMotdSet { status }).await;
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
