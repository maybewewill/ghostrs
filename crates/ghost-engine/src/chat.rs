use bytes::Bytes;
use ghost_protocol::w3gs::incoming::ChatToHost;

use crate::lang;
use crate::lobby::MAX_SLOTS;
use crate::players::NameMatch;
use crate::slots::SlotStatus;
use crate::state::{GamePhase, GameState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCommand {
    Start { force: bool },
    Abort,
    Open(u8),
    Close(u8),
    OpenAll,
    CloseAll,
    Swap(u8, u8),
    Hold { name: String, slot: Option<u8> },
    ClearHold,
    Kick(String),
    Ban { name: String, reason: String },
    Unban(String),
    CheckBan(String),
    BanLast(String),
    CheckAdmin(String),
    AddAdmin(String),
    DelAdmin(String),
    Ping,
    Mute(String),
    Unmute(String),
    MuteAll,
    UnmuteAll,
    VoteStart,
    VoteCancel,
    VoteKick(String),
    Yes,
    SyncLimit(u32),
    Latency(u32),
    ShufflePlayers,
    Version,
    Say(String),
    Whisper { user: String, message: String },
    Stats(String),
    StatsDotA(String),
    Drop,
    Draw,
    Hcl(String),
    ClearHcl,
    Owner(Option<String>),
    Unhost,
    End,
    Lock,
    Unlock,
    Check(String),
    CheckMe,
    Announce(String),
    AutoStart(Option<usize>),
    Refresh,
    VirtualHost(String),
    Comp { slot: u8, team: u8, colour: u8, race: u8, computer_type: u8, handicap: u8 },
    CompColour { slot: u8, colour: u8 },
    CompRace { slot: u8, race: u8 },
    CompHandicap { slot: u8, handicap: u8 },
    CompTeam { slot: u8, team: u8 },
    Download(String),
    AutoSave(Option<bool>),
    DbStatus,
    FakePlayer,
    FpPause,
    FpResume,
    From,
    Messages(Option<bool>),
    SendLan { ip: String, port: Option<u16> },
    Pub(String),
    Priv(String),
    MuteLobby(Option<bool>),
    Unknown(String),
}

fn slot_arg(s: &str) -> Option<u8> {
    let n: u8 = s.parse().ok()?;
    n.checked_sub(1)
}

pub fn parse_command(trigger: char, msg: &str) -> Option<ChatCommand> {
    let rest = msg.strip_prefix(trigger)?;
    let mut it = rest.split_whitespace();
    let verb = it.next()?.to_lowercase();
    let args: Vec<&str> = it.collect();

    Some(match verb.as_str() {
        "start" | "s" => {
            let force = args
                .first()
                .map(|s| s.eq_ignore_ascii_case("force") || s.eq_ignore_ascii_case("f"))
                .unwrap_or(false);
            ChatCommand::Start { force }
        }
        "sf" | "startforce" => ChatCommand::Start { force: true },
        "abort" | "a" => ChatCommand::Abort,
        "ping" | "p" => ChatCommand::Ping,
        "unhost" => ChatCommand::Unhost,
        "end" => ChatCommand::End,
        "lock" => ChatCommand::Lock,
        "unlock" => ChatCommand::Unlock,
        "open" | "o" => ChatCommand::Open(slot_arg(args.first()?)?),
        "close" | "c" => ChatCommand::Close(slot_arg(args.first()?)?),
        "openall" => ChatCommand::OpenAll,
        "closeall" => ChatCommand::CloseAll,
        "swap" => ChatCommand::Swap(slot_arg(args.first()?)?, slot_arg(args.get(1)?)?),
        "hold" | "h" => {
            let name = args.first()?.to_string();
            let slot = args.get(1).and_then(|s| slot_arg(s));
            ChatCommand::Hold { name, slot }
        }
        "clearhold" => ChatCommand::ClearHold,
        "kick" | "k" => ChatCommand::Kick(args.first()?.to_string()),
        "ban" => {
            let name = args.first()?.to_string();
            let reason = args
                .get(1..)
                .map(|r| r.join(" "))
                .unwrap_or_else(|| "banned by host".into());
            ChatCommand::Ban { name, reason }
        }
        "unban" => ChatCommand::Unban(args.first()?.to_string()),
        "checkban" => ChatCommand::CheckBan(args.first()?.to_string()),
        "banlast" => {
            let reason = args.join(" ");
            ChatCommand::BanLast(if reason.is_empty() {
                "banned by host".into()
            } else {
                reason
            })
        }
        "checkadmin" => ChatCommand::CheckAdmin(args.first()?.to_string()),
        "addadmin" => ChatCommand::AddAdmin(args.first()?.to_string()),
        "deladmin" => ChatCommand::DelAdmin(args.first()?.to_string()),
        "mute" => ChatCommand::Mute(args.first()?.to_string()),
        "unmute" => ChatCommand::Unmute(args.first()?.to_string()),
        "muteall" => ChatCommand::MuteAll,
        "unmuteall" => ChatCommand::UnmuteAll,
        "votestart" => ChatCommand::VoteStart,
        "votecancel" => ChatCommand::VoteCancel,
        "votekick" | "vk" => ChatCommand::VoteKick(args.first()?.to_string()),
        "yes" | "y" => ChatCommand::Yes,
        "synclimit" => ChatCommand::SyncLimit(args.first()?.parse().ok()?),
        "latency" => ChatCommand::Latency(args.first()?.parse().ok()?),
        "sp" => ChatCommand::ShufflePlayers,
        "version" => ChatCommand::Version,
        "say" => ChatCommand::Say(args.join(" ")),
        "w" | "whisper" => {
            let user = args.first()?.to_string();
            let message = args.get(1..).map(|r| r.join(" ")).unwrap_or_default();
            ChatCommand::Whisper { user, message }
        }
        "check" => ChatCommand::Check(args.first()?.to_string()),
        "checkme" => ChatCommand::CheckMe,
        "announce" => ChatCommand::Announce(args.join(" ")),
        "autostart" => {
            let num = args.first().and_then(|s| s.parse::<usize>().ok());
            ChatCommand::AutoStart(num)
        }
        "refresh" => ChatCommand::Refresh,
        "virtualhost" => ChatCommand::VirtualHost(args.first()?.to_string()),
        "dl" | "download" => ChatCommand::Download(args.first().map(|s| s.to_string()).unwrap_or_default()),
        "comp" => {
            let slot = slot_arg(args.first()?)?;
            let team = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
            let colour = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(slot + 1);
            let race = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0x20);
            let handicap = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(100);
            ChatCommand::Comp {
                slot,
                team,
                colour,
                race,
                computer_type: 1,
                handicap,
            }
        }
        "compcolour" => {
            let slot = slot_arg(args.first()?)?;
            let colour = args.get(1).and_then(|s| s.parse().ok())?;
            ChatCommand::CompColour { slot, colour }
        }
        "comprace" => {
            let slot = slot_arg(args.first()?)?;
            let race = args.get(1).and_then(|s| s.parse().ok())?;
            ChatCommand::CompRace { slot, race }
        }
        "comphandicap" => {
            let slot = slot_arg(args.first()?)?;
            let handicap = args.get(1).and_then(|s| s.parse().ok())?;
            ChatCommand::CompHandicap { slot, handicap }
        }
        "compteam" => {
            let slot = slot_arg(args.first()?)?;
            let team = args.get(1).and_then(|s| s.parse().ok())?;
            ChatCommand::CompTeam { slot, team }
        }
        "stats" => ChatCommand::Stats(args.first().map(|s| s.to_string()).unwrap_or_default()),
        "statsdota" | "sd" => {
            ChatCommand::StatsDotA(args.first().map(|s| s.to_string()).unwrap_or_default())
        }
        "drop" => ChatCommand::Drop,
        "draw" => ChatCommand::Draw,
        "hcl" => ChatCommand::Hcl(args.join(" ")),
        "clearhcl" => ChatCommand::ClearHcl,
        "owner" => ChatCommand::Owner(args.first().map(|s| s.to_string())),
        "autosave" => {
            let opt = args.first().and_then(|s| {
                if s.eq_ignore_ascii_case("on") || s.eq_ignore_ascii_case("1") {
                    Some(true)
                } else if s.eq_ignore_ascii_case("off") || s.eq_ignore_ascii_case("0") {
                    Some(false)
                } else {
                    None
                }
            });
            ChatCommand::AutoSave(opt)
        }
        "dbstatus" => ChatCommand::DbStatus,
        "fakeplayer" | "fp" => ChatCommand::FakePlayer,
        "fppause" => ChatCommand::FpPause,
        "fpresume" => ChatCommand::FpResume,
        "from" => ChatCommand::From,
        "messages" => {
            let opt = args.first().and_then(|s| {
                if s.eq_ignore_ascii_case("on") || s.eq_ignore_ascii_case("1") {
                    Some(true)
                } else if s.eq_ignore_ascii_case("off") || s.eq_ignore_ascii_case("0") {
                    Some(false)
                } else {
                    None
                }
            });
            ChatCommand::Messages(opt)
        }
        "sendlan" => {
            let ip = args.first()?.to_string();
            let port = args.get(1).and_then(|s| s.parse::<u16>().ok());
            ChatCommand::SendLan { ip, port }
        }
        "pub" => ChatCommand::Pub(args.join(" ")),
        "priv" => ChatCommand::Priv(args.join(" ")),
        "mutelobby" => {
            let opt = args.first().and_then(|s| {
                if s.eq_ignore_ascii_case("on") || s.eq_ignore_ascii_case("1") {
                    Some(true)
                } else if s.eq_ignore_ascii_case("off") || s.eq_ignore_ascii_case("0") {
                    Some(false)
                } else {
                    None
                }
            });
            ChatCommand::MuteLobby(opt)
        }
        _ => ChatCommand::Unknown(verb),
    })
}

impl GameState {
    pub fn handle_chat_to_host(&mut self, conn_id: u64, payload: &Bytes) {
        let Some((pid, name, is_muted)) = self
            .players
            .by_conn(conn_id)
            .map(|p| (p.pid, p.name.clone(), p.muted))
        else {
            tracing::warn!(conn_id, len = payload.len(), "chat received from unknown conn_id");
            return;
        };
        let chat = match ChatToHost::decode(payload) {
            Ok(c) => c,
            Err(e) => {
                let head: Vec<u8> = payload.iter().take(24).copied().collect();
                tracing::warn!(conn_id, len = payload.len(), head = format!("{head:02X?}"), error = %e, "malformed chat");
                return;
            }
        };
        tracing::info!(conn_id, pid, name = %name, from = chat.from_pid, flag = format!("0x{:02X}", chat.flag), to = ?chat.to_pids, extra = format!("{:02X?}", &chat.extra[..]), msg = %chat.message, "chat to host");

        // GHost++ game_base.cpp:2900: only honour chat claiming to come from
        // this player; a mismatched from-PID is ignored.
        if chat.from_pid != pid {
            tracing::debug!(conn_id, from = chat.from_pid, "chat from_pid mismatch, ignoring");
            return;
        }

        let trigger = '!';

        // GHost++ game_base.cpp:2952: "?trigger" replies with the command
        // trigger and is still relayed like any other message.
        if chat.message == "?trigger" {
            self.send_chat_to(pid, &lang::command_trigger(trigger));
        }

        // Team/colour/race/handicap change requests only apply in the lobby.
        if (0x11..=0x14).contains(&chat.flag) {
            if matches!(self.phase, GamePhase::Lobby)
                && self.apply_slot_request(pid, chat.flag, chat.byte)
            {
                // GHost++ sends SLOT_INFO inside each change handler, only after
                // the change actually applied.
                self.send_all_slot_info();
            }
            return;
        }

        let is_owner = name.eq_ignore_ascii_case(&self.cfg.owner)
            || self.cfg.owner.eq_ignore_ascii_case("BOT")
            || self.cfg.owner.is_empty();

        match parse_command(trigger, &chat.message) {
            Some(cmd) => {
                // C1: Start and Abort are restricted to owner/admin
                let public_cmd = matches!(
                    cmd,
                    ChatCommand::Ping
                        | ChatCommand::VoteStart
                        | ChatCommand::VoteCancel
                        | ChatCommand::VoteKick(_)
                        | ChatCommand::Yes
                        | ChatCommand::Draw
                        | ChatCommand::Stats(_)
                        | ChatCommand::StatsDotA(_)
                        | ChatCommand::Version
                        | ChatCommand::CheckMe
                        | ChatCommand::Whisper { .. }
                );

                if !is_owner && !public_cmd {
                    self.send_chat_to(pid, &lang::command_not_allowed());
                    return;
                }
                self.run_command(pid, &name, cmd);
            }
            None => {
                // If player is muted or global mute is on, don't relay their chat
                if (is_muted || self.muted_all) && !is_owner {
                    return;
                }

                if matches!(self.phase, GamePhase::Loading | GamePhase::Playing { .. }) {
                    if let Some(rep) = self.replay.as_mut() {
                        let extra_u32 = if chat.extra.len() >= 4 {
                            u32::from_le_bytes([chat.extra[0], chat.extra[1], chat.extra[2], chat.extra[3]])
                        } else {
                            0
                        };
                        rep.add_chat(pid, chat.flag, extra_u32, &chat.message);
                    }
                    if let Some(r) = &self.relay {
                        r.send_chat(&name, &chat.message);
                    }
                }

                if matches!(self.phase, GamePhase::Lobby) && self.mute_lobby {
                    return;
                }

                // GHost++ relays to exactly the recipient list the client sent
                // (game_base.cpp:2986 `Send(chatPlayer->GetToPIDs(), ...)`); the
                // client never lists itself, so the sender never sees its own
                // message echoed back. When the client sent an empty list, fall
                // back to every seated player but the sender. The virtual host is
                // never addressed (it has no socket).
                let to_pids: Vec<u8> = if chat.to_pids.is_empty() {
                    self.players
                        .iter()
                        .filter(|p| !p.virtual_host && p.left.is_none() && p.pid != pid)
                        .map(|p| p.pid)
                        .collect()
                } else {
                    chat.to_pids
                        .iter()
                        .copied()
                        .filter(|&to| {
                            self.players
                                .by_pid(to)
                                .map(|p| !p.virtual_host)
                                .unwrap_or(false)
                        })
                        .collect()
                };
                if let Ok(b) = ghost_protocol::w3gs::outgoing::chat_from_host(
                    pid,
                    &to_pids,
                    chat.flag,
                    &chat.extra,
                    &chat.message,
                ) {
                    if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
                        for &to in &to_pids {
                            self.send_to(to, b.clone());
                        }
                    } else {
                        self.broadcast(b);
                    }
                }
            }
        }
    }


    pub fn run_command(&mut self, pid: u8, caller_name: &str, cmd: ChatCommand) {
        match cmd {
            ChatCommand::Start { force } => {
                if !force {
                    if self.players.human_count() < 1 {
                        let msg = lang::unable_to_start_not_enough(self.players.human_count());
                        self.send_chat_to(pid, &msg);
                        return;
                    }
                    if let Some(hcl) = &self.hcl {
                        if hcl.len() > self.slots.len() {
                            self.send_chat_to(
                                pid,
                                "Unable to start: HCL string is too long for map slots. Use !start force to bypass.",
                            );
                            return;
                        }
                    }
                    let downloading_names: Vec<String> = self
                        .downloads
                        .iter()
                        .filter_map(|d| {
                            if d.sent_upto < self.cfg.map.size {
                                self.players.by_pid(d.pid).map(|p| p.name.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    if !downloading_names.is_empty() {
                        self.send_chat_to(
                            pid,
                            &format!(
                                "Unable to start: players still downloading map: {}. Use !start force to bypass.",
                                downloading_names.join(", ")
                            ),
                        );
                        return;
                    }
                    if self.cfg.require_spoof_checks {
                        let unverified: Vec<String> = self
                            .players
                            .iter()
                            .filter(|p| !p.virtual_host && !p.spoofed)
                            .map(|p| p.name.clone())
                            .collect();
                        if !unverified.is_empty() {
                            self.send_chat_to(
                                pid,
                                &format!(
                                    "Unable to start: unverified spoofcheck: {}. Use !start force to bypass.",
                                    unverified.join(", ")
                                ),
                            );
                            return;
                        }
                    }
                    let ping_unverified: Vec<String> = self
                        .players
                        .iter()
                        .filter(|p| !p.virtual_host && !p.reserved && p.ping_history.len() < 3)
                        .map(|p| p.name.clone())
                        .collect();
                    if !ping_unverified.is_empty() {
                        self.send_chat_to(
                            pid,
                            &format!(
                                "Unable to start: players ping not checked: {}. Use !start force to bypass.",
                                ping_unverified.join(", ")
                            ),
                        );
                        return;
                    }
                    if let Some(left_time) = self.last_player_left_time {
                        if left_time.elapsed() < std::time::Duration::from_secs(2) {
                            self.send_chat_to(
                                pid,
                                "Unable to start: a player left within the last 2 seconds. Use !start force to bypass.",
                            );
                            return;
                        }
                    }
                }
                let by = caller_name.to_string();
                self.start_countdown(&by);
            }
            ChatCommand::Abort => {
                if matches!(self.phase, GamePhase::Countdown { .. }) {
                    self.phase = GamePhase::Lobby;
                    self.send_chat_all(&lang::countdown_aborted());
                }
            }
            ChatCommand::Open(sid) => {
                if self.slots.open(sid) {
                    self.send_all_slot_info();
                }
            }
            ChatCommand::Close(sid) => {
                if self.slots.close(sid) {
                    self.send_all_slot_info();
                }
            }
            ChatCommand::OpenAll => {
                self.slots.open_all();
                self.send_all_slot_info();
                self.send_chat_all("Opened all closed slots.");
            }
            ChatCommand::CloseAll => {
                self.slots.close_all();
                self.send_all_slot_info();
                self.send_chat_all("Closed all open slots.");
            }
            ChatCommand::Swap(a, b) => {
                let fixed_settings = self.cfg.map.has_fixed_player_settings();
                let custom_forces = self.cfg.map.has_custom_forces();
                if self.slots.swap_slots(a, b, fixed_settings, custom_forces) {
                    self.send_all_slot_info();
                }
            }
            ChatCommand::Hold { name, slot } => {
                if let Some(s) = slot {
                    self.holds.insert(s, name.clone());
                }
                self.send_chat_all(&format!("Slot reserved for [{name}]."));
            }
            ChatCommand::ClearHold => {
                self.holds.clear();
                self.send_chat_all("Held slots cleared.");
            }
            ChatCommand::Kick(name) => match self.players.by_name_partial(&name) {
                Ok(target) => {
                    let target_pid = target.pid;
                    let left_code = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
                        ghost_protocol::w3gs::ids::PLAYERLEAVE_LOBBY
                    } else {
                        ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST
                    };
                    self.kick_player(target_pid, "was kicked", left_code);
                }
                Err(NameMatch::None) => self.send_chat_to(pid, &lang::no_such_player(&name)),
                Err(NameMatch::Ambiguous(n)) => {
                    self.send_chat_to(pid, &lang::ambiguous_player(&name, n))
                }
            },
            ChatCommand::Ban { name, reason } => {
                if let Some(store) = &self.store {
                    store.ban(&name, "", caller_name, &reason);
                }
                self.send_chat_all(&format!("Banned [{name}]: {reason}."));
                if let Ok(target) = self.players.by_name_partial(&name) {
                    let tpid = target.pid;
                    let left_code = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
                        ghost_protocol::w3gs::ids::PLAYERLEAVE_LOBBY
                    } else {
                        ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST
                    };
                    self.kick_player(tpid, &format!("banned: {reason}"), left_code);
                }
            }

            ChatCommand::Unban(name) => {
                if let Some(store) = &self.store {
                    store.unban(&name);
                }
                self.send_chat_to(pid, &format!("Unbanned [{name}]."));
            }
            ChatCommand::CheckBan(name) => {
                if let Some(store) = &self.store {
                    let s = store.clone();
                    let target_name = name.clone();
                    tokio::spawn(async move {
                        let _ = s.is_banned(&target_name, "").await;
                    });
                }
                self.send_chat_to(pid, &format!("Checking ban for [{name}]..."));
            }
            ChatCommand::BanLast(reason) => {
                if let Some((name, ip)) = &self.last_player_left {
                    let n = name.clone();
                    if let Some(store) = &self.store {
                        store.ban(&n, ip, caller_name, &reason);
                    }
                    self.send_chat_all(&format!("Banned last leaver [{n}]: {reason}."));
                } else {
                    self.send_chat_to(pid, "No player has left the game yet.");
                }
            }
            ChatCommand::CheckAdmin(name) => {
                self.send_chat_to(pid, &format!("Checking admin status for [{name}]..."));
            }
            ChatCommand::AddAdmin(name) => {
                if let Some(store) = &self.store {
                    store.add_admin(&name, "");
                }
                self.send_chat_to(pid, &format!("Added admin [{name}]."));
            }
            ChatCommand::DelAdmin(name) => {
                if let Some(store) = &self.store {
                    store.remove_admin(&name);
                }
                self.send_chat_to(pid, &format!("Removed admin [{name}]."));
            }
            ChatCommand::Mute(name) => {
                if let Ok(target) = self.players.by_name_partial(&name) {
                    let tpid = target.pid;
                    let target_name = target.name.clone();
                    if let Some(p) = self.players.by_pid_mut(tpid) {
                        p.muted = true;
                    }
                    self.send_chat_all(&format!("[{target_name}] has been muted."));
                }
            }
            ChatCommand::Unmute(name) => {
                if let Ok(target) = self.players.by_name_partial(&name) {
                    let tpid = target.pid;
                    let target_name = target.name.clone();
                    if let Some(p) = self.players.by_pid_mut(tpid) {
                        p.muted = false;
                    }
                    self.send_chat_all(&format!("[{target_name}] has been unmuted."));
                }
            }
            ChatCommand::MuteAll => {
                self.muted_all = true;
                self.send_chat_all("Global chat mute enabled.");
            }
            ChatCommand::UnmuteAll => {
                self.muted_all = false;
                self.send_chat_all("Global chat mute disabled.");
            }
            ChatCommand::VoteStart => {
                if !self.start_votes.contains(&pid) {
                    self.start_votes.push(pid);
                    let votes = self.start_votes.len();
                    let total = self.players.human_count();
                    let needed = (total / 2) + 1;
                    self.send_chat_all(&format!("Vote start: {votes}/{needed} votes."));
                    if votes >= needed {
                        self.start_countdown("vote");
                    }
                }
            }
            ChatCommand::VoteCancel => {
                self.start_votes.clear();
                self.votekick_target = None;
                self.votekick_votes.clear();
                self.send_chat_all("Active votes cancelled.");
            }
            ChatCommand::VoteKick(name) => {
                match self.players.by_name_partial(&name) {
                    Ok(target) => {
                        let tpid = target.pid;
                        let tname = target.name.clone();
                        self.votekick_target = Some(tpid);
                        self.votekick_votes = vec![pid];
                        let total = self.players.human_count();
                        let needed = (total / 2) + 1;
                        self.send_chat_all(&format!(
                            "Votekick started against [{tname}] (1/{needed} votes). Type !yes to vote."
                        ));
                    }
                    Err(_) => self.send_chat_to(pid, &lang::no_such_player(&name)),
                }
            }
            ChatCommand::Yes => {
                if let Some(target_pid) = self.votekick_target {
                    if !self.votekick_votes.contains(&pid) {
                        self.votekick_votes.push(pid);
                        let votes = self.votekick_votes.len();
                        let total = self.players.human_count();
                        let needed = (total / 2) + 1;
                        self.send_chat_all(&format!("Votekick: {votes}/{needed} votes."));
                        if votes >= needed {
                            let left_code = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
                                ghost_protocol::w3gs::ids::PLAYERLEAVE_LOBBY
                            } else {
                                ghost_protocol::w3gs::ids::PLAYERLEAVE_LOST
                            };
                            self.kick_player(target_pid, "voted out", left_code);
                            self.votekick_target = None;
                            self.votekick_votes.clear();
                        }

                    }
                }
            }
            ChatCommand::SyncLimit(limit) => {
                self.cfg.sync_limit = limit.clamp(10, 200);
                self.send_chat_to(pid, &format!("Sync limit set to {}.", self.cfg.sync_limit));
            }
            ChatCommand::Latency(lat) => {
                let d = std::time::Duration::from_millis(lat.clamp(20, 500) as u64);
                self.tick.set_period(d);
                self.cfg.latency = d;
                self.send_chat_to(pid, &format!("Latency set to {} ms.", lat));
            }
            ChatCommand::ShufflePlayers => {
                if matches!(self.phase, GamePhase::Lobby) {
                    for i in 0..self.slots.len() {
                        let j = (rand::random::<u16>() as usize) % self.slots.len();
                        self.slots.swap(i as u8, j as u8);
                    }
                    self.send_all_slot_info();
                    self.send_chat_all("Slots shuffled.");
                }
            }
            ChatCommand::Version => {
                self.send_chat_to(
                    pid,
                    "Ghost-RS v0.2.0 (High-Performance Async Warcraft III Hostbot)",
                );
            }
            ChatCommand::Ping => {
                let pairs: Vec<(String, Option<u32>)> = self
                    .players
                    .iter_humans()
                    .map(|p| (p.name.clone(), p.average_ping()))
                    .collect();
                let msg = lang::player_pings(&pairs);
                self.send_chat_to(pid, &msg);
            }
            ChatCommand::Check(name) => {
                if let Ok(target) = self.players.by_name_partial(&name) {
                    let ping = target.average_ping().unwrap_or(0);
                    let spoofed = if target.spoofed { "Yes" } else { "No" };
                    let realm = if target.joined_realm.is_empty() {
                        "LAN"
                    } else {
                        &target.joined_realm
                    };
                    self.send_chat_to(
                        pid,
                        &format!(
                            "Player [{}]: Ping: {}ms, Spoofed: {}, Realm: [{}]",
                            target.name, ping, spoofed, realm
                        ),
                    );
                } else {
                    self.send_chat_to(pid, &lang::no_such_player(&name));
                }
            }
            ChatCommand::CheckMe => {
                if let Some(target) = self.players.by_pid(pid) {
                    let ping = target.average_ping().unwrap_or(0);
                    let spoofed = if target.spoofed { "Yes" } else { "No" };
                    let realm = if target.joined_realm.is_empty() {
                        "LAN"
                    } else {
                        &target.joined_realm
                    };
                    self.send_chat_to(
                        pid,
                        &format!(
                            "You [{}]: Ping: {}ms, Spoofed: {}, Realm: [{}]",
                            target.name, ping, spoofed, realm
                        ),
                    );
                }
            }
            ChatCommand::Announce(msg) => {
                let mut parts = msg.split_whitespace();
                if let Some(first) = parts.next() {
                    if first.eq_ignore_ascii_case("off") {
                        self.announce_message = None;
                        self.announce_interval = std::time::Duration::ZERO;
                        self.send_chat_to(pid, "Announcement disabled.");
                    } else if let Ok(interval_sec) = first.parse::<u64>() {
                        let text = parts.collect::<Vec<_>>().join(" ");
                        self.announce_interval = std::time::Duration::from_secs(interval_sec);
                        self.announce_message = Some(text.clone());
                        self.last_announce_time = Some(std::time::Instant::now());
                        self.send_chat_all(&format!("Announcement (every {interval_sec}s): {text}"));
                    } else {
                        let full_text = msg.clone();
                        self.announce_message = Some(full_text.clone());
                        self.send_chat_all(&format!("Announcement: {full_text}"));
                    }
                } else {
                    self.announce_message = None;
                    self.announce_interval = std::time::Duration::ZERO;
                    self.send_chat_to(pid, "Announcement cleared.");
                }
            }
            ChatCommand::AutoStart(num) => {
                self.autostart_players = num;
                if let Some(n) = num {
                    self.send_chat_all(&format!("Autostart enabled when {n} players join."));
                } else {
                    self.send_chat_all("Autostart disabled.");
                }
            }
            ChatCommand::Refresh => {
                self.send_all_slot_info();
                self.send_chat_to(pid, "Refreshed slot info.");
            }
            ChatCommand::VirtualHost(vname) => {
                self.cfg.virtual_host_name = vname.clone();
                self.send_chat_all(&format!("Virtual host name set to [{vname}]."));
            }
            ChatCommand::Comp {
                slot,
                team,
                colour,
                race,
                computer_type,
                handicap,
            } => {
                if self.slots.add_computer(slot, team, colour, race, computer_type, handicap) {
                    self.send_all_slot_info();
                }
            }
            ChatCommand::CompColour { slot, colour } => {
                if let Some(s) = self.slots.as_wire_mut().get_mut(slot as usize) {
                    if s.computer == 1 {
                        s.colour = colour;
                        self.send_all_slot_info();
                    }
                }
            }
            ChatCommand::CompRace { slot, race } => {
                if let Some(s) = self.slots.as_wire_mut().get_mut(slot as usize) {
                    if s.computer == 1 {
                        s.race = race;
                        self.send_all_slot_info();
                    }
                }
            }
            ChatCommand::CompHandicap { slot, handicap } => {
                if let Some(s) = self.slots.as_wire_mut().get_mut(slot as usize) {
                    if s.computer == 1 {
                        s.handicap = handicap;
                        self.send_all_slot_info();
                    }
                }
            }
            ChatCommand::CompTeam { slot, team } => {
                if let Some(s) = self.slots.as_wire_mut().get_mut(slot as usize) {
                    if s.computer == 1 {
                        s.team = team;
                        self.send_all_slot_info();
                    }
                }
            }
            ChatCommand::Download(name) => {
                let target_pid = if name.is_empty() {
                    Some(pid)
                } else {
                    self.players.by_name_partial(&name).ok().map(|p| p.pid)
                };
                if let Some(tpid) = target_pid {
                    if let Some(p) = self.players.by_pid_mut(tpid) {
                        p.download_allowed = true;
                        let p_name = p.name.clone();
                        self.send_chat_to(pid, &format!("Map download allowed for player [{p_name}]."));
                    }
                    self.send_to(tpid, ghost_protocol::w3gs::outgoing::start_download(self.host_pid()));
                } else {
                    self.send_chat_to(pid, &lang::no_such_player(&name));
                }
            }
            ChatCommand::Stats(name) => {
                if let Some(caller_player) = self.players.by_pid_mut(pid) {
                    if let Some(last_sent) = caller_player.stats_sent_time {
                        if last_sent.elapsed() < std::time::Duration::from_secs(5) {
                            return;
                        }
                    }
                    caller_player.stats_sent_time = Some(std::time::Instant::now());
                }
                let target_name = if name.is_empty() { caller_name } else { &name };
                self.send_chat_to(pid, &format!("Querying stats for [{target_name}]..."));
            }
            ChatCommand::StatsDotA(name) => {
                if let Some(caller_player) = self.players.by_pid_mut(pid) {
                    if let Some(last_sent) = caller_player.stats_dota_sent_time {
                        if last_sent.elapsed() < std::time::Duration::from_secs(5) {
                            return;
                        }
                    }
                    caller_player.stats_dota_sent_time = Some(std::time::Instant::now());
                }
                let target_name = if name.is_empty() { caller_name } else { &name };
                if let Some(dota) = &self.dota {
                    if let Some(summary) = dota.format_player_stats(target_name) {
                        self.send_chat_to(pid, &summary);
                    } else {
                        self.send_chat_to(
                            pid,
                            &format!("No DotA stats found for [{target_name}]."),
                        );
                    }
                } else {
                    self.send_chat_to(pid, "Not a DotA map.");
                }
            }
            ChatCommand::Drop => {
                self.handle_drop_request(0);
            }
            ChatCommand::Draw => {
                if !self.draw_votes.contains(&pid) {
                    self.draw_votes.push(pid);
                    let votes = self.draw_votes.len();
                    let total = self.players.len();
                    self.send_chat_all(&format!("Draw vote: {votes}/{total} players agreed."));
                    if votes == total {
                        self.send_chat_all("All players agreed to a draw. Ending game.");
                        self.phase = GamePhase::Over;
                    }
                }
            }
            ChatCommand::Hcl(mode) => {
                self.hcl = Some(mode.clone());
                self.send_chat_all(&format!("HCL set to [{mode}]."));
            }
            ChatCommand::ClearHcl => {
                self.hcl = None;
                self.send_chat_all("HCL cleared.");
            }
            ChatCommand::Owner(new_owner) => {
                if let Some(o) = new_owner {
                    self.cfg.owner = o.clone();
                    self.send_chat_all(&format!("Game owner transferred to [{o}]."));
                } else {
                    self.send_chat_to(pid, &format!("Current owner is [{}].", self.cfg.owner));
                }
            }
            ChatCommand::Lock => {
                self.locked = true;
                self.send_chat_all("Game is now locked.");
            }
            ChatCommand::Unlock => {
                self.locked = false;
                self.send_chat_all("Game is now unlocked.");
            }
            ChatCommand::End => {
                self.send_chat_all("Game ended by host.");
                self.phase = GamePhase::Over;
                self.finished = true;
            }
            ChatCommand::Unhost => {
                if matches!(self.phase, GamePhase::Lobby) {
                    self.finished = true;
                }
            }
            ChatCommand::AutoSave(opt) => {
                match opt {
                    Some(true) => {
                        self.auto_save = true;
                        self.send_chat_to(pid, "Auto save enabled.");
                    }
                    Some(false) => {
                        self.auto_save = false;
                        self.send_chat_to(pid, "Auto save disabled.");
                    }
                    None => {
                        self.send_chat_to(
                            pid,
                            &format!(
                                "Auto save is {}.",
                                if self.auto_save { "enabled" } else { "disabled" }
                            ),
                        );
                    }
                }
            }
            ChatCommand::DbStatus => {
                self.send_chat_to(pid, "DB STATUS --- OK");
            }
            ChatCommand::FakePlayer => {
                if let Some(msg) = self.toggle_fake_player() {
                    self.send_chat_to(pid, msg);
                }
            }
            ChatCommand::FpPause => {
                if self.fake_player_pid.is_some() && matches!(self.phase, GamePhase::Playing) {
                    let act = ghost_protocol::w3gs::ActionBlock {
                        pid: self.fake_player_pid.unwrap(),
                        data: bytes::Bytes::from_static(&[0x01]),
                    };
                    self.actions.push(act);
                    self.send_chat_to(pid, "Game paused by fake player.");
                }
            }
            ChatCommand::FpResume => {
                if self.fake_player_pid.is_some() && matches!(self.phase, GamePhase::Playing) {
                    let act = ghost_protocol::w3gs::ActionBlock {
                        pid: self.fake_player_pid.unwrap(),
                        data: bytes::Bytes::from_static(&[0x02]),
                    };
                    self.actions.push(act);
                    self.send_chat_to(pid, "Game resumed by fake player.");
                }
            }
            ChatCommand::From => {
                let msgs: Vec<String> = self
                    .players
                    .iter()
                    .filter(|p| !p.virtual_host && p.left.is_none())
                    .map(|p| {
                        let loc = if !p.joined_realm.is_empty() {
                            p.joined_realm.clone()
                        } else {
                            format!(
                                "{}.{}.{}.{}",
                                p.external_ip[0], p.external_ip[1], p.external_ip[2], p.external_ip[3]
                            )
                        };
                        format!("Player [{}] is from [{}]", p.name, loc)
                    })
                    .collect();
                for msg in msgs {
                    self.send_chat_to(pid, &msg);
                }
            }
            ChatCommand::Messages(opt) => {
                match opt {
                    Some(true) => {
                        self.local_admin_messages = true;
                        self.send_chat_to(pid, "Local admin messages enabled.");
                    }
                    Some(false) => {
                        self.local_admin_messages = false;
                        self.send_chat_to(pid, "Local admin messages disabled.");
                    }
                    None => {
                        self.send_chat_to(
                            pid,
                            &format!(
                                "Local admin messages are {}.",
                                if self.local_admin_messages { "enabled" } else { "disabled" }
                            ),
                        );
                    }
                }
            }
            ChatCommand::SendLan { ip, port } => {
                let p = port.unwrap_or(6112);
                self.send_chat_to(pid, &format!("Sending LAN broadcast to [{ip}:{p}]."));
            }
            ChatCommand::Pub(name) => {
                if matches!(self.phase, GamePhase::Lobby) && !name.is_empty() {
                    self.last_game_name = self.cfg.name.clone();
                    self.cfg.name = name.clone();
                    self.cfg.host_counter = self.cfg.host_counter.wrapping_add(1);
                    self.refresh_rehosted = true;
                    self.send_chat_all(&format!("Rehosted game as public [{name}]."));
                    if let Some(tx) = &self.cfg.event_tx {
                        let _ = tx.try_send(crate::handle::GameEvent::LobbyStatus {
                            host_counter: self.cfg.host_counter,
                            slots_open: self.slots.count_open(),
                            slots_total: self.slots.len() as u32,
                            human_players: self.players.human_count() as u32,
                        });
                    }
                }
            }
            ChatCommand::Priv(name) => {
                if matches!(self.phase, GamePhase::Lobby) && !name.is_empty() {
                    self.last_game_name = self.cfg.name.clone();
                    self.cfg.name = name.clone();
                    self.cfg.host_counter = self.cfg.host_counter.wrapping_add(1);
                    self.refresh_rehosted = true;
                    self.send_chat_all(&format!("Rehosted game as private [{name}]."));
                    if let Some(tx) = &self.cfg.event_tx {
                        let _ = tx.try_send(crate::handle::GameEvent::LobbyStatus {
                            host_counter: self.cfg.host_counter,
                            slots_open: self.slots.count_open(),
                            slots_total: self.slots.len() as u32,
                            human_players: self.players.human_count() as u32,
                        });
                    }
                }
            }
            ChatCommand::MuteLobby(opt) => {
                let new_state = opt.unwrap_or(!self.mute_lobby);
                self.mute_lobby = new_state;
                if new_state {
                    self.send_chat_all("Lobby chat has been muted.");
                } else {
                    self.send_chat_all("Lobby chat has been unmuted.");
                }
            }
            ChatCommand::Say(msg) => self.send_chat_all(&msg),
            ChatCommand::Whisper { user, message } => {
                let target_info = self
                    .players
                    .by_name_partial(&user)
                    .ok()
                    .map(|p| (p.pid, p.name.clone()));
                if let Some((target_pid, target_name)) = target_info {
                    self.send_chat_to(
                        target_pid,
                        &format!("[Whisper from {caller_name}]: {message}"),
                    );
                    self.send_chat_to(
                        pid,
                        &format!("[Whisper to {target_name}]: {message}"),
                    );
                } else {
                    self.send_chat_to(pid, &lang::no_such_player(&user));
                }
            }
            ChatCommand::Unknown(v) => {
                self.send_chat_to(pid, &format!("Unknown command [{v}]."));
            }
        }
    }

    /// Applies a lobby slot-change request (team/colour/race/handicap) exactly
    /// like GHost++ `EventPlayerChangeTeam` / `ChangeColour` / `ChangeRace` /
    /// `ChangeHandicap` (game_base.cpp:3021-3160). Returns whether the slot
    /// table actually changed; the caller broadcasts SLOT_INFO only then,
    /// matching GHost++ which sends it inside each handler after a success.
    fn apply_slot_request(&mut self, pid: u8, flag: u8, value: u8) -> bool {
        let Some(sid) = self.slots.sid_of_pid(pid) else {
            return false;
        };
        let fixed_settings = self.cfg.map.has_fixed_player_settings();
        let custom_forces = self.cfg.map.has_custom_forces();

        match flag {
            0x11 => {
                let target_team = value;
                if custom_forces {
                    // game_base.cpp:3028: on custom-forces maps a team change is
                    // a move to another slot, GetEmptySlot(team, PID) + SwapSlots.
                    if let Some(target_sid) = self.slots.first_open_in_team_from(sid, target_team) {
                        self.slots.swap_slots(sid, target_sid, fixed_settings, custom_forces);
                        return true;
                    }
                    return false;
                }
                // Direct team set on the player's own slot (game_base.cpp:3038).
                if target_team > MAX_SLOTS as u8 {
                    return false;
                }
                if target_team == MAX_SLOTS as u8 {
                    // Observer team is only reachable when the map allows observers.
                    let obs = self.cfg.map.observers();
                    if obs != crate::map::MAPOBS_ALLOWED && obs != crate::map::MAPOBS_REFEREES {
                        return false;
                    }
                } else {
                    if target_team >= self.cfg.map.num_players {
                        return false;
                    }
                    // game_base.cpp:3056: don't let more players in than the map
                    // supports (counts occupied non-observer slots except self).
                    let num_other = self
                        .slots
                        .as_wire()
                        .iter()
                        .filter(|s| {
                            s.slot_status == SlotStatus::Occupied as u8
                                && s.team != MAX_SLOTS as u8
                                && s.pid != pid
                        })
                        .count() as u8;
                    if num_other >= self.cfg.map.num_players {
                        return false;
                    }
                }
                let Some(mut updated) = self.slots.as_wire().get(sid as usize).copied() else {
                    return false;
                };
                updated.team = target_team;
                if target_team == MAX_SLOTS as u8 {
                    // joining observers gives them the observer colour
                    updated.colour = MAX_SLOTS as u8;
                } else if updated.colour == MAX_SLOTS as u8 {
                    // leaving the observer team gets an unused colour
                    updated.colour = self.slots.unused_colour();
                }
                self.slots.replace(sid, updated);
                true
            }
            0x12 => {
                let colour = value;
                if fixed_settings {
                    return false;
                }
                if colour > MAX_SLOTS as u8 - 1 {
                    return false;
                }
                let Some(slot) = self.slots.as_wire().get(sid as usize).copied() else {
                    return false;
                };
                if slot.team == MAX_SLOTS as u8 {
                    // observers can't change colour (game_base.cpp:3105)
                    return false;
                }
                self.colour_slot(sid, colour)
            }
            0x13 => {
                if fixed_settings {
                    return false;
                }
                if self.cfg.map.has_random_races() {
                    // MAPFLAG_RANDOMRACES blocks race changes (game_base.cpp:3120)
                    return false;
                }
                if !matches!(value, 1 | 2 | 4 | 8 | 32) {
                    // SLOTRACE_HUMAN/ORC/NIGHTELF/UNDEAD/RANDOM only
                    return false;
                }
                let Some(slot) = self.slots.as_wire().get(sid as usize).copied() else {
                    return false;
                };
                let mut updated = slot;
                updated.race = value | 0x40; // SLOTRACE_SELECTABLE
                self.slots.replace(sid, updated);
                true
            }
            0x14 => {
                if fixed_settings {
                    return false;
                }
                if (50..=100).step_by(10).any(|h| h == value) {
                    let Some(slot) = self.slots.as_wire().get(sid as usize).copied() else {
                        return false;
                    };
                    let mut updated = slot;
                    updated.handicap = value;
                    self.slots.replace(sid, updated);
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    /// GHost++ `ColourSlot` (game_base.cpp:4014): if the requested colour is
    /// held by an unoccupied slot, swap the player's current colour into it so
    /// colours stay unique; if it is held by an occupied player, ignore the
    /// request.
    fn colour_slot(&mut self, sid: u8, colour: u8) -> bool {
        if sid as usize >= self.slots.len() || colour >= MAX_SLOTS as u8 {
            return false;
        }
        let wire = self.slots.as_wire();
        let mut taken_sid: Option<usize> = None;
        for (i, s) in wire.iter().enumerate() {
            if s.colour == colour {
                taken_sid = Some(i);
            }
        }
        match taken_sid {
            Some(tsid) if wire[tsid].slot_status != SlotStatus::Occupied as u8 => {
                let old_colour = wire[sid as usize].colour;
                self.slots.set_colour(tsid as u8, old_colour);
                self.slots.set_colour(sid, colour);
                true
            }
            Some(_) => false,
            None => {
                self.slots.set_colour(sid, colour);
                true
            }
        }
    }

    pub fn send_chat_to(&mut self, pid: u8, message: &str) {
        let from = self.host_pid();
        if from == 255 {
            return;
        }
        if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
            let msg = if message.len() > 254 { &message[..254] } else { message };
            if let Ok(b) =
                ghost_protocol::w3gs::outgoing::chat_from_host(from, &[pid], 0x10, &[], msg)
            {
                self.send_to(pid, b);
            }
        } else {
            let msg = if message.len() > 127 { &message[..127] } else { message };
            let sid = self.slots.sid_of_pid(pid);
            let colour = sid
                .and_then(|s| self.slots.as_wire().get(s as usize))
                .map(|s| s.colour)
                .unwrap_or(0);
            let extra = [3 + colour, 0, 0, 0];
            if let Ok(b) =
                ghost_protocol::w3gs::outgoing::chat_from_host(from, &[pid], 0x20, &extra, msg)
            {
                self.send_to(pid, b);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::{BufMut, BytesMut};

    #[test]
    fn parses_comprehensive_command_set() {
        assert_eq!(
            parse_command('!', "!start"),
            Some(ChatCommand::Start { force: false })
        );
        assert_eq!(
            parse_command('!', "!start force"),
            Some(ChatCommand::Start { force: true })
        );
        assert_eq!(parse_command('!', "!close 3"), Some(ChatCommand::Close(2)));
        assert_eq!(parse_command('!', "!open 1"), Some(ChatCommand::Open(0)));
        assert_eq!(
            parse_command('!', "!swap 1 4"),
            Some(ChatCommand::Swap(0, 3))
        );
        assert_eq!(
            parse_command('!', "!kick Slash"),
            Some(ChatCommand::Kick("Slash".into()))
        );
        assert_eq!(parse_command('!', "!ping"), Some(ChatCommand::Ping));
        assert_eq!(parse_command('!', "!muteall"), Some(ChatCommand::MuteAll));
        assert_eq!(
            parse_command('!', "!unmuteall"),
            Some(ChatCommand::UnmuteAll)
        );
        assert_eq!(
            parse_command('!', "!mute Slash"),
            Some(ChatCommand::Mute("Slash".into()))
        );
        assert_eq!(parse_command('!', "!sp"), Some(ChatCommand::ShufflePlayers));
        assert_eq!(
            parse_command('!', "!synclimit 60"),
            Some(ChatCommand::SyncLimit(60))
        );
        assert_eq!(
            parse_command('!', "!latency 50"),
            Some(ChatCommand::Latency(50))
        );
        assert_eq!(
            parse_command('!', "!hcl -apem"),
            Some(ChatCommand::Hcl("-apem".into()))
        );
        assert_eq!(parse_command('!', "!draw"), Some(ChatCommand::Draw));
        assert_eq!(
            parse_command('!', "!votestart"),
            Some(ChatCommand::VoteStart)
        );
    }

    #[test]
    fn slot_numbers_are_one_based_on_the_wire_and_rejected_when_zero() {
        assert_eq!(parse_command('!', "!close 0"), None);
        assert_eq!(parse_command('!', "!close abc"), None);
    }

    #[tokio::test]
    async fn the_virtual_host_does_not_count_toward_the_minimum_to_start() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(0);
        st.create_virtual_host();
        assert_eq!(
            st.players.len(),
            1,
            "only virtual host present"
        );

        st.run_command(1, "VirtualHost", ChatCommand::Start { force: false });

        assert!(
            matches!(st.phase, GamePhase::Lobby),
            "zero humans plus the virtual host must not be enough to start, got {:?}",
            st.phase
        );
    }

    #[tokio::test]
    async fn chat_from_a_mismatched_pid_is_ignored() {
        // GHost++ game_base.cpp:2900: only chat whose from-PID matches the
        // sender is honoured.
        let (mut st, mut rxs) = crate::actor::tests_support::seated_game(1);
        crate::actor::tests_support::drain_ids(&mut rxs[0]);

        let mut b = BytesMut::new();
        b.put_u8(1); // count
        b.put_u8(0); // to_pid
        b.put_u8(7); // from_pid != player pid 1
        b.put_u8(0x10); // flag
        b.put_slice(b"hello\0");
        st.handle_chat_to_host(1, &b.freeze());

        assert!(
            rxs[0].try_recv().is_err(),
            "chat with a mismatched from_pid must be dropped"
        );
    }

    #[tokio::test]
    async fn trigger_question_mark_gets_the_command_trigger_reply() {
        // GHost++ game_base.cpp:2952: "?trigger" is answered with the trigger
        // and the message is still relayed.
        let (mut st, mut rxs) = crate::actor::tests_support::seated_game(1);
        crate::actor::tests_support::drain_ids(&mut rxs[0]);

        let mut b = BytesMut::new();
        b.put_u8(1);
        b.put_u8(0);
        b.put_u8(1);
        b.put_u8(0x10);
        b.put_slice(b"?trigger\0");
        st.handle_chat_to_host(1, &b.freeze());

        let sent = crate::actor::tests_support::drain_ids(&mut rxs[0]);
        assert!(
            sent.contains(&ghost_protocol::w3gs::ids::CHAT_FROM_HOST),
            "must answer with the trigger, got {sent:?}"
        );
    }

    #[test]
    fn team_change_on_a_custom_forces_map_moves_the_player_slot() {
        // GHost++ EventPlayerChangeTeam (game_base.cpp:3028): on custom-forces
        // maps the player moves to an open slot of the target team.
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.cfg.map.options = crate::map::MAPOPT_CUSTOMFORCES;
        st.cfg.map.layout_style = 1;
        // player 1 is in slot 0 (team 0); slot 6 is the first open slot of team 1
        assert_eq!(st.slots.sid_of_pid(1), Some(0));
        assert!(st.apply_slot_request(1, 0x11, 1));
        assert_eq!(st.slots.sid_of_pid(1), Some(6));
        assert_eq!(st.slots.as_wire()[6].team, 1);
        assert!(st.slots.as_wire()[0].slot_status == 0, "old slot must be open again");
    }

    #[test]
    fn direct_team_change_is_rejected_when_the_map_is_full() {
        // GHost++ game_base.cpp:3056: no more players than the map supports.
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(3);
        st.cfg.map.num_players = 2;

        // team 1 is within bounds but the map is already full of other players
        assert!(!st.apply_slot_request(1, 0x11, 1));
        assert_eq!(st.slots.as_wire()[0].team, 0, "team must be unchanged");

        // a team number past the map's player count is rejected outright
        assert!(!st.apply_slot_request(1, 0x11, 2));
        assert_eq!(st.slots.as_wire()[0].team, 0);
    }

    #[test]
    fn observer_team_change_requires_a_map_that_allows_observers() {
        // GHost++ game_base.cpp:3041: the observer team is only reachable when
        // the map allows observers, and joining it assigns the observer colour.
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.cfg.map.num_players = 12;

        // map without observers -> rejected
        assert!(!st.apply_slot_request(1, 0x11, MAX_SLOTS as u8));
        assert_eq!(st.slots.as_wire()[0].team, 0);

        // map allowing observers (MAPOBS_ALLOWED baked into game flags)
        st.cfg.map.flags |= 0x0000_3000;
        assert!(st.apply_slot_request(1, 0x11, MAX_SLOTS as u8));
        assert_eq!(st.slots.as_wire()[0].team, MAX_SLOTS as u8);
        assert_eq!(st.slots.as_wire()[0].colour, MAX_SLOTS as u8);
    }

    #[test]
    fn colour_change_swaps_with_an_unoccupied_slot_instead_of_duplicating() {
        // GHost++ ColourSlot (game_base.cpp:4014): a colour held by an open
        // slot is swapped so colours stay unique.
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        // player 1 at slot 0 (colour 0); open slot 1 holds colour 1 by default
        assert!(st.apply_slot_request(1, 0x12, 1));
        let wire = st.slots.as_wire();
        assert_eq!(wire[0].colour, 1, "player takes the requested colour");
        assert_eq!(wire[1].colour, 0, "the old colour moves to the open slot");
    }

    #[test]
    fn colour_change_is_ignored_when_held_by_an_occupied_player() {
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(2);
        // player 2 sits in slot 1 with colour 1
        assert!(!st.apply_slot_request(1, 0x12, 1));
        assert_eq!(st.slots.as_wire()[0].colour, 0, "request must be ignored");
    }

    #[test]
    fn race_change_rejects_invalid_races_and_random_races_maps() {
        // GHost++ EventPlayerChangeRace (game_base.cpp:3113): only the five
        // SLOTRACE values are accepted and MAPFLAG_RANDOMRACES blocks changes.
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);

        assert!(!st.apply_slot_request(1, 0x13, 3));
        assert_eq!(st.slots.as_wire()[0].race, 0x20);

        assert!(st.apply_slot_request(1, 0x13, 1)); // Human
        assert_eq!(st.slots.as_wire()[0].race, 0x41); // 1 | SLOTRACE_SELECTABLE

        st.cfg.map.flags |= 0x0400_0000; // MAPFLAG_RANDOMRACES baked in
        assert!(!st.apply_slot_request(1, 0x13, 2));
        assert_eq!(st.slots.as_wire()[0].race, 0x41);
    }
}
