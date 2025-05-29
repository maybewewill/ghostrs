use crate::util::*;

pub type ByteArray = Vec<u8>;

const BNET_HEADER_CONSTANT: u8 = 255;

#[allow(non_camel_case_types)]
pub enum Protocol {
    SID_NULL = 0,
    SID_STOPADV = 2,
    SID_GETADVLISTEX = 9,
    SID_ENTERCHAT = 10,
    SID_JOINCHANNEL = 12,
    SID_CHATCOMMAND = 14,
    SID_CHATEVENT = 15,
    SID_CHECKAD = 21,
    SID_STARTADVEX3 = 28,
    SID_DISPLAYAD = 33,
    SID_NOTIFYJOIN = 34,
    SID_PING = 37,
    SID_LOGONRESPONSE = 41,
    SID_NETGAMEPORT = 69,
    SID_AUTH_INFO = 80,
    SID_AUTH_CHECK = 81,
    SID_AUTH_ACCOUNTLOGON = 83,
    SID_AUTH_ACCOUNTLOGONPROOF = 84,
    SID_WARDEN = 94,
    SID_FRIENDSLIST = 101,
    SID_FRIENDSUPDATE = 102,
    SID_CLANCREATIONINVITATION = 114,
    SID_CLANINVITATION = 119,
    SID_CLANREMOVEMEMBER = 120,
    SID_CLANINVITATIONRESPONSE = 121,
    SID_CLANCHANGERANK = 122,
    SID_CLANSETMOTD = 123,
    SID_CLANMEMBERLIST = 125,
    SID_CLANMEMBERSTATUSCHANGE = 127,
}

#[allow(non_camel_case_types)]
pub enum KeyResult {
    KR_GOOD = 0,
    KR_OLD_GAME_VERSION = 256,
    KR_INVALID_VERSION = 257,
    KR_ROC_KEY_IN_USE = 513,
    KR_TFT_KEY_IN_USE = 529,
}

#[derive(Copy, Clone)]
#[derive(PartialEq)]
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum IncomingChatEventEnum {
    EID_SHOWUSER = 1,
    EID_JOIN = 2,
    EID_LEAVE = 3,
    EID_WHISPER = 4,
    EID_TALK = 5,
    EID_BROADCAST = 6,
    EID_CHANNEL = 7,
    EID_USERFLAGS = 9,
    EID_WHISPERSENT = 10,
    EID_CHANNELFULL = 13,
    EID_CHANNELDOESNOTEXIST = 14,
    EID_CHANNELRESTRICTED = 15,
    EID_INFO = 18,
    EID_ERROR = 19,
    EID_EMOTE = 23,
}

#[repr(u8)]
#[allow(non_camel_case_types)]
pub enum RankCode {
    CLAN_INITIATE = 0,
    CLAN_PARTIAL_MEMBER = 1,
    CLAN_MEMBER = 2,
    CLAN_OFFICER = 3,
    CLAN_LEADER = 4,
}
#[derive(Debug, Clone)]
pub struct BnetProtocol {
    m_ClientToken: ByteArray,
    m_LogonType: ByteArray,
    m_ServerToken: ByteArray,
    m_MPQFileTime: ByteArray,
    m_IX86VerFileName: ByteArray,
    m_ValueStringFormula: ByteArray,
    m_KeyState: ByteArray,
    m_KeyStateDescription: ByteArray,
    m_Salt: ByteArray,
    m_ServerPublicKey: ByteArray,
    m_UniqueName: ByteArray,
    m_ClanLastInviteTag: ByteArray,
    m_ClanLastInviteName: ByteArray,
}

impl BnetProtocol {
    pub fn new() -> Self {
        let client_token = vec![220, 1, 203, 7];
        BnetProtocol {
            m_ClientToken: client_token,
            m_LogonType: Vec::new(),
            m_ServerToken: Vec::new(),
            m_MPQFileTime: Vec::new(),
            m_IX86VerFileName: Vec::new(),
            m_ValueStringFormula: Vec::new(),
            m_KeyState: Vec::new(),
            m_KeyStateDescription: Vec::new(),
            m_Salt: Vec::new(),
            m_ServerPublicKey: Vec::new(),
            m_UniqueName: Vec::new(),
            m_ClanLastInviteTag: Vec::new(),
            m_ClanLastInviteName: Vec::new(),
        }
    }

    pub fn GetClientToken(&self) -> ByteArray { self.m_ClientToken.clone() }
    pub fn GetLogonType(&self) -> ByteArray { self.m_LogonType.clone() }
    pub fn GetServerToken(&self) -> ByteArray { self.m_ServerToken.clone() }
    pub fn GetMPQFileTime(&self) -> ByteArray { self.m_MPQFileTime.clone() }
    pub fn GetIX86VerFileName(&self) -> ByteArray { self.m_IX86VerFileName.clone() }
    pub fn GetIX86VerFileNameString(&self) -> String { String::from_utf8_lossy(&self.m_IX86VerFileName).to_string() }
    pub fn GetValueStringFormula(&self) -> ByteArray { self.m_ValueStringFormula.clone() }
    pub fn GetValueStringFormulaString(&self) -> String { String::from_utf8_lossy(&self.m_ValueStringFormula).to_string() }
    pub fn GetKeyState(&self) -> ByteArray { self.m_KeyState.clone() }
    pub fn GetKeyStateDescription(&self) -> String { String::from_utf8_lossy(&self.m_KeyStateDescription).to_string() }
    pub fn GetSalt(&self) -> ByteArray { self.m_Salt.clone() }
    pub fn GetServerPublicKey(&self) -> ByteArray { self.m_ServerPublicKey.clone() }
    pub fn GetUniqueName(&self) -> ByteArray { self.m_UniqueName.clone() }

    pub fn RECEIVE_SID_NULL(&self, data: ByteArray) -> bool {
        self.ValidateLength(&data)
    }

    pub fn RECEIVE_SID_GETADVLISTEX(&self, data: ByteArray) -> Option<IncomingGameHost> {
        if self.ValidateLength(&data) && data.len() >= 8 {
            let games_found = byte_array_to_u32(&data[4..8].to_vec(), false, 0);
            if games_found > 0 && data.len() >= 25 {
                let port = byte_array_to_u16(&data[18..20].to_vec(), false,0);
                let ip = data[20..24].to_vec();
                let game_name = extract_cstring(&data, 24);
                if data.len() >= game_name.len() + 35 {
                    let mut host_counter = Vec::new();
                    host_counter.push(extract_hex(&data, game_name.len() + 27, true));
                    host_counter.push(extract_hex(&data, game_name.len() + 29, true));
                    host_counter.push(extract_hex(&data, game_name.len() + 31, true));
                    host_counter.push(extract_hex(&data, game_name.len() + 33, true));
                    return Some(IncomingGameHost::new(
                        ip,
                        port,
                        String::from_utf8_lossy(&game_name).to_string(),
                        host_counter,
                    ));
                }
            }
        }
        None
    }

    pub fn RECEIVE_SID_ENTERCHAT(&mut self, data: ByteArray) -> bool {
        if self.ValidateLength(&data) && data.len() >= 5 {
            self.m_UniqueName = extract_cstring(&data, 4);
            return true;
        }
        false
    }

    pub fn RECEIVE_SID_CHATEVENT(&self, data: ByteArray) -> Option<IncomingChatEvent> {
        if self.ValidateLength(&data) && data.len() >= 29 {
            let event_id = byte_array_to_u32(&data[4..8].to_vec(), false, 0);
            let ping = byte_array_to_u32(&data[12..16].to_vec(), false, 0);
            let user = extract_cstring(&data, 28);
            let message = extract_cstring(&data, user.len() + 29);
            match event_id {
                1 | 2 | 3 | 4 | 5 | 6 | 7 | 9 | 10 | 13 | 14 | 15 | 18 | 19 | 23 => {
                    return Some(IncomingChatEvent::new(
                        unsafe { std::mem::transmute(event_id as u8) },
                        ping as i32,
                        String::from_utf8_lossy(&user).to_string(),
                        String::from_utf8_lossy(&message).to_string(),
                    ));
                }
                _ => {}
            }
        }
        None
    }

    pub fn RECEIVE_SID_CHECKAD(&self, data: ByteArray) -> bool {
        self.ValidateLength(&data)
    }

    pub fn RECEIVE_SID_STARTADVEX3(&self, data: ByteArray) -> bool {
        if self.ValidateLength(&data) && data.len() >= 8 {
            let status = byte_array_to_u32(&data[4..8].to_vec(), false, 0);
            return status == 0;
        }
        false
    }

    pub fn RECEIVE_SID_PING(&self, data: ByteArray) -> ByteArray {
        if self.ValidateLength(&data) && data.len() >= 8 {
            return data[4..8].to_vec();
        }
        ByteArray::new()
    }

    pub fn RECEIVE_SID_LOGONRESPONSE(&self, data: ByteArray) -> bool {
        if self.ValidateLength(&data) && data.len() >= 8 {
            let status = byte_array_to_u32(&data[4..8].to_vec(), false, 0);
            return status == 1;
        }
        false
    }

    pub fn RECEIVE_SID_AUTH_INFO(&mut self, data: ByteArray) -> bool {
        if self.ValidateLength(&data) && data.len() >= 25 {
            self.m_LogonType = data[4..8].to_vec();
            self.m_ServerToken = data[8..12].to_vec();
            self.m_MPQFileTime = data[16..24].to_vec();
            self.m_IX86VerFileName = extract_cstring(&data, 24);
            self.m_ValueStringFormula = extract_cstring(&data, self.m_IX86VerFileName.len() + 25);
            return true;
        }
        false
    }

    pub fn RECEIVE_SID_AUTH_CHECK(&mut self, data: ByteArray) -> bool {
        if self.ValidateLength(&data) && data.len() >= 9 {
            self.m_KeyState = data[4..8].to_vec();
            self.m_KeyStateDescription = extract_cstring(&data, 8);
            return byte_array_to_u32(&self.m_KeyState, false, 0) == KeyResult::KR_GOOD as u32;
        }
        false
    }

    pub fn RECEIVE_SID_AUTH_ACCOUNTLOGON(&mut self, data: ByteArray) -> bool {
        if self.ValidateLength(&data) && data.len() >= 8 {
            let status = byte_array_to_u32(&data[4..8].to_vec(), false, 0);
            if status == 0 && data.len() >= 72 {
                self.m_Salt = data[8..40].to_vec();
                self.m_ServerPublicKey = data[40..72].to_vec();
                return true;
            }
        }
        false
    }

    pub fn RECEIVE_SID_AUTH_ACCOUNTLOGONPROOF(&mut self, data: ByteArray) -> bool {
        if self.ValidateLength(&data) && data.len() >= 8 {
            let status = byte_array_to_u32(&data[4..8].to_vec(), false, 0);
            return status == 0 || status == 0xE;
        }
        false
    }

    pub fn RECEIVE_SID_WARDEN(&self, data: ByteArray) -> ByteArray {
        if self.ValidateLength(&data) && data.len() >= 4 {
            return data[4..].to_vec();
        }
        ByteArray::new()
    }

    pub fn RECEIVE_SID_FRIENDSLIST(&self, data: ByteArray) -> Vec<IncomingFriendList> {
        let mut friends = Vec::new();
        if self.ValidateLength(&data) && data.len() >= 5 {
            let mut i = 5;
            let mut total = data[4];
            while total > 0 {
                total -= 1;
                if data.len() < i + 1 {
                    break;
                }
                let account = extract_cstring(&data, i);
                i += account.len() + 1;
                if data.len() < i + 7 {
                    break;
                }
                let status = data[i];
                let area = data[i + 1];
                i += 6;
                let location = extract_cstring(&data, i);
                i += location.len() + 1;
                friends.push(IncomingFriendList::new(
                    String::from_utf8_lossy(&account).to_string(),
                    status,
                    area,
                    String::from_utf8_lossy(&location).to_string(),
                ));
            }
        }
        friends
    }

    pub fn RECEIVE_SID_CLANMEMBERLIST(&self, data: ByteArray) -> Vec<IncomingClanList> {
        let mut clan_list = Vec::new();
        if self.ValidateLength(&data) && data.len() >= 9 {
            let mut i = 9;
            let mut total = data[8];
            while total > 0 {
                total -= 1;
                if data.len() < i + 1 {
                    break;
                }
                let name = extract_cstring(&data, i);
                i += name.len() + 1;
                if data.len() < i + 3 {
                    break;
                }
                let rank = data[i];
                let status = data[i + 1];
                i += 2;
                let _location = extract_cstring(&data, i);
                i += _location.len() + 1;
                clan_list.push(IncomingClanList::new(
                    String::from_utf8_lossy(&name).to_string(),
                    rank,
                    status,
                ));
            }
        }
        clan_list
    }

    pub fn RECEIVE_SID_CLANMEMBERSTATUSCHANGE(&self, data: ByteArray) -> Option<IncomingClanList> {
        if self.ValidateLength(&data) && data.len() >= 5 {
            let name = extract_cstring(&data, 4);
            if data.len() >= name.len() + 7 {
                let rank = data[name.len() + 5];
                let status = data[name.len() + 6];
                let _location = extract_cstring(&data, name.len() + 7);
                return Some(IncomingClanList::new(
                    String::from_utf8_lossy(&name).to_string(),
                    rank,
                    status,
                ));
            }
        }
        None
    }

    pub fn RECEIVE_SID_CLANCREATIONINVITATION(&mut self, data: ByteArray) -> String {
        if self.ValidateLength(&data) && data.len() >= 12 {
            self.m_ClanLastInviteTag = data[8..12].to_vec();
            let clan_name = extract_cstring(&data, 12);
            self.m_ClanLastInviteName = extract_cstring(&data, 12 + clan_name.len());
            return String::from_utf8_lossy(&self.m_ClanLastInviteName).to_string();
        }
        String::new()
    }

    pub fn RECEIVE_SID_CLANINVITATIONRESPONSE(&mut self, data: ByteArray) -> String {
        if self.ValidateLength(&data) && data.len() >= 12 {
            self.m_ClanLastInviteTag = data[8..12].to_vec();
            let clan_name = extract_cstring(&data, 12);
            self.m_ClanLastInviteName = extract_cstring(&data, 12 + clan_name.len());
            return String::from_utf8_lossy(&self.m_ClanLastInviteName).to_string();
        }
        String::new()
    }

    pub fn SEND_PROTOCOL_INITIALIZE_SELECTOR(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(1);
        packet
    }

    pub fn SEND_SID_NULL(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_NULL as u8);
        packet.push(0);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_STOPADV(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_STOPADV as u8);
        packet.push(0);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_GETADVLISTEX(&self, game_name: String) -> ByteArray {
        let map_filter1 = [255, 3, 0, 0];
        let map_filter2 = [255, 3, 0, 0];
        let map_filter3 = [0, 0, 0, 0];
        let num_games = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_GETADVLISTEX as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&map_filter1);
        packet.extend_from_slice(&map_filter2);
        packet.extend_from_slice(&map_filter3);
        packet.extend_from_slice(&num_games);
        append_byte_array_fast_from_string(&mut packet, &game_name, false);
        packet.push(0);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_ENTERCHAT(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_ENTERCHAT as u8);
        packet.push(0);
        packet.push(0);
        packet.push(0);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_JOINCHANNEL(&self, channel: String) -> ByteArray {
        let no_create_join = [2, 0, 0, 0];
        let first_join = [1, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_JOINCHANNEL as u8);
        packet.push(0);
        packet.push(0);
        if !channel.is_empty() {
            packet.extend_from_slice(&no_create_join);
        } else {
            packet.extend_from_slice(&first_join);
        }
        append_byte_array_fast_from_string(&mut packet, &channel, true);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CHATCOMMAND(&self, command: String) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CHATCOMMAND as u8);
        packet.push(0);
        packet.push(0);
        append_byte_array_fast_from_string(&mut packet, &command, true);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CHECKAD(&self) -> ByteArray {
        let zeros = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CHECKAD as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&zeros);
        packet.extend_from_slice(&zeros);
        packet.extend_from_slice(&zeros);
        packet.extend_from_slice(&zeros);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_STARTADVEX3(&self, state: u8, map_game_type: ByteArray, map_flags: ByteArray, map_width: ByteArray, map_height: ByteArray, game_name: String, host_name: String, up_time: u32, map_path: String, map_crc: ByteArray, map_sha1: ByteArray, host_counter: u32) -> ByteArray {
        let unknown = [255, 3, 0, 0];
        let custom_game = [0, 0, 0, 0];
        let mut host_counter_string = format!("{:x}", host_counter);
        if host_counter_string.len() < 8 {
            host_counter_string = format!("{:0>8}", host_counter_string);
        }
        let host_counter_string: String = host_counter_string.chars().rev().collect();
        let mut packet = ByteArray::new();
        let mut stat_string = ByteArray::new();
        stat_string.extend_from_slice(&map_flags);
        stat_string.push(0);
        stat_string.extend_from_slice(&map_width);
        stat_string.extend_from_slice(&map_height);
        stat_string.extend_from_slice(&map_crc);
        append_byte_array_fast_from_string(&mut stat_string, &map_path, true);
        append_byte_array_fast_from_string(&mut stat_string, &host_name, true);
        stat_string.push(0);
        stat_string.extend_from_slice(&map_sha1);
        let stat_string = encode_stat_string(&stat_string);
        // println!("--- SEND_SID_STARTADVEX3 CHECK ---");
        // println!("map_game_type = {:?}, len = {}", map_game_type, map_game_type.len());
        // println!("  → expected: len == 4");

        // println!("map_flags = {:?}, len = {}", map_flags, map_flags.len());
        // println!("  → expected: len == 4");

        // println!("map_width = {:?}, len = {}", map_width, map_width.len());
        // println!("  → expected: len == 2");

        // println!("map_height = {:?}, len = {}", map_height, map_height.len());
        // println!("  → expected: len == 2");

        // println!("game_name = \"{}\", is_empty = {}", game_name, game_name.is_empty());
        // println!("  → expected: is_empty == false");

        // println!("host_name = \"{}\", is_empty = {}", host_name, host_name.is_empty());
        // println!("  → expected: is_empty == false");

        // println!("map_path = \"{}\", is_empty = {}", map_path, map_path.is_empty());
        // println!("  → expected: is_empty == false");

        // println!("map_crc = {:?}, len = {}", map_crc, map_crc.len());
        // println!("  → expected: len == 4");

        // println!("map_sha1 = {:?}, len = {}", map_sha1, map_sha1.len());
        // println!("  → expected: len == 20");

        // println!("stat_string = {:?}, len = {}", stat_string, stat_string.len());
        // println!("  → expected: len < 128");

        // println!("host_counter_string = \"{}\", len = {}", host_counter_string, host_counter_string.len());
        // println!("  → expected: len == 8");

        if map_game_type.len() == 4 && map_flags.len() == 4 && map_width.len() == 2 && map_height.len() == 2 && !game_name.is_empty() && !host_name.is_empty() && !map_path.is_empty() && map_crc.len() == 4 && map_sha1.len() == 20 && stat_string.len() < 128 && host_counter_string.len() == 8 {
            //println!("Called SEND_SID_STARTADVEX3");
            packet.push(BNET_HEADER_CONSTANT);
            packet.push(Protocol::SID_STARTADVEX3 as u8);
            packet.push(0);
            packet.push(0);
            packet.push(state);
            packet.push(0);
            packet.push(0);
            packet.push(0);
            append_byte_array_from_u32(&mut packet, up_time, false);
            append_byte_array(&mut packet, map_game_type);
            append_byte_array(&mut packet, unknown.to_vec());
            append_byte_array(&mut packet, custom_game.to_vec());
            append_byte_array_fast_from_string(&mut packet, &game_name, true);
            packet.push(0);
            packet.push(98);
            append_byte_array_fast_from_string(&mut packet, &host_counter_string, false);
            append_byte_array(&mut packet,stat_string);
            packet.push(0);  
            self.AssignLength(&mut packet);
            // for byte in &packet {
            //     print!("{:02x} ", byte);
            // }
        }
        //println!("End SEND_SID_STARTADVEX3");
        // for byte in &packet {
        //     print!("{:02x} ", byte);
        // }
        packet
    }

    pub fn SEND_SID_NOTIFYJOIN(&self, game_name: String) -> ByteArray {
        let product_id = [0, 0, 0, 0];
        let product_version = [14, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_NOTIFYJOIN as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&product_id);
        packet.extend_from_slice(&product_version);
        append_byte_array_fast_from_string(&mut packet, &game_name, false);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_PING(&self, ping_value: ByteArray) -> ByteArray {
        let mut packet = ByteArray::new();
        if ping_value.len() == 4 {
            packet.push(BNET_HEADER_CONSTANT);
            packet.push(Protocol::SID_PING as u8);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&ping_value);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_SID_LOGONRESPONSE(&self, client_token: ByteArray, server_token: ByteArray, password_hash: ByteArray, account_name: String) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_LOGONRESPONSE as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&client_token);
        packet.extend_from_slice(&server_token);
        packet.extend_from_slice(&password_hash);
        append_byte_array_fast_from_string(&mut packet, &account_name, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_NETGAMEPORT(&self, server_port: u16) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_NETGAMEPORT as u8);
        packet.push(0);
        packet.push(0);
        append_byte_array_from_u16(&mut packet, server_port, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_AUTH_INFO(&self, ver: u8, tft: bool, locale_id: u32, country_abbrev: String, country: String) -> ByteArray {
        let protocol_id = [0, 0, 0, 0];
        let platform_id = [54, 56, 88, 73];
        let product_id_roc = [51, 82, 65, 87];
        let product_id_tft = [80, 88, 51, 87];
        let version = [ver, 0, 0, 0];
        let language = [83, 85, 110, 101];
        let local_ip = [127, 0, 0, 1];
        let time_zone_bias = [44, 1, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_AUTH_INFO as u8);
        packet.push(0);
        packet.push(0);
        append_byte_array_size(&mut packet, &protocol_id, 4);
        append_byte_array_size(&mut packet, &platform_id, 4);
        
        if tft {
            append_byte_array_size(&mut packet, &product_id_tft, 4);
        } else {
            append_byte_array_size(&mut packet, &product_id_roc, 4);
        }
        append_byte_array_size(&mut packet, &version, 4);
        append_byte_array_size(&mut packet, &language, 4);
        append_byte_array_size(&mut packet, &local_ip, 4);
        append_byte_array_size(&mut packet, &time_zone_bias, 4);
        append_byte_array_from_u32(&mut packet, locale_id, false);
        append_byte_array_from_u32(&mut packet, locale_id, false);
        append_byte_array_fast_from_string(&mut packet, &country_abbrev, true);
        append_byte_array_fast_from_string(&mut packet, &country, true);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_AUTH_CHECK(&self, tft: bool, client_token: ByteArray, exe_version: ByteArray, exe_version_hash: ByteArray, key_info_roc: ByteArray, key_info_tft: ByteArray, exe_info: String, key_owner_name: String) -> ByteArray {
        let num_keys = if tft { 2 } else { 1 };
        let mut packet = ByteArray::new();
        if client_token.len() == 4 && exe_version.len() == 4 && exe_version_hash.len() == 4 && key_info_roc.len() == 36 && (!tft || key_info_tft.len() == 36) {
            packet.push(BNET_HEADER_CONSTANT);
            packet.push(Protocol::SID_AUTH_CHECK as u8);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&client_token);
            packet.extend_from_slice(&exe_version);
            packet.extend_from_slice(&exe_version_hash);
            append_byte_array_from_u32(&mut packet, num_keys, false);
            append_byte_array_from_u32(&mut packet, 0, false);
            packet.extend_from_slice(&key_info_roc);
            if tft {
                packet.extend_from_slice(&key_info_tft);
            }
            append_byte_array_fast_from_string(&mut packet, &exe_info, true);
            append_byte_array_fast_from_string(&mut packet, &key_owner_name, true);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_SID_AUTH_ACCOUNTLOGON(&self, client_public_key: ByteArray, account_name: String) -> ByteArray {
        let mut packet = ByteArray::new();
        if client_public_key.len() == 32 {
            packet.push(BNET_HEADER_CONSTANT);
            packet.push(Protocol::SID_AUTH_ACCOUNTLOGON as u8);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&client_public_key);
            append_byte_array_fast_from_string(&mut packet, &account_name, true);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_SID_AUTH_ACCOUNTLOGONPROOF(&self, client_password_proof: ByteArray) -> ByteArray {
        let mut packet = ByteArray::new();
        if client_password_proof.len() == 20 {
            packet.push(BNET_HEADER_CONSTANT);
            packet.push(Protocol::SID_AUTH_ACCOUNTLOGONPROOF as u8);
            packet.push(0);
            packet.push(0);
            packet.extend_from_slice(&client_password_proof);
            self.AssignLength(&mut packet);
        }
        packet
    }

    pub fn SEND_SID_WARDEN(&self, warden_response: ByteArray) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_WARDEN as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&warden_response);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_FRIENDSLIST(&self) -> ByteArray {
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_FRIENDSLIST as u8);
        packet.push(0);
        packet.push(0);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CLANMEMBERLIST(&self) -> ByteArray {
        let cookie = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CLANMEMBERLIST as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&cookie);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CLANINVITATION(&self, account_name: String) -> ByteArray {
        let cookie = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CLANINVITATION as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&cookie);
        append_byte_array_fast_from_string(&mut packet, &account_name, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CLANREMOVEMEMBER(&self, account_name: String) -> ByteArray {
        let cookie = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CLANREMOVEMEMBER as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&cookie);
        append_byte_array_fast_from_string(&mut packet, &account_name, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CLANCHANGERANK(&self, account_name: String, rank: RankCode) -> ByteArray {
        let cookie = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CLANCHANGERANK as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&cookie);
        append_byte_array_fast_from_string(&mut packet, &account_name, false);
        packet.push(rank as u8);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CLANSETMOTD(&self, motd: String) -> ByteArray {
        let cookie = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CLANSETMOTD as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&cookie);
        append_byte_array_fast_from_string(&mut packet, &motd,false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CLANCREATIONINVITATION(&self, accept: bool) -> ByteArray {
        let cookie = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CLANCREATIONINVITATION as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&cookie);
        packet.extend_from_slice(&self.m_ClanLastInviteTag);
        packet.extend_from_slice(&self.m_ClanLastInviteName);
        packet.push(if accept { 0x06 } else { 0x04 });
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_SID_CLANINVITATIONRESPONSE(&self, accept: bool) -> ByteArray {
        let cookie = [0, 0, 0, 0];
        let mut packet = ByteArray::new();
        packet.push(BNET_HEADER_CONSTANT);
        packet.push(Protocol::SID_CLANINVITATIONRESPONSE as u8);
        packet.push(0);
        packet.push(0);
        packet.extend_from_slice(&cookie);
        packet.extend_from_slice(&self.m_ClanLastInviteTag);
        packet.extend_from_slice(&self.m_ClanLastInviteName);
        packet.push(if accept { 0x06 } else { 0x04 });
        self.AssignLength(&mut packet);
        packet
    }

    fn AssignLength(&self, content: &mut ByteArray) -> bool {
        let mut length_bytes = ByteArray::new();
        if content.len() >= 4 && content.len() <= 65535 {
            length_bytes = create_byte_array_from_u16(content.len() as u16, false);
            content[2] = length_bytes[0];
            content[3] = length_bytes[1];
            return true;
        }
        false
    }

    fn ValidateLength(&self, content: &ByteArray) -> bool {
        let length: u16;
        let mut length_bytes = ByteArray::new();
        if content.len() >= 4 && content.len() <= 65535 {
            length_bytes.push(content[2]);
            length_bytes.push(content[3]);
            length = byte_array_to_u16(&length_bytes, false, 0);
            return length == content.len() as u16;
        }
        false
    }
}

pub struct IncomingGameHost {
    m_IP: ByteArray,
    m_Port: u16,
    m_GameName: String,
    m_HostCounter: ByteArray,
}

impl IncomingGameHost {
    pub fn new(n_ip: ByteArray, n_port: u16, n_game_name: String, n_host_counter: ByteArray) -> Self {
        IncomingGameHost {
            m_IP: n_ip,
            m_Port: n_port,
            m_GameName: n_game_name,
            m_HostCounter: n_host_counter,
        }
    }

    pub fn GetIP(&self) -> ByteArray { self.m_IP.clone() }
    pub fn GetIPString(&self) -> String {
        if self.m_IP.len() >= 4 {
            return format!("{}.{}.{}.{}", self.m_IP[0], self.m_IP[1], self.m_IP[2], self.m_IP[3]);
        }
        String::new()
    }
    pub fn GetPort(&self) -> u16 { self.m_Port }
    pub fn GetGameName(&self) -> String { self.m_GameName.clone() }
    pub fn GetHostCounter(&self) -> ByteArray { self.m_HostCounter.clone() }
}

#[derive(Debug)]
pub struct IncomingChatEvent {
    m_ChatEvent: IncomingChatEventEnum,
    m_Ping: i32,
    m_User: String,
    m_Message: String,
}

impl IncomingChatEvent {
    pub fn new(n_chat_event: IncomingChatEventEnum, n_ping: i32, n_user: String, n_message: String) -> Self {
        IncomingChatEvent {
            m_ChatEvent: n_chat_event,
            m_Ping: n_ping,
            m_User: n_user,
            m_Message: n_message,
        }
    }

    pub fn GetChatEvent(&self) -> IncomingChatEventEnum { self.m_ChatEvent }
    pub fn GetPing(&self) -> i32 { self.m_Ping }
    pub fn GetUser(&self) -> String { self.m_User.clone() }
    pub fn GetMessage(&self) -> String { self.m_Message.clone() }
}

pub struct IncomingFriendList {
    m_Account: String,
    m_Status: u8,
    m_Area: u8,
    m_Location: String,
}

impl IncomingFriendList {
    pub fn new(n_account: String, n_status: u8, n_area: u8, n_location: String) -> Self {
        IncomingFriendList {
            m_Account: n_account,
            m_Status: n_status,
            m_Area: n_area,
            m_Location: n_location,
        }
    }

    pub fn GetAccount(&self) -> String { self.m_Account.clone() }
    pub fn GetStatus(&self) -> u8 { self.m_Status }
    pub fn GetArea(&self) -> u8 { self.m_Area }
    pub fn GetLocation(&self) -> String { self.m_Location.clone() }
    pub fn GetDescription(&self) -> String {
        format!("{}\n{}\n{}\n{}\n\n",
            self.GetAccount(),
            self.ExtractStatus(self.GetStatus()),
            self.ExtractArea(self.GetArea()),
            self.ExtractLocation(self.GetLocation()))
    }

    fn ExtractStatus(&self, status: u8) -> String {
        let mut result = String::new();
        if status & 1 != 0 { result.push_str("<Mutual>"); }
        if status & 2 != 0 { result.push_str("<DND>"); }
        if status & 4 != 0 { result.push_str("<Away>"); }
        if result.is_empty() { "<None>".to_string() } else { result }
    }

    fn ExtractArea(&self, area: u8) -> String {
        match area {
            0 => "<Offline>",
            1 => "<No Channel>",
            2 => "<In Channel>",
            3 => "<Public Game>",
            4 => "<Private Game>",
            5 => "<Private Game>",
            _ => "<Unknown>",
        }.to_string()
    }

    fn ExtractLocation(&self, location: String) -> String {
        if location.starts_with("PX3W") {
            location[4..].to_string()
        } else if location.is_empty() {
            ".".to_string()
        } else {
            location
        }
    }
}

pub struct IncomingClanList {
    m_Name: String,
    m_Rank: u8,
    m_Status: u8,
}

impl IncomingClanList {
    pub fn new(n_name: String, n_rank: u8, n_status: u8) -> Self {
        IncomingClanList {
            m_Name: n_name,
            m_Rank: n_rank,
            m_Status: n_status,
        }
    }

    pub fn GetName(&self) -> String { self.m_Name.clone() }
    pub fn GetRank(&self) -> String {
        match self.m_Rank {
            r if r == RankCode::CLAN_INITIATE as u8 => "Recruit",
            r if r == RankCode::CLAN_PARTIAL_MEMBER as u8 => "Peon",
            r if r == RankCode::CLAN_MEMBER as u8 => "Grunt",
            r if r == RankCode::CLAN_OFFICER as u8 => "Shaman",
            r if r == RankCode::CLAN_LEADER as u8 => "Chieftain",
            _ => "Rank Unknown",
        }.to_string()
    }
    pub fn GetStatus(&self) -> String {
        if self.m_Status == 0 { "Offline" } else { "Online" }.to_string()
    }
    pub fn GetDescription(&self) -> String {
        format!("{}\n{}\n{}\n\n",
            self.GetName(),
            self.GetStatus(),
            self.GetRank())
    }
}