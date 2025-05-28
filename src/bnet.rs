
use libc::wait;
use rand::rand_core::le;
use tokio::time::timeout;

use crate::config;
use crate::game;
use crate::game::Game;
use crate::gameprotocol::GAME_PRIVATE;
use crate::gameprotocol::GAME_PUBLIC;
use crate::ghost;
use crate::lang::Language;
use crate::logger::log_error;
use crate::logger::log_info;
use crate::logger::log_warning;
use crate::socket::*;
use crate::bnetprotocol::*;
use crate::util::*;
use crate::map::*;
use crate::util::{ByteArray};
use crate::ghost::*;
use crate::commandpacket::*;
use crate::bncsutilinterface::*;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;


#[derive(Debug, Clone)]
pub struct BNET {
    m_Ghost: Arc<Mutex<Ghost>>,
    m_Socket: TcpClient,
    m_Protocol: BnetProtocol,
    m_Packets: VecDeque<CommandPacket>,
    m_IncomingBuffer: Vec<u8>,
    m_BNCSUtil: BNCSUtilInterface,
    m_OutPackets: VecDeque<ByteArray>,
    m_Exiting: bool,
    m_Server: String,
    m_ServerIP: String,
    m_ServerAlias: String,
    m_ServerReconnectCount: u32,
    m_BNLSServer: String,
    m_BNLSPort: u16,
    m_BNLSWardenCookie: u32,
    m_CDKeyROC: String,
    m_CDKeyTFT: String,
    m_CountryAbbrev: String,
    m_Country: String,
    m_LocaleID: u32,
    m_UserName: String,
    m_UserPassword: String,
    m_FirstChannel: String,
    m_CurrentChannel: String,
    m_RootAdmin: String,
    m_CommandTrigger: char,
    m_War3Version: u8,
    m_EXEVersion: Vec<u8>,
    m_EXEVersionHash: Vec<u8>,
    m_PasswordHashType: String,
    m_PvPGNRealmName: String,
    m_MaxMessageLength: u32,
    m_HostCounterID: u32,
    m_LastDisconnectedTime: u32,
    m_LastConnectionAttemptTime: u32,
    m_LastNullTime: u32,
    m_LastOutPacketTicks: u32,
    m_LastOutPacketSize: u32,
    m_FrequencyDelayTimes: u32,
    m_LastAdminRefreshTime: u32,
    m_LastBanRefreshTime: u32,
    m_FirstConnect: bool,
    m_WaitingToConnect: bool,
    m_LoggedIn: bool,
    m_InChat: bool,
    m_HoldFriends: bool,
    m_HoldClan: bool,
    m_PublicCommands: bool,
    m_LastInviteCreation: bool,
    m_Admins: Vec<String>,
    m_CurrentMap: Map
}

impl BNET {

    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        ghost: Arc<Mutex<Ghost>>,
        nServer: String,
        nServerAlias: String,
        nBNLSServer: String,
        nBNLSPort: u16,
        nBNLSWardenCookie: u32,
        nCDKeyROC: String,
        nCDKeyTFT: String,
        nCountryAbbrev: String,
        nCountry: String,
        nLocaleID: u32,
        nUserName: String,
        nUserPassword: String,
        nFirstChannel: String,
        nRootAdmin: String,
        nCommandTrigger: char,
        nHoldFriends: bool,
        nHoldClan: bool,
        nPublicCommands: bool,
        nWar3Version: u8,
        nEXEVersion: Vec<u8>,
        nEXEVersionHash: Vec<u8>,
        nPasswordHashType: String,
        nPVPGNRealmName: String,
        nMaxMessageLength: u32,
        nHostCounterID: u32,
        nAdmins: Vec<String>
    ) -> Self {
        let ghost_ptr = Arc::as_ptr(&ghost) as usize;
        log_info(&format!("[BNET: {}] Ghost instance ptr: {:#x}", nServerAlias, ghost_ptr));
        let mut lower_server = nServer.clone();
        lower_server.make_ascii_lowercase();

        let server_alias = nServerAlias;

        // If pvpgn and BNLS server is configured, ignore BNLS server
        let mut bnlss = nBNLSServer.clone();
        let mut bnlsp = nBNLSPort;
        let mut bnlswc = nBNLSWardenCookie;
        if nPasswordHashType == "pvpgn" && !bnlss.is_empty() {
            log_info(&format!("[BNET: {}] pvpgn connection found with a configured BNLS server, ignoring BNLS server", server_alias));
            bnlss.clear();
            bnlsp = 0;
            bnlswc = 0;
        }
        
        let ghost_tft = true;
        if nCDKeyROC.len() != 26 {
            log_info(&format!("[BNET: {}] warning - your ROC CD key is not 26 characters long and is probably invalid", server_alias));
        }
        if ghost_tft && nCDKeyTFT.len() != 26 {
            log_info(&format!("[BNET: {}] warning - your TFT CD key is not 26 characters long and is probably invalid", server_alias));
        }
        let mut map = Map::new(Arc::clone(&ghost), "iCCup DotA 454.w3x".to_owned());
        map.load("iCCup DotA 454.w3x".to_string()).await;
        BNET {
            m_Ghost: ghost,
            m_Socket: TcpClient::new(),
            m_Protocol: BnetProtocol::new(),
            m_Packets: VecDeque::new(),
            m_IncomingBuffer: Vec::new(),
            m_BNCSUtil: BNCSUtilInterface::new(&nUserName, &nUserPassword),
            m_OutPackets: VecDeque::new(),
            m_Exiting: false,
            m_Server: lower_server.clone(),
            m_ServerIP: String::new(),
            m_ServerAlias: server_alias.clone(),
            m_ServerReconnectCount: 0,
            m_BNLSServer: bnlss.clone(),
            m_BNLSPort: bnlsp,
            m_BNLSWardenCookie: bnlswc,
            m_CDKeyROC: nCDKeyROC.clone(),
            m_CDKeyTFT: nCDKeyTFT.clone(),
            m_CountryAbbrev: nCountryAbbrev.clone(),
            m_Country: nCountry.clone(),
            m_LocaleID: nLocaleID,
            m_UserName: nUserName.clone(),
            m_UserPassword: nUserPassword.clone(),
            m_FirstChannel: nFirstChannel.clone(),
            m_CurrentChannel: nFirstChannel.clone(),
            m_RootAdmin: nRootAdmin.clone(),
            m_CommandTrigger: nCommandTrigger,
            m_War3Version: nWar3Version,
            m_EXEVersion: nEXEVersion.clone(),
            m_EXEVersionHash: nEXEVersionHash.clone(),
            m_PasswordHashType: nPasswordHashType.clone(),
            m_PvPGNRealmName: nPVPGNRealmName.clone(),
            m_MaxMessageLength: nMaxMessageLength,
            m_HostCounterID: nHostCounterID,
            m_LastDisconnectedTime: 0,
            m_LastConnectionAttemptTime: 0,
            m_LastNullTime: 0,
            m_LastOutPacketTicks: 0,
            m_LastOutPacketSize: 0,
            m_FrequencyDelayTimes: 0,
            m_LastAdminRefreshTime: get_time(),
            m_LastBanRefreshTime: get_time(),
            m_FirstConnect: true,
            m_WaitingToConnect: true,
            m_LoggedIn: false,
            m_InChat: false,
            m_HoldFriends: nHoldFriends,
            m_HoldClan: nHoldClan,
            m_PublicCommands: nPublicCommands,
            m_LastInviteCreation: false,
            m_Admins: nAdmins,
            m_CurrentMap: map
        }
    }

    // In BNET struct
// Add:
// m_LastOutPacketInstant: Option<tokio::time::Instant>, (for send throttling)

// In BNET::new()
// m_LastOutPacketInstant: None,

pub async fn update(&mut self) -> bool {
    if !self.m_Socket.connected() {
        self.m_Socket.connect(&self.m_Server, 6112).await;

        if self.m_Socket.connected() {
            log_info(&format!(
                "[BNET: {}] connected to server on {} ip and {} port",
                self.m_ServerAlias,
                self.m_Server,
                6112
            ));
            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_PROTOCOL_INITIALIZE_SELECTOR()).await;
            let _ = self.m_Socket.do_send(
                &self.m_Protocol.SEND_SID_AUTH_INFO(
                    self.m_War3Version,
                    true,
                    self.m_LocaleID,
                    self.m_CountryAbbrev.clone(),
                    self.m_Country.clone()
                )
            ).await;
        } else {
            log_info(&format!("[BNET: {}] failed to connect to server", self.m_ServerAlias));
            return true;
        }
    }

    if self.m_Socket.connected() {
        let mut buf = [0u8; 4096];

        match timeout(Duration::from_millis(1), self.m_Socket.do_recv(&mut buf)).await {
            Ok(Ok(bytes_received)) if bytes_received > 0 => {
                self.m_IncomingBuffer.extend(&buf[..bytes_received]);
                
                self.extract_packets().await;
                self.process_packets().await;
        
                let mut wait_ticks = 0;
        
                wait_ticks += self.m_FrequencyDelayTimes * 60;
        
                if !self.m_OutPackets.is_empty() && get_ticks() - self.m_LastOutPacketTicks as u128 >= wait_ticks.into() {
                    if self.m_OutPackets.len() > 7 {
                        log_warning(&format!("[BNET: {}] packet queue warning - there are {} packets waiting to be sent", self.m_ServerAlias, self.m_OutPackets.len()));
                    }
        
                    let mut last = self.m_OutPackets.pop_front().unwrap();
                    let _ = self.m_Socket.do_send(&last).await;
        
                    if self.m_FrequencyDelayTimes >= 100 || get_ticks() > (self.m_LastOutPacketTicks + wait_ticks + 400).into() {
                        self.m_FrequencyDelayTimes = 0;
                    } else {
                        self.m_FrequencyDelayTimes += 1;
                    }
        
                    self.m_LastOutPacketTicks = get_ticks() as u32;
                }
        
                if get_time() - self.m_LastNullTime >= 60 && get_ticks() - self.m_LastOutPacketTicks as u128 >= 60000 {
                    let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_NULL()).await;
                }
        
                return self.m_Exiting;
            }
            Ok(Ok(_)) => {
                log_info(&format!(
                    "[BNET: {}] recv returned nothing or error",
                    self.m_ServerAlias
                ));
            }
            Ok(Err(e)) => {
                log_warning(&format!("[BNET: {}] recv error: {}", self.m_ServerAlias, e));
                self.m_Socket.disconnect();
            }
            Err(_) => {
                // Таймаут - recv не вернул данные за 10ms
                // Можно просто продолжить или сделать логику "ничего не пришло"
            }
        }
    }

    self.m_Exiting
}

    
    pub async fn extract_packets(&mut self) {
        //log_info(&format!("[BNET: {}] extracting packets from buffer, size {:?}", self.m_ServerAlias, bytes));
        while self.m_IncomingBuffer.len() >= 4 {
            let bytes = &self.m_IncomingBuffer;
            if bytes[0] == 255 {
                let length = byte_array_to_u16(&bytes, false, 2);
                //log_info(&format!("[BNET: {}] found packet with length {}", self.m_ServerAlias, length));
                
                if length >= 4 {
                    if bytes.len() >= length as usize {
                        let packet_data = self.m_IncomingBuffer[..length as usize].to_vec();

                        // log_info(
                        //     &format!(
                        //     "packet_data bytes: {:?}",
                        //     packet_data
                        //     )
                        // );
                        self.m_Packets.push_back(CommandPacket::new(
                            255,
                            bytes[1] as i32,
                            packet_data,
                        ));

                        self.m_IncomingBuffer.drain(..length as usize);
                    } else {
                        return;
                    }
                } else {
                    log_warning(&format!(
                        "[BNET: {}] error - received invalid packet from battle.net (bad length), disconnecting",
                        self.m_ServerAlias
                    ));
                    return;
                }
            } else {
                log_warning(&format!(
                    "[BNET: {}] error - received invalid packet from battle.net (bad header constant), disconnecting",
                    self.m_ServerAlias
                ));
                return;
            }
        }
    }
    
    

    pub async fn process_packets(&mut self) {
        
        let mut game_host: Option<IncomingGameHost>;
        let mut chat_event: Option<IncomingChatEvent>;

        let mut warden_data: ByteArray;
        let mut friends: Vec<IncomingFriendList>;
        let mut clans: Vec<IncomingClanList>;
        //println!("{:?}", self.m_Packets);
        while let Some(packet) = self.m_Packets.pop_front() {
            if packet.get_packet_type() == 255 {
                let packet_type: i32 = packet.get_id();
                
                match packet_type {
                    x if x == Protocol::SID_NULL as i32 => {
                        self.m_Protocol.RECEIVE_SID_NULL( packet.get_data().to_vec() );
                    }
                    x if x == Protocol::SID_GETADVLISTEX as i32 => {
                        game_host = self.m_Protocol.RECEIVE_SID_GETADVLISTEX( packet.get_data().to_vec() );
                        
                        if game_host.is_some() {
                            log_info(&format!("[BNET: {}] joining game [{}]", self.m_ServerAlias, game_host.as_ref().unwrap().GetGameName()));
                        }
                        game_host = None;
                    }
                    x if x == Protocol::SID_ENTERCHAT as i32 => {

                        if self.m_Protocol.RECEIVE_SID_ENTERCHAT( packet.get_data().to_vec() ) {
                            log_info(&format!("[BNET: {}] joining channel '{}'", self.m_ServerAlias, self.m_FirstChannel));
                            self.m_InChat = true;
                            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_JOINCHANNEL( self.m_FirstChannel.clone())).await;
                        }
                    }
                    x if x == Protocol::SID_CHATEVENT as i32 => {
                        chat_event = self.m_Protocol.RECEIVE_SID_CHATEVENT(packet.get_data().to_vec());
                        if chat_event.is_some() {
                            self.process_chat_event(&chat_event.unwrap()).await;
                        }
                        chat_event = None;
                    }
                    x if x == Protocol::SID_CHECKAD as i32 => {
                        self.m_Protocol.RECEIVE_SID_CHECKAD( packet.get_data().to_vec() );
                    }
                    x if x == Protocol::SID_STARTADVEX3 as i32 => {
                        if self.m_Protocol.RECEIVE_SID_STARTADVEX3( packet.get_data().to_vec() ) {
                            self.m_InChat = false;
                        }
                        else {
                            log_info(&format!("[BNET: {}] error - STARTADVEX3", self.m_ServerAlias));
                        }
                    }
                    x if x == Protocol::SID_PING as i32 => {
                        let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_PING(packet.get_data().to_vec())).await;
                    }
                    x if x == Protocol::SID_AUTH_INFO as i32 => {
                        log_info(&format!("[BNET: {}] received SID_AUTH_INFO", self.m_ServerAlias));
                        if self.m_Protocol.RECEIVE_SID_AUTH_INFO( packet.get_data().to_vec() ) {
                            if self.m_BNCSUtil.HELP_SID_AUTH_CHECK(true, self.m_War3Version.into(), &config::get_string("bot_war3path", ""), &self.m_CDKeyROC, &self.m_CDKeyTFT, &self.m_Protocol.GetValueStringFormulaString(), &self.m_Protocol.GetIX86VerFileNameString(), self.m_Protocol.GetClientToken(), self.m_Protocol.GetServerToken()) {
                                
                                if self.m_EXEVersion.len() == 4 {
                                    log_info(&format!("[BNET: {}] using custom exe version bnet_custom_exeversion = {} {} {} {}", self.m_ServerAlias, self.m_EXEVersion[0], self.m_EXEVersion[1], self.m_EXEVersion[2], self.m_EXEVersion[3]));
                                    self.m_BNCSUtil.set_exe_version(self.m_EXEVersion.clone());
                                }

                                if self.m_EXEVersionHash.len() == 4 {
                                    log_info(&format!("[BNET: {}] using custom exe version hash bnet_custom_exeversion_hash = {} {} {} {}", self.m_ServerAlias, self.m_EXEVersionHash[0], self.m_EXEVersionHash[1], self.m_EXEVersionHash[2], self.m_EXEVersionHash[3]));
                                    self.m_BNCSUtil.set_exe_version_hash(self.m_EXEVersionHash.clone());
                                }

                                log_info(&format!("[BNET: {}] attempting to auth as Warcraft III: The Frozen Throne", self.m_ServerAlias));

                                let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_AUTH_CHECK(true, self.m_Protocol.GetClientToken(), self.m_BNCSUtil.get_exe_version().to_vec(), self.m_BNCSUtil.get_exe_version_hash().to_vec(), self.m_BNCSUtil.get_key_info_roc().to_vec(), self.m_BNCSUtil.get_key_info_tft().to_vec(), self.m_BNCSUtil.get_exe_info().to_string(), "GHost".to_owned())).await;


                            }   
                        }
                        else {
                            log_info(&format!("[BNET: {}] logon failed - bncsutil key hash failed (check your Warcraft 3 path and cd keys), disconnecting", self.m_ServerAlias));
                            self.m_Socket.disconnect();
                            return;
                        }
                    }
                    x if x == Protocol::SID_AUTH_CHECK as i32 => {
                        //log_info(&format!("[BNET: {}] received SID_AUTH_CHECK", self.m_ServerAlias));
                        if self.m_Protocol.RECEIVE_SID_AUTH_CHECK(packet.get_data().to_vec() ) 
                        {
                            log_info(&format!("[BNET: {}] cd keys accepted", self.m_ServerAlias));
                            self.m_BNCSUtil.HELP_SID_AUTH_ACCOUNTLOGON();
                            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_AUTH_ACCOUNTLOGON(
                                self.m_BNCSUtil.get_client_key().to_vec(), 
                                self.m_UserName.clone(),
                            )).await;
                        }
                        else {
                            // CD keys not accepted
                            let key_state = byte_array_to_u32(&self.m_Protocol.GetKeyState(), false,0);
                            let reason = match key_state {
                                513 => format!(
                                    "logon failed - ROC CD key in use by user [{}], disconnecting",
                                    self.m_Protocol.GetKeyStateDescription()
                                ),
                                529 => format!(
                                    "logon failed - TFT CD key in use by user [{}], disconnecting",
                                    self.m_Protocol.GetKeyStateDescription()
                                ),
                                256 => "logon failed - game version is too old, disconnecting".to_string(),
                                257 => "logon failed - game version is invalid, disconnecting".to_string(),
                                _ => "logon failed - cd keys not accepted, disconnecting".to_string(),
                            };
                    
                            log_warning(&format!("[BNET: {}] {}", self.m_ServerAlias, reason));
                            self.m_Socket.disconnect();
                            return;
                        }
                    }

                    x if x == Protocol::SID_AUTH_ACCOUNTLOGON as i32 => {
                        if self.m_Protocol.RECEIVE_SID_AUTH_ACCOUNTLOGON( packet.get_data().to_vec() ) {
                                
                            log_info(&format!("[BNET: {}] username {} accepted", self.m_ServerAlias , self.m_UserName));

                            if self.m_PasswordHashType == "pvpgn" {
                                log_info(&format!("[BNET: {}] using pvpgn password hash", self.m_ServerAlias));
                                self.m_BNCSUtil.HELP_PvPGNPasswordHash(&self.m_UserPassword);
                                let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_AUTH_ACCOUNTLOGONPROOF(
                                    self.m_BNCSUtil.get_pvpgn_password_hash().to_vec(), 
                                )).await;
                            }
                        } else {
                            log_info(&format!("[BNET: {}] logon failed - invalid username", self.m_ServerAlias));
                            self.m_Socket.disconnect();
                            return;
                        }

                    }

                    x if x == Protocol::SID_AUTH_ACCOUNTLOGONPROOF as i32 => {
                        if self.m_Protocol.RECEIVE_SID_AUTH_ACCOUNTLOGONPROOF( packet.get_data().to_vec() ) {
                            log_info(&format!("[BNET: {}] logon successful", self.m_ServerAlias));
                            self.m_LoggedIn = true;
                            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_NETGAMEPORT(6112)).await;
                            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_ENTERCHAT()).await;
                            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_FRIENDSLIST()).await;
                            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_CLANMEMBERLIST()).await;
                        }
                        else {
                            log_info(&format!("[BNET: {}] logon failed - invalid password", self.m_ServerAlias));
                            self.m_Socket.disconnect();
                            return;
                        }
                    }
                    x if x == Protocol::SID_WARDEN as i32 => {

                    }

                    x if x == Protocol::SID_FRIENDSLIST as i32 => {

                    }

                    x if x == Protocol::SID_CLANMEMBERLIST as i32 => {

                    }
                    x if x == Protocol::SID_CLANCREATIONINVITATION as i32 => {

                    }

                    x if x == Protocol::SID_CLANINVITATIONRESPONSE as i32 => {

                    }
                    _ => {
                        // handle other packet types or ignore
                    }
                }
                
            }
        }
    }

    pub async fn process_chat_event(&mut self, chat_event: &IncomingChatEvent) {
        let event: IncomingChatEventEnum = chat_event.GetChatEvent();
        let whisper: bool = event == IncomingChatEventEnum::EID_WHISPER;
        let user: String = chat_event.GetUser().to_string();
        let message: String = chat_event.GetMessage().to_string();

        if event == IncomingChatEventEnum::EID_WHISPER || event == IncomingChatEventEnum::EID_TALK {
            if event == IncomingChatEventEnum::EID_WHISPER {
                log_info(&format!("[WHISPER: {}] [{}] {}", self.m_ServerAlias, user, message));
                
            } else {
                
                log_info(&format!("[LOCAL: {}] [{}] {}", self.m_ServerAlias, user, message));
            }

            if !message.is_empty() && message.starts_with(self.m_CommandTrigger) {
                let content = &message[1..]; // обрезаем символ команды
                let (command, payload) = match content.find(' ') {
                    Some(index) => {
                        let cmd = &content[..index];
                        let pl = &content[index + 1..];
                        (cmd.to_lowercase(), pl.to_string())
                    },
                    None => (content.to_lowercase(), String::new()),
                };
                if self.is_admin(user.clone()) {
                    
                    log_info(&format!("[BNET: {}] админ [{}] отправил команду [{}]", self.m_ServerAlias, user, message));

                    let game_arc = {
                        let current_game = CURRENT_GAME.read().await;
                        current_game.as_ref().map(Arc::clone)
                    };

                    if (command == "channel" || command == "j") && !payload.is_empty() {
                        self.queue_chat_command(format!("/join {}", payload)).await;
                    }
                    else if command == "close" && !payload.is_empty() {
                        if let Some(game_arc) = game_arc {
                            game_arc.lock().await.close_slot(payload.parse::<u8>().unwrap(), true).await;
                        }
                    }
                    else if command == "closeall" {
                        if let Some(game_arc) = game_arc {
                            game_arc.lock().await.close_all_slots().await;
                        }
                    }
                    else if command == "hold" && !payload.is_empty() {
                        if let Some(game_arc) = game_arc {
                            game_arc.lock().await.add_to_reserved(payload);
                        }
                    } 
                    else if command == "map" {
                        if payload.is_empty() {
                            let current_cfg = self.m_CurrentMap.get_map_path();
                            self.queue_chat_command2(
                                Language::new().currently_loaded_map_cfg_is(&current_cfg),
                                user.clone(),
                                whisper
                            ).await;
                        } else {
                            let mut found_maps = vec![];
                            let map_path = Path::new("maps/");
                            let pattern = payload.to_lowercase();

                            if !map_path.exists() {
                                log_info(&format!("[BNET: {}] error listing maps - map path doesn't exist", self.m_ServerAlias));
                                self.queue_chat_command2(Language::new().error_listing_maps(), user.clone(), whisper).await;
                            } else {
                                let mut last_match = None;
                                let mut matches = 0;

                                let read_dir = match fs::read_dir(map_path) {
                                    Ok(d) => d,
                                    Err(e) => {
                                        println!("[BNET: {}] error listing maps - {}", self.m_ServerAlias, e);
                                        self.queue_chat_command2(Language::new().error_listing_maps(), user.clone(), whisper).await;
                                        return;
                                    }
                                };

                                for entry in read_dir.flatten() {
                                    let path = entry.path();
                                    let file_name = entry.file_name().to_string_lossy().to_lowercase();
                                    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();

                                    if path.is_file() && file_name.contains(&pattern) {
                                        last_match = Some(path.clone());
                                        matches += 1;

                                        let file_str = path.file_name().unwrap_or_default().to_string_lossy();
                                        found_maps.push(file_str.to_string());

                                        // точное совпадение — сразу прерываем
                                        if file_name == pattern || stem == pattern {
                                            matches = 1;
                                            break;
                                        }
                                    }
                                }

                                match matches {
                                    0 => {
                                        let response = Language::new().no_maps_found();
                                        self.queue_chat_command2(response, user.clone(), whisper).await;
                                    }
                                    1 => {
                                        let file = last_match.unwrap().file_name().unwrap().to_string_lossy().to_string();
                                        let response = Language::new().loading_config_file(&file);
                                        self.queue_chat_command2(response, user.clone(), whisper).await;

                                        // Загружаем карту
                                        // let mut cfg = Config::new();
                                        // cfg.set("map_path", &format!("Maps\\Download\\{}", file));
                                        // cfg.set("map_localpath", &file);
                                        let _ = self.m_CurrentMap.load(file).await;
                                        //self.ghost.map_mut().load(&cfg, &file);
                                    }
                                    _ => {
                                        let response = Language::new().found_maps(&found_maps.join(", "));
                                        self.queue_chat_command2(response, user.clone(), whisper).await;
                                    }
                                }
                            }
                        }
                    }
                    else if command == "open" && !payload.is_empty() {
                        if let Some(game_arc) = game_arc {
                            game_arc.lock().await.open_slot(payload.parse::<u8>().unwrap(), true).await;
                        }
                    } 
                    else if command == "openall" && !payload.is_empty() {
                        if let Some(game_arc) = game_arc {
                            game_arc.lock().await.open_all_slots().await;
                        }
                    } 
                    else if command == "priv" && !payload.is_empty() {
                        self.create_game(payload, user, GAME_PRIVATE).await;
                    }
                    else if command == "pub" && !payload.is_empty() {
                        self.create_game(payload, user, GAME_PUBLIC).await;
                    }
                    else if command == "say" {
                        self.queue_chat_command(payload).await;
                    } 
                    else if command == "sp" || command == "shuffle" {
                        if let Some(game_arc) = game_arc {
                            let mut game = game_arc.lock().await;
                            if !game.get_count_down_started() {
                                game.shuffle_slots().await;
                            }
                        }
                    }
                    else if command == "start" {
                        if let Some(game_arc) = game_arc {
                            let mut game = game_arc.lock().await;
                            if !game.get_count_down_started() && game.get_num_human_players() > 0 {
                                if !game.get_locked() {
                                    game.start_count_down( false ).await;
                                }
                            }
                        }
                    }
                    else if command == "swap" {
                        if let Some(game_arc) = game_arc {
                            let mut game = game_arc.lock().await;
                            let pl: Vec<u8> = payload.split_whitespace().filter_map(
                                |s| s.parse().ok()
                            ).collect();
                            if pl.len() != 2 {
                                log_info(&format!("[BNET: {}] неправильные аргументы команды \"swap\"", self.m_ServerAlias));
                            } else {
                                game.swap_slots(pl[0], pl[1]).await;
                            }
                        }
                    }
                    else if command == "unhost" {
                        if let Some(game_arc) = game_arc {
                            let mut game = game_arc.lock().await;

                            if game.get_count_down_started() {
                                self.queue_chat_command2(Language::new().unable_to_unhost_game_countdown_started(&game.get_description()), user, whisper).await;
                            } else {
                                self.queue_chat_command2(Language::new().unhosting_game(&game.get_description()), user, whisper).await;
                                game.set_exiting(true);
                            }
                        } else {
                            self.queue_chat_command2(Language::new().unable_to_unhost_game_no_game_in_lobby(), user, whisper).await;
                        }
                    }
                }
            }
        } else if event == IncomingChatEventEnum::EID_CHANNEL {
            log_info(&format!("[BNET: {}] joined channel [{}]", self.m_ServerAlias, message));
            self.m_CurrentChannel = message;
        } else if event == IncomingChatEventEnum::EID_INFO {
            log_info(&format!("[INFO: {}] {}", self.m_ServerAlias, message));
        } else if event == IncomingChatEventEnum::EID_ERROR {
            log_error(&format!("[ERROR: {}] {}", self.m_ServerAlias, message));
        } else if event == IncomingChatEventEnum::EID_EMOTE {
            log_info(&format!("[EMOTE: {}] [{}] {}", self.m_ServerAlias, user, message));
        }
    }

    pub async fn send_join_channel(&mut self, channel: String) {
        if self.m_LoggedIn && self.m_InChat {
            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_JOINCHANNEL(channel)).await;
        }
    }

    pub async fn send_get_friends_list(&mut self) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_FRIENDSLIST()).await;
        }
    }

    pub async fn send_get_clan_list(&mut self) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_CLANMEMBERLIST()).await;
        }
    }
    pub async fn send_clan_invitation(&mut self, account_name: String) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send(&&self.m_Protocol.SEND_SID_CLANINVITATION(account_name)).await;
        }
    }

    pub async fn send_clan_remove_member(&mut self, account_name: String) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send(&&&self.m_Protocol.SEND_SID_CLANREMOVEMEMBER(account_name)).await;
        }
    }

    pub async fn send_clan_change_rank(&mut self, account_name: String, rank: RankCode) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send(&&&self.m_Protocol.SEND_SID_CLANCHANGERANK(account_name, rank)).await;
        }
    }

    pub async fn send_clan_set_motd(&mut self, motd: String) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_CLANSETMOTD(motd)).await;
        }
    }

    pub async fn send_clan_accept_invite(&mut self, accept: bool) {
        if self.m_LoggedIn {
            if self.m_LastInviteCreation {
                let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_CLANCREATIONINVITATION(accept)).await;
            } else {
                let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_CLANINVITATIONRESPONSE(accept)).await;
            }
        }
    }

    pub async fn queue_enter_chat(&mut self) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send( &self.m_Protocol.SEND_SID_ENTERCHAT()).await;
        }
    }

    pub async fn queue_chat_command(&mut self, chat_command: String) {
        let mut mutable = chat_command.clone();
        if mutable.is_empty() {
            return;
        }

        if self.m_LoggedIn {
            if self.m_PasswordHashType == "pvpgn" && mutable.len() > self.m_MaxMessageLength as usize {
                mutable = mutable[..self.m_MaxMessageLength as usize].to_owned();
            }

            if mutable.len() > 255 {
                mutable = mutable[..255].to_owned();
            }

            if self.m_OutPackets.len() > 10 {
                log_info(&format!("[BNET: {}] attempted to queue chat command [{}] but there are too many ({}) packets queued, discarding",
                self.m_ServerAlias,
                mutable,
                self.m_OutPackets.len()
            ));
            } else {
                log_info(&format!("[QUEUED: {}] {}", self.m_ServerAlias, mutable));
                self.m_OutPackets.push_back(self.m_Protocol.SEND_SID_CHATCOMMAND(mutable));
            }
        }
    }

    pub async fn queue_chat_command2(&mut self, chat_command: String, user: String, whisper: bool) {
        if chat_command.is_empty() {
            return;
        }

        if whisper { 
            self.queue_chat_command(format!("/w {} {}", user, chat_command)).await;
        } else {
            self.queue_chat_command(chat_command).await;
        }
    }

    pub async fn queue_game_create( &mut self, state: u8, game_name: String, host_name :String, map: &mut Map, host_counter: u32) {
        if self.m_LoggedIn && map.get_valid() {
            if !self.m_CurrentChannel.is_empty() {
                self.m_FirstChannel = self.m_CurrentChannel.clone();
            }

            self.m_InChat = false;
            self.queue_game_refresh(state, game_name, host_name, map, 0, host_counter).await;

        }
    }

    pub async fn queue_game_refresh(&mut self, state: u8, game_name: String, host_name :String, map: &mut Map, up_time:u32, host_counter: u32) {
        let mut hostname = host_name.clone();
        if hostname.is_empty() {
            let unique_name = self.m_Protocol.GetUniqueName();
            hostname = String::from_utf8_lossy(&unique_name).to_string()
        }

        if self.m_LoggedIn && map.get_valid() {
            let fixed_host_counter = ( host_counter & 0x0FFFFFFF ) | (self.m_HostCounterID << 28);

            let mut map_game_type = map.get_map_game_type();
            map_game_type |= MAPGAMETYPE_UNKNOWN0;

            if state == GAME_PRIVATE {
                map_game_type |= MAPGAMETYPE_PRIVATEGAME;
            }

            let mut map_width: Vec<u8> = Vec::new();
            map_width.push( 192 );
            map_width.push( 7 );
            let mut map_height: Vec<u8> = Vec::new();
            map_height.push( 192 );
            map_height.push( 7 );

            // println!("STATE: {}", state);
            // print!("MAP GAME TYPE: ");
            // for byte in create_byte_array_from_u32(map_game_type, false) {
            //     print!("{:02x} ", byte);
            // }
            // println!();
            // print!("MAP_GAME_FLAGS: ");
            // for byte in map.get_map_game_flags() {
            //     print!("{:02x} ", byte);
            // }
            // println!();
            // println!("MAP WIDTH: {:x?}", map.get_map_width());
            // println!("MAP HEIGHT: {:x?}", map.get_map_height());
            // println!("GAME NAME: {}", game_name);
            // println!("HOST NAME: {}", hostname);
            // println!("UP TIME: {}", up_time);
            // println!("MAP PATH: {}", map.get_map_path());
            // for byte in map.get_map_crc() {
            //     print!("{:02x} ", byte);
            // }
            // for byte in map.get_map_sha1() {
            //     print!("{:02x} ", byte);
            // }
            // println!();
            // println!("FIXED HOST COUNTER: {}", fixed_host_counter);

            
            let _ = self.m_Socket.do_send(&self.m_Protocol.SEND_SID_STARTADVEX3(
                state,
                create_byte_array_from_u32(map_game_type, false),
                map.get_map_game_flags(),
                map.get_map_width(),
                map.get_map_height(),
                game_name,
                host_name,
                up_time,
                format!("Maps\\Download\\{}", map.get_map_path()),
                map.get_map_crc(),
                map.get_map_sha1(),
                fixed_host_counter,
            )
            ).await;
                    }
    }

    pub async fn queue_game_uncreate( &mut self ) {
        if self.m_LoggedIn {
            let _ = self.m_Socket.do_send( &self.m_Protocol.SEND_SID_STOPADV() ).await;
        }
    }

    pub fn unqueue_packets( &mut self , _type: u8) {
        let mut packets = VecDeque::<ByteArray>::new();
        let mut unqueued = 0;

        while !self.m_OutPackets.is_empty() {
            let packet = self.m_OutPackets.pop_front().unwrap();
            if packet.len() >= 2 && packet[1] == _type {
                unqueued += 1;
            } else {
                packets.push_back(packet);
            }
        }

        self.m_OutPackets = packets.clone();

        if unqueued > 0 {
            log_info(&format!("[BNET: {}] unqueued {} packets of type {}", self.m_ServerAlias, unqueued, _type));
        }
    }

    pub fn unqueue_chat_command(&mut self, chat_command: String) {
        let mut packet_to_unqueue = self.m_Protocol.SEND_SID_CHATCOMMAND(chat_command);
        let mut packets = VecDeque::<ByteArray>::new();
        let mut unqueued = 0;
        
        while !self.m_OutPackets.is_empty() {
            let packet = self.m_OutPackets.pop_front().unwrap();

            if packet == packet_to_unqueue {
                unqueued += 1;
            } else {
                packets.push_back(packet);
            }
        }
        self.m_OutPackets = packets.clone();

        if unqueued > 0 {
            log_info(&format!("[BNET: {}] unqueued {} chat command packets", self.m_ServerAlias, unqueued));
        }
    }

    pub fn is_admin(&mut self, nick: String) -> bool {
        self.m_Admins.contains(&nick)
    }

    pub fn unqueue_game_refreshes(&mut self) {
        self.unqueue_packets(Protocol::SID_STARTADVEX3 as u8);
    }

    pub async fn create_game(&mut self, payload: String, user: String, game_state: u8) {
        let host_counter = HOST_COUNTER.fetch_add(1, Ordering::SeqCst);
        if self.m_CurrentMap.get_valid() {
            let game_name = payload;
            let host_name = user.clone();
            log_info(&format!("[BNET: {}] Creating game: {}", self.m_ServerAlias, game_name));
            if game_state == GAME_PRIVATE {
                self.queue_chat_command(format!(
                    "Creating private game [{}] by user [{}]",
                    game_name, "BOT"
                ))
                .await;
            } else if game_state == GAME_PUBLIC {
                self.queue_chat_command(format!(
                    "Creating public game [{}] by user [{}]",
                    game_name, "BOT"
                ))
                .await;
            }
            let game = Game::new(
                Arc::clone(&self.m_Ghost),
                self.m_CurrentMap.clone(),
                6112,
                game_state,
                game_name.to_owned(),
                host_name.clone(),
                host_name.clone(),
                self.m_Server.clone(),
                host_counter
            );
            let base_game_arc = Arc::new(AsyncMutex::new(game.base));
            *CURRENT_GAME.write().await = Some(Arc::clone(&base_game_arc));
            self.queue_game_create(game_state, game_name.to_owned(), "BOT".to_owned(), &mut self.m_CurrentMap.clone(), host_counter)
                .await;
        } else {
            self.queue_chat_command(format!("/w {} Invalid map configuration", user))
                .await;
        }
    }
    pub fn get_server(&self) -> String {
        return self.m_Server.clone();
    }

    pub fn get_host_counter_id(&self) -> u32 {
        self.m_HostCounterID
    }

    pub fn get_out_packets_queued(&mut self) -> usize  {
        self.m_OutPackets.len()
    }
    
}