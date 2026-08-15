use bytes::Bytes;
use ghost_protocol::w3gs::incoming::ChatToHost;

use crate::lang;
use crate::players::NameMatch;
use crate::state::{GamePhase, GameState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatCommand {
    Start,
    Abort,
    /// Slot ids are zero-based here; the chat syntax is one-based.
    Open(u8),
    Close(u8),
    Swap(u8, u8),
    Kick(String),
    Ping,
    Unhost,
    Say(String),
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
        "start" => ChatCommand::Start,
        "abort" => ChatCommand::Abort,
        "ping" => ChatCommand::Ping,
        "unhost" => ChatCommand::Unhost,
        "open" => ChatCommand::Open(slot_arg(args.first()?)?),
        "close" => ChatCommand::Close(slot_arg(args.first()?)?),
        "swap" => ChatCommand::Swap(slot_arg(args.first()?)?, slot_arg(args.get(1)?)?),
        "kick" => ChatCommand::Kick(args.first()?.to_string()),
        "say" => ChatCommand::Say(args.join(" ")),
        other => ChatCommand::Unknown(other.to_string()),
    })
}

impl GameState {
    pub fn handle_chat_to_host(&mut self, conn_id: u64, payload: &Bytes) {
        let Some((pid, name)) = self
            .players
            .by_conn(conn_id)
            .map(|p| (p.pid, p.name.clone()))
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
        match parse_command('!', &chat.message) {
            Some(cmd) => {
                if !is_owner {
                    self.send_chat_to(pid, &lang::command_not_allowed());
                    return;
                }
                self.run_command(pid, cmd);
            }
            // Not a command: relay it to the recipients the client picked.
            None => {
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

    fn run_command(&mut self, pid: u8, cmd: ChatCommand) {
        match cmd {
            ChatCommand::Start => {
                if self.players.len() < 2 {
                    let msg = lang::unable_to_start_not_enough(self.players.len());
                    self.send_chat_to(pid, &msg);
                } else {
                    let by = self.cfg.owner.clone();
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
            ChatCommand::Ping => {
                let pairs: Vec<(String, Option<u32>)> = self
                    .players
                    .iter()
                    .map(|p| (p.name.clone(), p.average_ping()))
                    .collect();
                let msg = lang::player_pings(&pairs);
                self.send_chat_to(pid, &msg);
            }
            ChatCommand::Unhost => {
                if matches!(self.phase, GamePhase::Lobby) {
                    self.finished = true;
                }
            }
            ChatCommand::Say(msg) => self.send_chat_all(&msg),
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
    fn parses_commands_with_and_without_arguments() {
        assert_eq!(parse_command('!', "!start"), Some(ChatCommand::Start));
        assert_eq!(parse_command('!', "!close 3"), Some(ChatCommand::Close(2)));
        assert_eq!(parse_command('!', "!open 1"), Some(ChatCommand::Open(0)));
        assert_eq!(parse_command('!', "!swap 1 4"), Some(ChatCommand::Swap(0, 3)));
        assert_eq!(parse_command('!', "!kick Slash"), Some(ChatCommand::Kick("Slash".into())));
        assert_eq!(parse_command('!', "!ping"), Some(ChatCommand::Ping));
    }

    #[test]
    fn slot_numbers_are_one_based_on_the_wire_and_rejected_when_zero() {
        assert_eq!(parse_command('!', "!close 0"), None);
        assert_eq!(parse_command('!', "!close abc"), None);
    }

    #[test]
    fn plain_chat_is_not_a_command() {
        assert_eq!(parse_command('!', "hello"), None);
        assert_eq!(parse_command('!', ""), None);
        assert_eq!(parse_command('!', "!"), None);
    }

    #[test]
    fn the_trigger_character_is_configurable() {
        assert_eq!(parse_command('.', ".start"), Some(ChatCommand::Start));
        assert_eq!(parse_command('.', "!start"), None);
    }

    #[test]
    fn commands_are_case_insensitive() {
        assert_eq!(parse_command('!', "!START"), Some(ChatCommand::Start));
    }
}
