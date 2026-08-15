use bytes::Bytes;
use ghost_protocol::w3gs::incoming::ChatToHost;

use crate::lang;
use crate::players::NameMatch;
use crate::state::{GamePhase, GameState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCommand {
    Start { force: bool },
    Abort,
    Open(u8),
    Close(u8),
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
    Owner(Option<String>),
    Unhost,
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
        "start" => {
            let force = args.first().map(|s| s.eq_ignore_ascii_case("force")).unwrap_or(false);
            ChatCommand::Start { force }
        }
        "abort" => ChatCommand::Abort,
        "ping" => ChatCommand::Ping,
        "unhost" => ChatCommand::Unhost,
        "open" => ChatCommand::Open(slot_arg(args.first()?)?),
        "close" => ChatCommand::Close(slot_arg(args.first()?)?),
        "swap" => ChatCommand::Swap(slot_arg(args.first()?)?, slot_arg(args.get(1)?)?),
        "hold" => {
            let name = args.first()?.to_string();
            let slot = args.get(1).and_then(|s| slot_arg(s));
            ChatCommand::Hold { name, slot }
        }
        "clearhold" => ChatCommand::ClearHold,
        "kick" => ChatCommand::Kick(args.first()?.to_string()),
        "ban" => {
            let name = args.first()?.to_string();
            let reason = args.get(1..).map(|r| r.join(" ")).unwrap_or_else(|| "banned by host".into());
            ChatCommand::Ban { name, reason }
        }
        "unban" => ChatCommand::Unban(args.first()?.to_string()),
        "checkban" => ChatCommand::CheckBan(args.first()?.to_string()),
        "banlast" => {
            let reason = args.join(" ");
            ChatCommand::BanLast(if reason.is_empty() { "banned by host".into() } else { reason })
        }
        "checkadmin" => ChatCommand::CheckAdmin(args.first()?.to_string()),
        "addadmin" => ChatCommand::AddAdmin(args.first()?.to_string()),
        "deladmin" => ChatCommand::DelAdmin(args.first()?.to_string()),
        "mute" => ChatCommand::Mute(args.first()?.to_string()),
        "unmute" => ChatCommand::Unmute(args.first()?.to_string()),
        "muteall" => ChatCommand::MuteAll,
        "unmuteall" => ChatCommand::UnmuteAll,
        "votestart" => ChatCommand::VoteStart,
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
        "stats" => ChatCommand::Stats(args.first().map(|s| s.to_string()).unwrap_or_default()),
        "statsdota" => ChatCommand::StatsDotA(args.first().map(|s| s.to_string()).unwrap_or_default()),
        "drop" => ChatCommand::Drop,
        "draw" => ChatCommand::Draw,
        "hcl" => ChatCommand::Hcl(args.first()?.to_string()),
        "owner" => ChatCommand::Owner(args.first().map(|s| s.to_string())),
        other => ChatCommand::Unknown(other.to_string()),
    })
}

impl GameState {
    pub fn handle_chat_to_host(&mut self, conn_id: u64, payload: &Bytes) {
        let Some((pid, name, is_muted)) = self
            .players
            .by_conn(conn_id)
            .map(|p| (p.pid, p.name.clone(), p.muted))
        else {
            return;
        };
        let chat = match ChatToHost::decode(payload) {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(conn_id, error = %e, "malformed chat");
                return;
            }
        };

        // Team/colour/race/handicap change requests only apply in the lobby.
        if (0x11..=0x14).contains(&chat.flag) {
            if matches!(self.phase, GamePhase::Lobby) {
                self.apply_slot_request(pid, chat.flag, chat.byte);
                self.send_all_slot_info();
            }
            return;
        }

        let is_owner = name.eq_ignore_ascii_case(&self.cfg.owner);
        let trigger = '!';

        match parse_command(trigger, &chat.message) {
            Some(cmd) => {
                // Some commands are available to all players, others only to the owner
                let public_cmd = matches!(
                    cmd,
                    ChatCommand::Ping
                        | ChatCommand::VoteStart
                        | ChatCommand::Draw
                        | ChatCommand::Stats(_)
                        | ChatCommand::StatsDotA(_)
                        | ChatCommand::Version
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

                if let Ok(b) = ghost_protocol::w3gs::outgoing::chat_from_host(
                    pid,
                    &chat.to_pids,
                    chat.flag,
                    &chat.extra,
                    &chat.message,
                ) {
                    self.broadcast(b);
                }
            }
        }
    }

    pub fn run_command(&mut self, pid: u8, caller_name: &str, cmd: ChatCommand) {
        match cmd {
            ChatCommand::Start { force } => {
                if self.players.human_count() < 2 && !force {
                    let msg = lang::unable_to_start_not_enough(self.players.human_count());
                    self.send_chat_to(pid, &msg);
                } else {
                    let by = caller_name.to_string();
                    self.start_countdown(&by);
                }
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
            ChatCommand::Swap(a, b) => {
                if self.slots.swap(a, b) {
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
                    if let Some(p) = self.players.by_pid_mut(target_pid) {
                        p.left = Some("was kicked".into());
                    }
                }
                Err(NameMatch::None) => self.send_chat_to(pid, &lang::no_such_player(&name)),
                Err(NameMatch::Ambiguous(n)) => {
                    self.send_chat_to(pid, &lang::ambiguous_player(&name, n))
                }
            },
            ChatCommand::Ban { name, reason } => {
                self.send_chat_all(&format!("Banned [{name}]: {reason}."));
                if let Ok(target) = self.players.by_name_partial(&name) {
                    let tpid = target.pid;
                    if let Some(p) = self.players.by_pid_mut(tpid) {
                        p.left = Some(format!("banned: {reason}"));
                    }
                }
            }
            ChatCommand::Unban(name) => {
                self.send_chat_to(pid, &format!("Unbanned [{name}]."));
            }
            ChatCommand::CheckBan(name) => {
                self.send_chat_to(pid, &format!("Checking ban for [{name}]..."));
            }
            ChatCommand::BanLast(reason) => {
                if let Some((name, _ip)) = &self.last_player_left {
                    let n = name.clone();
                    self.send_chat_all(&format!("Banned last leaver [{n}]: {reason}."));
                } else {
                    self.send_chat_to(pid, "No player has left the game yet.");
                }
            }
            ChatCommand::CheckAdmin(name) => {
                self.send_chat_to(pid, &format!("Checking admin status for [{name}]..."));
            }
            ChatCommand::AddAdmin(name) => {
                self.send_chat_to(pid, &format!("Added admin [{name}]."));
            }
            ChatCommand::DelAdmin(name) => {
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
                self.send_chat_to(pid, "Ghost-RS v0.2.0 (High-Performance Async Warcraft III Hostbot)");
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
            ChatCommand::Stats(name) => {
                let target_name = if name.is_empty() { caller_name } else { &name };
                self.send_chat_to(pid, &format!("Querying stats for [{target_name}]..."));
            }
            ChatCommand::StatsDotA(name) => {
                let target_name = if name.is_empty() { caller_name } else { &name };
                if let Some(dota) = &self.dota {
                    if let Some(summary) = dota.format_player_stats(target_name) {
                        self.send_chat_to(pid, &summary);
                    } else {
                        self.send_chat_to(pid, &format!("No DotA stats found for [{target_name}]."));
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
            ChatCommand::Owner(new_owner) => {
                if let Some(o) = new_owner {
                    self.cfg.owner = o.clone();
                    self.send_chat_all(&format!("Game owner transferred to [{o}]."));
                } else {
                    self.send_chat_to(pid, &format!("Current owner is [{}].", self.cfg.owner));
                }
            }
            ChatCommand::Unhost => {
                if matches!(self.phase, GamePhase::Lobby) {
                    self.finished = true;
                }
            }
            ChatCommand::Say(msg) => self.send_chat_all(&msg),
            ChatCommand::Whisper { user, message } => {
                self.send_chat_to(pid, &format!("[Whisper -> {user}]: {message}"));
            }
            ChatCommand::Unknown(v) => {
                tracing::debug!(command = %v, "unknown command");
            }
        }
    }

    fn apply_slot_request(&mut self, pid: u8, flag: u8, value: u8) {
        let Some(sid) = self.slots.sid_of_pid(pid) else { return };
        let Some(slot) = self.slots.as_wire().get(sid as usize).copied() else { return };
        let mut updated = slot;
        match flag {
            0x11 => updated.team = value.min(11),
            0x12 => updated.colour = value.min(11),
            0x13 => updated.race = value,
            0x14 => updated.handicap = value.clamp(50, 100),
            _ => return,
        }
        self.slots.replace(sid, updated);
    }

    pub fn send_chat_to(&mut self, pid: u8, message: &str) {
        let flag = if matches!(self.phase, GamePhase::Lobby | GamePhase::Countdown { .. }) {
            0x10
        } else {
            0x20
        };
        let extra: &[u8] = if flag == 0x20 { &[0, 0, 0, 0] } else { &[] };
        if let Ok(b) = ghost_protocol::w3gs::outgoing::chat_from_host(255, &[pid], flag, extra, message) {
            self.send_to(pid, b);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comprehensive_command_set() {
        assert_eq!(parse_command('!', "!start"), Some(ChatCommand::Start { force: false }));
        assert_eq!(parse_command('!', "!start force"), Some(ChatCommand::Start { force: true }));
        assert_eq!(parse_command('!', "!close 3"), Some(ChatCommand::Close(2)));
        assert_eq!(parse_command('!', "!open 1"), Some(ChatCommand::Open(0)));
        assert_eq!(parse_command('!', "!swap 1 4"), Some(ChatCommand::Swap(0, 3)));
        assert_eq!(parse_command('!', "!kick Slash"), Some(ChatCommand::Kick("Slash".into())));
        assert_eq!(parse_command('!', "!ping"), Some(ChatCommand::Ping));
        assert_eq!(parse_command('!', "!muteall"), Some(ChatCommand::MuteAll));
        assert_eq!(parse_command('!', "!unmuteall"), Some(ChatCommand::UnmuteAll));
        assert_eq!(parse_command('!', "!mute Slash"), Some(ChatCommand::Mute("Slash".into())));
        assert_eq!(parse_command('!', "!sp"), Some(ChatCommand::ShufflePlayers));
        assert_eq!(parse_command('!', "!synclimit 60"), Some(ChatCommand::SyncLimit(60)));
        assert_eq!(parse_command('!', "!latency 50"), Some(ChatCommand::Latency(50)));
        assert_eq!(parse_command('!', "!hcl -apem"), Some(ChatCommand::Hcl("-apem".into())));
        assert_eq!(parse_command('!', "!draw"), Some(ChatCommand::Draw));
        assert_eq!(parse_command('!', "!votestart"), Some(ChatCommand::VoteStart));
    }

    #[test]
    fn slot_numbers_are_one_based_on_the_wire_and_rejected_when_zero() {
        assert_eq!(parse_command('!', "!close 0"), None);
        assert_eq!(parse_command('!', "!close abc"), None);
    }

    #[tokio::test]
    async fn the_virtual_host_does_not_count_toward_the_minimum_to_start() {
        // One human plus the virtual host must not satisfy the "2 players"
        // rule: the virtual host can never confirm ready, so counting it
        // would let `!start` fire with only one real player present.
        let (mut st, _rxs) = crate::actor::tests_support::seated_game(1);
        st.create_virtual_host();
        assert_eq!(st.players.len(), 2, "virtual host is seated alongside the one human");

        st.run_command(1, "P1", ChatCommand::Start { force: false });

        assert!(
            matches!(st.phase, GamePhase::Lobby),
            "one human plus the virtual host must not be enough to start, got {:?}",
            st.phase
        );
    }
}
