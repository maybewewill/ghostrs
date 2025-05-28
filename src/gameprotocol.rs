use crate::util::*;
use crate::ghost::*;
use crate::gameslot::*;
use crate::bnet::*;
use crate::crc32::*;
use crate::sha1::*; 
use crate::gameplayer::*;
use crate::logger::*;
use std::sync::{Arc, Mutex};

use std::collections::VecDeque;

pub type ByteArray = Vec<u8>;

pub const W3GS_HEADER_CONSTANT: u8 = 247;

pub const GAME_NONE: u8 = 0;
pub const GAME_FULL: u8 = 2;
pub const GAME_PUBLIC: u8 = 16;
pub const GAME_PRIVATE: u8 = 17;

pub const GAMETYPE_CUSTOM: u8 = 1;
pub const GAMETYPE_BLIZZARD: u8 = 9;

pub const PLAYERLEAVE_DISCONNECT: u8 = 1;
pub const PLAYERLEAVE_LOST: u8 = 7;
pub const PLAYERLEAVE_LOSTBUILDINGS: u8 = 8;
pub const PLAYERLEAVE_WON: u8 = 9;
pub const PLAYERLEAVE_DRAW: u8 = 10;
pub const PLAYERLEAVE_OBSERVER: u8 = 11;
pub const PLAYERLEAVE_LOBBY: u8 = 13;
pub const PLAYERLEAVE_GPROXY: u8 = 100;

pub const REJECTJOIN_FULL: u8 = 9;
pub const REJECTJOIN_STARTED: u8 = 10;
pub const REJECTJOIN_WRONGPASSWORD: u8 = 27;

#[allow(non_camel_case_types)]
pub enum ProtocolG {
    W3GS_PING_FROM_HOST = 1,
    W3GS_SLOTINFOJOIN = 4,
    W3GS_REJECTJOIN = 5,
    W3GS_PLAYERINFO = 6,
    W3GS_PLAYERLEAVE_OTHERS = 7,
    W3GS_GAMELOADED_OTHERS = 8,
    W3GS_SLOTINFO = 9,
    W3GS_COUNTDOWN_START = 10,
    W3GS_COUNTDOWN_END = 11,
    W3GS_INCOMING_ACTION = 12,
    W3GS_CHAT_FROM_HOST = 15,
    W3GS_START_LAG = 16,
    W3GS_STOP_LAG = 17,
    W3GS_HOST_KICK_PLAYER = 28,
    W3GS_REQJOIN = 30,
    W3GS_LEAVEGAME = 33,
    W3GS_GAMELOADED_SELF = 35,
    W3GS_OUTGOING_ACTION = 38,
    W3GS_OUTGOING_KEEPALIVE = 39,
    W3GS_CHAT_TO_HOST = 40,
    W3GS_DROPREQ = 41,
    W3GS_SEARCHGAME = 47,
    W3GS_GAMEINFO = 48,
    W3GS_CREATEGAME = 49,
    W3GS_REFRESHGAME = 50,
    W3GS_DECREATEGAME = 51,
    W3GS_CHAT_OTHERS = 52,
    W3GS_PING_FROM_OTHERS = 53,
    W3GS_PONG_TO_OTHERS = 54,
    W3GS_MAPCHECK = 61,
    W3GS_STARTDOWNLOAD = 63,
    W3GS_MAPSIZE = 66,
    W3GS_MAPPART = 67,
    W3GS_MAPPARTOK = 68,
    W3GS_MAPPARTNOTOK = 69,
    W3GS_PONG_TO_HOST = 70,
    W3GS_INCOMING_ACTION2 = 72,
}
#[derive(Clone)]
#[derive(Debug)]
#[derive(Default)]
pub struct GameProtocol {
    ghost: Arc<Mutex<Ghost>>
}

impl GameProtocol {
    pub fn new(_ghost: Arc<Mutex<Ghost>>) -> Self {
        GameProtocol {
            ghost: _ghost
        }
    }

    pub fn RECEIVE_W3GS_REQJOIN(&self, data: ByteArray) -> Option<IncomingJoinPlayer> {
        if self.ValidateLength(&data) && data.len() >= 20 {
            let host_counter = byte_array_to_u32(&data[4..8].to_vec(), false, 0);
            let entry_key = byte_array_to_u32(&data[8..12].to_vec(), false, 0);
            let name = extract_cstring(&data, 19);
            if !name.is_empty() && data.len() >= name.len() + 30 {
                let internal_ip = data[name.len() + 26..name.len() + 30].to_vec();
                return Some(IncomingJoinPlayer::new(host_counter, entry_key, String::from_utf8_lossy(&name).to_string(), internal_ip));
            }
        }
        None
    }

    pub fn RECEIVE_W3GS_LEAVEGAME(&self, data: ByteArray) -> u32 {
        if self.ValidateLength(&data) && data.len() >= 8 {
            return byte_array_to_u32(&data[4..8].to_vec(), false, 0);
        }
        0
    }

    pub fn RECEIVE_W3GS_GAMELOADED_SELF(&self, data: ByteArray) -> bool {
        self.ValidateLength(&data)
    }

    pub fn RECEIVE_W3GS_OUTGOING_ACTION(&self, data: ByteArray, pid: u8) -> Option<CIncomingAction> {
        if pid != 255 && self.ValidateLength(&data) && data.len() >= 8 {
            let crc = data[4..8].to_vec();
            let action = data[8..].to_vec();
            return Some(CIncomingAction::new(pid, crc, action));
        }
        None
    }

    pub fn RECEIVE_W3GS_OUTGOING_KEEPALIVE(&self, data: ByteArray) -> u32 {
        if self.ValidateLength(&data) && data.len() == 9 {
            return byte_array_to_u32(&data[5..9].to_vec(), false, 0);
        }
        0
    }

    pub fn RECEIVE_W3GS_CHAT_TO_HOST(&self, data: ByteArray) -> Option<CIncomingChatPlayer> {
        if self.ValidateLength(&data) {
            let mut i: usize = 5;
            let total = data[4];
            if total > 0 && total <= 12 && data.len() >= i + total as usize {
                let to_pids = data[i..i + total as usize].to_vec();
                i += total as usize;
                let from_pid = data[i];
                let flag = data[i + 1];
                i += 2;
                if flag == 16 && data.len() >= i + 1 {
                    let message = extract_cstring(&data, i);
                    return Some(CIncomingChatPlayer::new_message(from_pid, to_pids, flag, String::from_utf8_lossy(&message).to_string()));
                } else if (17..=20).contains(&flag) && data.len() >= i + 1 {
                    let byte = data[i];
                    return Some(CIncomingChatPlayer::new_request(from_pid, to_pids, flag, byte));
                } else if flag == 32 && data.len() >= i + 5 {
                    let extra_flags = data[i..i + 4].to_vec();
                    let message = extract_cstring(&data, i + 4);
                    return Some(CIncomingChatPlayer::new_message_extra(from_pid, to_pids, flag, String::from_utf8_lossy(&message).to_string(), extra_flags));
                }
            }
        }
        None
    }

    pub fn RECEIVE_W3GS_SEARCHGAME(&self, data: ByteArray, war3_version: u8) -> bool {
        let product_id: u32 = 1462982736;
        let version = war3_version as u32;
        if self.ValidateLength(&data) && data.len() >= 16 {
            if byte_array_to_u32(&data[4..8].to_vec(), false, 0) == product_id {
                if byte_array_to_u32(&data[8..12].to_vec(), false, 0) == version {
                    if byte_array_to_u32(&data[12..16].to_vec(), false, 0) == 0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn RECEIVE_W3GS_MAPSIZE(&self, data: ByteArray, _map_size: ByteArray) -> Option<CIncomingMapSize> {
        if self.ValidateLength(&data) && data.len() >= 13 {
            return Some(CIncomingMapSize::new(data[8], byte_array_to_u32(&data[9..13].to_vec(), false, 0)));
        }
        None
    }

    pub fn RECEIVE_W3GS_MAPPARTOK(&self, data: ByteArray) -> u32 {
        if self.ValidateLength(&data) && data.len() >= 14 {
            return byte_array_to_u32(&data[10..14].to_vec(), false, 0);
        }
        0
    }

    pub fn RECEIVE_W3GS_PONG_TO_HOST(&self, data: ByteArray) -> u32 {
        if self.ValidateLength(&data) && data.len() >= 8 {
            return byte_array_to_u32(&data[4..8].to_vec(), false, 0);
        }
        1
    }

    pub fn SEND_W3GS_PING_FROM_HOST(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_PING_FROM_HOST as u8);
        packet.push(0);
        packet.push(0);
        append_byte_array_from_u32(&mut packet, get_ticks() as u32, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_SLOTINFOJOIN(&self, pid: u8, port: ByteArray, external_ip: ByteArray, slots: Vec<GameSlot>, random_seed: u32, layout_style: u8, player_slots: u8) -> ByteArray {
        let zeros = [0, 0, 0, 0];
        let slot_info = self.EncodeSlotInfo(slots, random_seed, layout_style, player_slots);
        let mut packet = ByteArray::new();
        if port.len() == 2 && external_ip.len() == 4 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_SLOTINFOJOIN as u8);
            packet.push(0);
            packet.push(0);
            append_byte_array_from_u16(&mut packet, slot_info.len() as u16, false);
            packet.extend_from_slice(&slot_info);
            packet.push(pid);
            packet.push(2);
            packet.push(0);
            packet.extend_from_slice(&port);
            packet.extend_from_slice(&external_ip);
            packet.extend_from_slice(&zeros);
            packet.extend_from_slice(&zeros);
            self.AssignLength(&mut packet);
        }
        for byte in &packet {
            print!("{:02x} ", byte);
        }
        packet
    }

    pub fn SEND_W3GS_REJECTJOIN(&self, reason: u32) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_REJECTJOIN as u8);
        packet.push(0);
        packet.push(0);
        append_byte_array_from_u32(&mut packet, reason, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_PLAYERINFO(&self, pid: u8, name: String, external_ip: ByteArray, internal_ip: ByteArray) -> ByteArray {
        let player_join_counter = [2, 0, 0, 0];
        let zeros = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        if !name.is_empty() && name.len() <= 15 && external_ip.len() == 4 && internal_ip.len() == 4 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_PLAYERINFO as u8);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&player_join_counter);
            packet.push(pid);
            append_byte_array_fast_from_string(&mut packet, &name, true);
            packet.push(1);
            packet.push(0);
            packet.push(2);
            packet.push(0);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&external_ip);
            packet.extend_from_slice(&zeros);
            packet.extend_from_slice(&zeros);
            packet.push(2);
            packet.push(0);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&internal_ip);
            packet.extend_from_slice(&zeros);
            packet.extend_from_slice(&zeros);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_PLAYERLEAVE_OTHERS(&self, pid: u8, left_code: u32) -> ByteArray {
        let mut packet = ByteArray::new();
        if pid != 255 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_PLAYERLEAVE_OTHERS as u8);
            packet.push(0);
            packet.push(0);
            packet.push(pid);
            append_byte_array_from_u32(&mut packet, left_code, false);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_GAMELOADED_OTHERS(&self, pid: u8) -> ByteArray {
        let mut packet = ByteArray::new();
        if pid != 255 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_GAMELOADED_OTHERS as u8);
            packet.push(0);
            packet.push(0);
            packet.push(pid);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_SLOTINFO(&self, slots: &Vec<GameSlot>, random_seed: u32, layout_style: u8, player_slots: u8) -> ByteArray {
        let slot_info = self.EncodeSlotInfo(slots.to_vec(), random_seed, layout_style, player_slots);
        let mut packet = ByteArray::new();
        // println!();
        // println!("SLOT_INFO: {:x?}", slot_info);
        // println!("Layout style: {}, Player slots: {}, Slots: {:?}", layout_style, player_slots, slots);
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_SLOTINFO as u8);
        packet.push(0);
        packet.push(0);
        append_byte_array_from_u16(&mut packet, slot_info.len() as u16, false);
        packet.extend_from_slice(&slot_info);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_COUNTDOWN_START(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_COUNTDOWN_START as u8);
        packet.push(0);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_COUNTDOWN_END(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_COUNTDOWN_END as u8);
        packet.push(0);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_INCOMING_ACTION(&self, actions: VecDeque<CIncomingAction>, send_interval: u16) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_INCOMING_ACTION as u8);
        packet.push(0);
        packet.push(0);
        append_byte_array_from_u16(&mut packet, send_interval, false);
        let mut subpacket = ByteArray::new();
        for action in actions {
            subpacket.push(action.get_pid());
            append_byte_array_from_u16(&mut packet, action.get_action().len() as u16, false);
            packet.extend_from_slice(&action.get_action());
        }
        let ghost = self.ghost.lock().unwrap();
        let crc32 = create_byte_array_from_u32(ghost.m_CRC.full_crc(&subpacket, subpacket.len().try_into().unwrap()), false);
        packet.extend_from_slice(&crc32[..2]);
        packet.extend_from_slice(&subpacket);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_CHAT_FROM_HOST(&self, from_pid: u8, to_pids: ByteArray, flag: u8, flag_extra: ByteArray, message: String) -> ByteArray {
        let mut packet = ByteArray::new();
        if !to_pids.is_empty() && !message.is_empty() && message.len() < 255 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_CHAT_FROM_HOST as u8);
            packet.push(0);
            packet.push(0);
            packet.push(to_pids.len() as u8);
            packet.extend_from_slice(&to_pids);
            packet.push(from_pid);
            packet.push(flag);
            packet.extend_from_slice(&flag_extra);
            append_byte_array_fast_from_string(&mut packet, &message, true);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_START_LAG(&self, players: Vec<GamePlayer>, load_in_game: bool) -> ByteArray {
        let mut packet = ByteArray::new();
        let mut num_laggers = 0;
        for player in &players {
            if load_in_game {
                if !player.get_finished_loading() {
                    num_laggers += 1;
                }
            } else {
                if player.get_lagging() {
                    num_laggers += 1;
                }
            }
        }
        if num_laggers > 0 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_START_LAG as u8);
            packet.push(0);
            packet.push(0);
            packet.push(num_laggers);
            for player in players {
                if load_in_game {
                    if !player.get_finished_loading() {
                        packet.push(player.get_pid());
                        append_byte_array_from_u32(&mut packet, 0, false);
                    }
                } else {
                    if player.get_lagging() {
                        packet.push(player.get_pid());
                        append_byte_array_from_u32(&mut packet, (get_ticks() - player.get_started_lagging_ticks() as u128) as u32, false);
                    }
                }
            }
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_STOP_LAG(&self, player: &GamePlayer, load_in_game: bool) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_STOP_LAG as u8);
        packet.push(0);
        packet.push(0);
        packet.push(player.get_pid());
        if load_in_game {
            append_byte_array_from_u32(&mut packet, 0, false);
        } else {
            append_byte_array_from_u32(&mut packet, (get_ticks() as u32).wrapping_sub(player.get_started_lagging_ticks()), false);
        }
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_SEARCHGAME(&self, tft: bool, war3_version: u8) -> ByteArray {
        let product_id_roc = [51, 82, 65, 87];
        let product_id_tft = [80, 88, 51, 87];
        let version = [war3_version, 0, 0, 0];
        let unknown = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_SEARCHGAME as u8);
        packet.push(0);
        packet.push(0);
        if tft {
            packet.extend_from_slice(&product_id_tft);
        } else {
            packet.extend_from_slice(&product_id_roc);
        }
        packet.extend_from_slice(&version);
        packet.extend_from_slice(&unknown);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_GAMEINFO(&self, tft: bool, war3_version: u8, map_game_type: ByteArray, map_flags: ByteArray, map_width: ByteArray, map_height: ByteArray, game_name: String, host_name: String, up_time: u32, map_path: String, map_crc: ByteArray, slots_total: u32, slots_open: u32, port: u16, host_counter: u32, entry_key: u32) -> ByteArray {
        let product_id_roc = [51, 82, 65, 87];
        let product_id_tft = [80, 88, 51, 87];
        let version = [war3_version, 0, 0, 0];
        let unknown2 = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        if map_game_type.len() == 4 && map_flags.len() == 4 && map_width.len() == 2 && map_height.len() == 2 && !game_name.is_empty() && !host_name.is_empty() && !map_path.is_empty() && map_crc.len() == 4 {
            let mut stat_string = ByteArray::new();
            stat_string.extend_from_slice(&map_flags);
            stat_string.push(0);
            stat_string.extend_from_slice(&map_width);
            stat_string.extend_from_slice(&map_height);
            stat_string.extend_from_slice(&map_crc);
            append_byte_array_fast_from_string(&mut stat_string, &map_path, true);
            append_byte_array_fast_from_string(&mut stat_string, &host_name, true);
            stat_string.push(0);
            let stat_string = encode_stat_string(&stat_string);
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_GAMEINFO as u8);
            packet.push(0);
            packet.push(0);
            if tft {
                packet.extend_from_slice(&product_id_tft);
            } else {
                packet.extend_from_slice(&product_id_roc);
            }
            packet.extend_from_slice(&version);
            append_byte_array_from_u32(&mut packet, host_counter, false);
            append_byte_array_from_u32(&mut packet, entry_key, false);
            append_byte_array_fast_from_string(&mut packet, &game_name, true);
            packet.push(0);
            packet.extend_from_slice(&stat_string);
            packet.push(0);
            append_byte_array_from_u32(&mut packet, slots_total, false);
            packet.extend_from_slice(&map_game_type);
            packet.extend_from_slice(&unknown2);
            append_byte_array_from_u32(&mut packet, slots_open, false);
            append_byte_array_from_u32(&mut packet, up_time, false);
            append_byte_array_from_u16(&mut packet, port, false);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_CREATEGAME(&self, tft: bool, war3_version: u8) -> ByteArray {
        let product_id_roc = [51, 82, 65, 87];
        let product_id_tft = [80, 88, 51, 87];
        let version = [war3_version, 0, 0, 0];
        let host_counter = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_CREATEGAME as u8);
        packet.push(0);
        packet.push(0);
        if tft {
            packet.extend_from_slice(&product_id_tft);
        } else {
            packet.extend_from_slice(&product_id_roc);
        }
        packet.extend_from_slice(&version);
        packet.extend_from_slice(&host_counter);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_REFRESHGAME(&self, players: u32, player_slots: u32) -> ByteArray {
        let host_counter = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_REFRESHGAME as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&host_counter);
        append_byte_array_from_u32(&mut packet, players, false);
        append_byte_array_from_u32(&mut packet, player_slots, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_DECREATEGAME(&self) -> ByteArray {
        let host_counter = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_DECREATEGAME as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&host_counter);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_MAPCHECK(&self, map_path: String, map_size: ByteArray, map_info: ByteArray, map_crc: ByteArray, map_sha1: ByteArray) -> ByteArray {
        let unknown = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        if !map_path.is_empty() && map_size.len() == 4 && map_info.len() == 4 && map_crc.len() == 4 && map_sha1.len() == 20 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_MAPCHECK as u8);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&unknown);
            append_byte_array_fast_from_string(&mut packet, &map_path, true);
            packet.extend_from_slice(&map_size);
            packet.extend_from_slice(&map_info);
            packet.extend_from_slice(&map_crc);
            packet.extend_from_slice(&map_sha1);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_STARTDOWNLOAD(&self, from_pid: u8) -> ByteArray {
        let unknown = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_STARTDOWNLOAD as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&unknown);
        packet.push(from_pid);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_W3GS_MAPPART(&self, from_pid: u8, to_pid: u8, start: u32, map_data: &str) -> ByteArray {
        let unknown = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        if start < map_data.len() as u32 {
            packet.push(W3GS_HEADER_CONSTANT);
            packet.push(ProtocolG::W3GS_MAPPART as u8);
            packet.push(0);
            packet.push(0);
            packet.push(to_pid);
            packet.push(from_pid);
            packet.extend_from_slice(&unknown);
            append_byte_array_from_u32(&mut packet, start, false);
            let end = std::cmp::min(start + 1442, map_data.len() as u32);
            let ghost = self.ghost.lock().unwrap();
            let crc32 = create_byte_array_from_u32(ghost.m_CRC.full_crc(&map_data[start as usize..end as usize].as_bytes().to_vec(), end-start), false);
            packet.extend_from_slice(&crc32);
            packet.extend_from_slice(&map_data[start as usize..end as usize].as_bytes());
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_W3GS_INCOMING_ACTION2(&self, actions: VecDeque<CIncomingAction>) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(W3GS_HEADER_CONSTANT);
        packet.push(ProtocolG::W3GS_INCOMING_ACTION2 as u8);
        packet.push(0);
        packet.push(0);
        packet.push(0);
        packet.push(0);
        let mut subpacket = ByteArray::new();
        for action in actions {
            subpacket.push(action.get_pid());
            append_byte_array_from_u16(&mut packet, action.get_action().len() as u16, false);
            packet.extend_from_slice(&action.get_action());
        }
        let ghost = self.ghost.lock().unwrap();
        let crc32 = create_byte_array_from_u32(ghost.m_CRC.full_crc(&subpacket, subpacket.len().try_into().unwrap()), false);
        packet.extend_from_slice(&crc32[..2]);
        packet.extend_from_slice(&subpacket);
        self.AssignLength(&mut packet);
        packet
    }

    fn AssignLength(&self, content: &mut ByteArray) -> bool {
        if content.len() >= 4 && content.len() <= 65535 {
            let length_bytes = create_byte_array_from_u16(content.len() as u16, false);
            content[2] = length_bytes[0];
            content[3] = length_bytes[1];
            return true;
        }
        false
    }

    fn ValidateLength(&self, content: &ByteArray) -> bool {
        if content.len() >= 4 && content.len() <= 65535 {
            let length = byte_array_to_u16(&content[2..4].to_vec(), false, 0);
            return length == content.len() as u16;
        }
        false
    }

    fn EncodeSlotInfo(&self, slots: Vec<GameSlot>, random_seed: u32, layout_style: u8, player_slots: u8) -> ByteArray {
        let mut slot_info = ByteArray::new();
        slot_info.push(slots.len() as u8);
        for slot in slots {
            slot_info.extend_from_slice(&slot.to_bytes());
        }
        append_byte_array_from_u32(&mut slot_info, random_seed, false);
        slot_info.push(layout_style);
        slot_info.push(player_slots);
        slot_info
    }
}

#[derive(Clone, Debug)]
#[derive(Default)]
pub struct IncomingJoinPlayer {
    m_host_counter: u32,
    m_entry_key: u32,
    m_name: String,
    m_internal_ip: ByteArray,
}

impl IncomingJoinPlayer {
    pub fn new(n_host_counter: u32, n_entry_key: u32, n_name: String, n_internal_ip: ByteArray) -> Self {
        IncomingJoinPlayer {
            m_host_counter: n_host_counter,
            m_entry_key: n_entry_key,
            m_name: n_name,
            m_internal_ip: n_internal_ip,
        }
    }

    pub fn get_host_counter(&self) -> u32 { self.m_host_counter }
    pub fn get_entry_key(&self) -> u32 { self.m_entry_key }
    pub fn get_name(&self) -> String { self.m_name.clone() }
    pub fn get_internal_ip(&self) -> ByteArray { self.m_internal_ip.clone() }
}

#[derive(Debug, Clone)]
pub struct CIncomingAction {
    m_pid: u8,
    m_crc: ByteArray,
    m_action: ByteArray,
}

impl CIncomingAction {
    pub fn new(n_pid: u8, n_crc: ByteArray, n_action: ByteArray) -> Self {
        CIncomingAction {
            m_pid: n_pid,
            m_crc: n_crc,
            m_action: n_action,
        }
    }

    pub fn get_pid(&self) -> u8 { self.m_pid }
    pub fn get_crc(&self) -> ByteArray { self.m_crc.clone() }
    pub fn get_action(&self) -> ByteArray { self.m_action.clone() }
    pub fn get_length(&self) -> u32 { self.m_action.len() as u32 + 3 }
}
#[derive(Debug)]
pub struct CIncomingChatPlayer {
    m_type: ChatToHostType,
    m_from_pid: u8,
    m_to_pids: ByteArray,
    m_flag: u8,
    m_message: String,
    m_byte: u8,
    m_extra_flags: ByteArray,
}
#[derive(Debug)]
#[derive(PartialEq, Copy, Clone)]
pub enum ChatToHostType {
    CTH_MESSAGE = 0,
    CTH_MESSAGEEXTRA = 1,
    CTH_TEAMCHANGE = 2,
    CTH_COLOURCHANGE = 3,
    CTH_RACECHANGE = 4,
    CTH_HANDICAPCHANGE = 5,
}

impl CIncomingChatPlayer {
    pub fn new_message(n_from_pid: u8, n_to_pids: ByteArray, n_flag: u8, n_message: String) -> Self {
        CIncomingChatPlayer {
            m_type: ChatToHostType::CTH_MESSAGE,
            m_from_pid: n_from_pid,
            m_to_pids: n_to_pids,
            m_flag: n_flag,
            m_message: n_message,
            m_byte: 0,
            m_extra_flags: ByteArray::new(),
        }
    }

    pub fn new_message_extra(n_from_pid: u8, n_to_pids: ByteArray, n_flag: u8, n_message: String, n_extra_flags: ByteArray) -> Self {
        CIncomingChatPlayer {
            m_type: ChatToHostType::CTH_MESSAGEEXTRA,
            m_from_pid: n_from_pid,
            m_to_pids: n_to_pids,
            m_flag: n_flag,
            m_message: n_message,
            m_byte: 0,
            m_extra_flags: n_extra_flags,
        }
    }

    pub fn new_request(n_from_pid: u8, n_to_pids: ByteArray, n_flag: u8, n_byte: u8) -> Self {
        let m_type = match n_flag {
            17 => ChatToHostType::CTH_TEAMCHANGE,
            18 => ChatToHostType::CTH_COLOURCHANGE,
            19 => ChatToHostType::CTH_RACECHANGE,
            20 => ChatToHostType::CTH_HANDICAPCHANGE,
            _ => ChatToHostType::CTH_MESSAGE,
        };
        CIncomingChatPlayer {
            m_type,
            m_from_pid: n_from_pid,
            m_to_pids: n_to_pids,
            m_flag: n_flag,
            m_message: String::new(),
            m_byte: n_byte,
            m_extra_flags: ByteArray::new(),
        }
    }

    pub fn get_type(&self) -> ChatToHostType { self.m_type }
    pub fn get_from_pid(&self) -> u8 { self.m_from_pid }
    pub fn get_to_pids(&self) -> ByteArray { self.m_to_pids.clone() }
    pub fn get_flag(&self) -> u8 { self.m_flag }
    pub fn get_message(&self) -> String { self.m_message.clone() }
    pub fn get_byte(&self) -> u8 { self.m_byte }
    pub fn get_extra_flags(&self) -> ByteArray { self.m_extra_flags.clone() }
}

pub struct CIncomingMapSize {
    m_size_flag: u8,
    m_map_size: u32,
}

impl CIncomingMapSize {
    pub fn new(n_size_flag: u8, n_map_size: u32) -> Self {
        CIncomingMapSize {
            m_size_flag: n_size_flag,
            m_map_size: n_map_size,
        }
    }

    pub fn get_size_flag(&self) -> u8 { self.m_size_flag }
    pub fn get_map_size(&self) -> u32 { self.m_map_size }
}
