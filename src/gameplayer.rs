use serde::de;

use crate::gameprotocol::*;
use crate::socket::*;
use crate::logger::*;
use crate::commandpacket::*;
use crate::game::*;
use crate::gpsprotocol::*;
use crate::game_base::*;
use crate::util::byte_array_to_uint16;
use crate::util::create_byte_array_size;
use std::collections::VecDeque;

#[derive(Clone)]
#[derive(Debug)]
pub struct PotentialPlayer {
    pub m_Game: BaseGame,
    pub m_Protocol: GameProtocol,
    pub m_Socket: TcpClient,
    pub m_Packets: VecDeque<CommandPacket>,
    pub m_DeleteMe: bool,
    pub m_Error: bool,
    pub m_ErrorString: String,
    pub m_IncomingJoinPlayer: IncomingJoinPlayer,
    pub m_IncomingBuffer: Vec<u8>,
}

impl PotentialPlayer {
    pub fn new(protocol: GameProtocol, game: BaseGame, socket: TcpClient) -> Self {
        Self {
            m_Protocol: protocol,
            m_ErrorString: String::new(),
            m_Packets: VecDeque::new(),
            m_Game: game,
            m_Socket: socket,
            m_DeleteMe: false,
            m_Error: false,
            m_IncomingJoinPlayer: IncomingJoinPlayer::new(0, 0, String::new(), ByteArray::new()),
            m_IncomingBuffer: Vec::new()
        }
    }


    pub fn get_external_ip(&self) -> Vec<u8> {
        let mut zeros: [u8; 4] = [0, 0, 0, 0];

        if self.m_Socket.connected() {
            return self.m_Socket.get_ip().unwrap();
        } 
        return create_byte_array_size(&zeros, 4);
    }

    pub fn get_external_ip_string(&self) -> String {
        if self.m_Socket.connected() {
           return self.m_Socket.get_ip_string();
        } 
        return String::new();
   }
    pub fn get_packets(&self) -> VecDeque<CommandPacket> { self.m_Packets.clone() }
    pub fn get_delete_me(&self) -> bool { self.m_DeleteMe }
    pub fn get_error(&self) -> bool { self.m_Error }
    pub fn get_error_string(&self) -> String { self.m_ErrorString.clone() }
    pub fn get_join_player(&self) -> IncomingJoinPlayer { self.m_IncomingJoinPlayer.clone() }

    pub fn set_socket(&mut self, socket: TcpClient) {
        self.m_Socket = socket;
    }

    pub fn set_delete_me(&mut self, delete_me: bool) {
        self.m_DeleteMe = delete_me;
    }

    pub async fn update(&mut self) -> bool {
        if self.m_DeleteMe { return true; }

        if !self.m_Socket.connected() { return false; }

        let mut buf = [0u8; 4096];

        match self.m_Socket.do_recv(&mut buf).await {
            Ok(bytes_received) if bytes_received > 0 => {
                self.m_IncomingBuffer.extend(&buf[..bytes_received]);
                
                self.extract_packets().await;
                self.process_packets().await;
            }
            Ok(_) => { }
            Err(e) => {
            }
        }
        return self.m_DeleteMe || self.m_Error || self.m_Socket.has_error() || self.m_Socket.get_connected();
    }
    pub async fn extract_packets(&mut self) {
        while self.m_IncomingBuffer.len() >= 4 {
            let bytes = &self.m_IncomingBuffer;
            if bytes[0] == W3GS_HEADER_CONSTANT || bytes[0] == GPS_HEADER_CONSTANT {
                let mut length: u16 = byte_array_to_uint16(&bytes, false, 2);

                if length >= 4 {
                    if bytes.len() >= length as usize {
                        let packet_data = self.m_IncomingBuffer[..length as usize].to_vec();
                        self.m_Packets.push_back(CommandPacket::new(
                            bytes[0],
                            bytes[1] as i32,
                            packet_data,
                        ));

                        self.m_IncomingBuffer.drain(..length as usize);
                    } else {
                        return;
                    }
                } else {
                    self.m_Error = true;
                    self.m_ErrorString = "received invalid packet from player (bad length)".to_owned();
                    return ;
                }
            } else {
                self.m_Error = true;
                self.m_ErrorString = "received invalid packet from player (bad header constant)".to_owned();
                return ;
            }
        }
    }
    pub async fn process_packets(&mut self) {
        while let Some(packet) = self.m_Packets.pop_front() {
            if packet.get_packet_type() == W3GS_HEADER_CONSTANT {
                let packet_type: i32 = packet.get_id();
    
                match packet_type {
                    x if x == ProtocolG::W3GS_REQJOIN as i32 => {
                        if let Some(join_player) = self.m_Protocol.RECEIVE_W3GS_REQJOIN(packet.get_data().clone()) {
                            self.m_IncomingJoinPlayer = join_player.clone();
                            let join_player = &self.m_IncomingJoinPlayer;
                            let potential = self.clone();
                            let game = &mut self.m_Game;
                            game.event_player_joined(potential, join_player).await;
                            return;
                        }
                        
                        
                    }
                    _ => {}
                }
            }
        }
    }
    
    

    pub async fn send(&mut self, data: ByteArray) {
        if self.m_Socket.connected() {
            let _ = self.m_Socket.do_send(&data).await;
        }
    }
}

#[derive(Clone, Debug)]
pub struct GamePlayer {
    m_protocol: GameProtocol,
    m_game: Option<Game>, // Assuming Game is defined in gameprotocol or elsewhere
    m_socket: TcpClient,
    m_packets: VecDeque<CommandPacket>,
    m_delete_me: bool,
    m_error: bool,
    m_error_string: String,
    m_incoming_join_player: IncomingJoinPlayer,
    m_pid: u8,
    m_name: String,
    m_internal_ip: ByteArray,
    m_pings: Vec<u32>,

    m_check_sums: VecDeque<u32>,
    m_left_reason: String,
    m_spoofed_realm: String,
    m_joined_realm: String,
    m_total_packets_sent: u32,
    m_total_packets_received: u32,
    m_left_code: u32,
    m_login_attempts: u32,
    m_sync_counter: u32,
    m_join_time: u32,
    m_last_map_part_sent: u32,
    m_last_map_part_acked: u32,
    m_started_downloading_ticks: u32,
    m_finished_downloading_time: u32,
    m_finished_loading_ticks: u32,
    m_started_lagging_ticks: u32,
    m_stats_sent_time: u32,
    m_stats_dota_sent_time: u32,
    m_last_gproxy_wait_notice_sent_time: u32,
    m_load_in_game_data: VecDeque<ByteArray>,
    m_score: f64,
    m_logged_in: bool,
    m_spoofed: bool,
    m_reserved: bool,
    m_whois_should_be_sent: bool,
    m_whois_sent: bool,
    m_download_allowed: bool,
    m_download_started: bool,
    m_download_finished: bool,
    m_finished_loading: bool,
    m_lagging: bool,
    m_drop_vote: bool,
    m_kick_vote: bool,
    m_muted: bool,
    m_left_message_sent: bool,
    m_gproxy: bool,
    m_gproxy_disconnect_notice_sent: bool,
    m_gproxy_buffer: VecDeque<ByteArray>,
    m_gproxy_reconnect_key: u32,
    m_last_gproxy_ack_time: u32,
}

impl GamePlayer {
    pub fn new(
        protocol: GameProtocol,
        game: Option<Game>,
        socket: TcpClient,
        pid: u8,
        joined_realm: String,
        name: String,
        internal_ip: ByteArray,
        reserved: bool,
    ) -> Self {
        GamePlayer {
            m_protocol: protocol,
            m_game: game,
            m_socket: socket,
            m_packets: VecDeque::new(),
            m_delete_me: false,
            m_error: false,
            m_error_string: String::new(),
            m_incoming_join_player: IncomingJoinPlayer::new(0, 0, String::new(), ByteArray::new()),
            m_pid: pid,
            m_name: name,
            m_internal_ip: internal_ip,
            m_pings: Vec::new(),
            m_check_sums: VecDeque::new(),
            m_left_reason: String::new(),
            m_spoofed_realm: String::new(),
            m_joined_realm: joined_realm,
            m_total_packets_sent: 0,
            m_total_packets_received: 0,
            m_left_code: 0,
            m_login_attempts: 0,
            m_sync_counter: 0,
            m_join_time: 0,
            m_last_map_part_sent: 0,
            m_last_map_part_acked: 0,
            m_started_downloading_ticks: 0,
            m_finished_downloading_time: 0,
            m_finished_loading_ticks: 0,
            m_started_lagging_ticks: 0,
            m_stats_sent_time: 0,
            m_stats_dota_sent_time: 0,
            m_last_gproxy_wait_notice_sent_time: 0,
            m_load_in_game_data: VecDeque::new(),
            m_score: 0.0,
            m_logged_in: false,
            m_spoofed: false,
            m_reserved: reserved,
            m_whois_should_be_sent: false,
            m_whois_sent: false,
            m_download_allowed: false,
            m_download_started: false,
            m_download_finished: false,
            m_finished_loading: false,
            m_lagging: false,
            m_drop_vote: false,
            m_kick_vote: false,
            m_muted: false,
            m_left_message_sent: false,
            m_gproxy: false,
            m_gproxy_disconnect_notice_sent: false,
            m_gproxy_buffer: VecDeque::new(),
            m_gproxy_reconnect_key: 0,
            m_last_gproxy_ack_time: 0,
        }
    }

    pub fn new_from_potential(
        potential: PotentialPlayer,
        pid: u8,
        joined_realm: String,
        name: String,
        internal_ip: ByteArray,
        reserved: bool,
    ) -> Self {
        GamePlayer {
            m_protocol: potential.m_Protocol,
            m_game: None, // Assuming no game reference in PotentialPlayer; adjust if needed
            m_socket: potential.m_Socket,
            m_packets: potential.m_Packets,
            m_delete_me: potential.m_DeleteMe,
            m_error: potential.m_Error,
            m_error_string: potential.m_ErrorString,
            m_incoming_join_player: potential.m_IncomingJoinPlayer,
            m_pid: pid,
            m_name: name,
            m_internal_ip: internal_ip,
            m_pings: Vec::new(),
            m_check_sums: VecDeque::new(),
            m_left_reason: String::new(),
            m_spoofed_realm: String::new(),
            m_joined_realm: joined_realm,
            m_total_packets_sent: 0,
            m_total_packets_received: 0,
            m_left_code: 0,
            m_login_attempts: 0,
            m_sync_counter: 0,
            m_join_time: 0,
            m_last_map_part_sent: 0,
            m_last_map_part_acked: 0,
            m_started_downloading_ticks: 0,
            m_finished_downloading_time: 0,
            m_finished_loading_ticks: 0,
            m_started_lagging_ticks: 0,
            m_stats_sent_time: 0,
            m_stats_dota_sent_time: 0,
            m_last_gproxy_wait_notice_sent_time: 0,
            m_load_in_game_data: VecDeque::new(),
            m_score: 0.0,
            m_logged_in: false,
            m_spoofed: false,
            m_reserved: reserved,
            m_whois_should_be_sent: false,
            m_whois_sent: false,
            m_download_allowed: false,
            m_download_started: false,
            m_download_finished: false,
            m_finished_loading: false,
            m_lagging: false,
            m_drop_vote: false,
            m_kick_vote: false,
            m_muted: false,
            m_left_message_sent: false,
            m_gproxy: false,
            m_gproxy_disconnect_notice_sent: false,
            m_gproxy_buffer: VecDeque::new(),
            m_gproxy_reconnect_key: 0,
            m_last_gproxy_ack_time: 0,
        }
    }
    pub fn get_external_ip(&self) -> Vec<u8> {
        let mut zeros: [u8; 4] = [0, 0, 0, 0];

        if self.m_socket.connected() {
            return self.m_socket.get_ip().unwrap();
        } 
        return create_byte_array_size(&zeros, 4);
    }
    
    pub fn get_external_ip_string(&self) -> String {
        if self.m_socket.connected() {
           return self.m_socket.get_ip_string();
        } 
        return String::new();
   }
    pub fn get_socket(&self) -> TcpClient { self.m_socket.clone() }
    pub fn get_pid(&self) -> u8 { self.m_pid }
    pub fn get_name(&self) -> String { self.m_name.clone() }
    pub fn get_internal_ip(&self) -> ByteArray { self.m_internal_ip.clone() }
    pub fn get_num_pings(&self) -> usize { self.m_pings.len() }
    pub fn get_num_check_sums(&self) -> usize { self.m_check_sums.len() }
    pub fn get_check_sums(&self) -> VecDeque<u32> { self.m_check_sums.clone() }
    pub fn get_left_reason(&self) -> String { self.m_left_reason.clone() }
    pub fn get_spoofed_realm(&self) -> String { self.m_spoofed_realm.clone() }
    pub fn get_joined_realm(&self) -> String { self.m_joined_realm.clone() }
    pub fn get_left_code(&self) -> u32 { self.m_left_code }
    pub fn get_login_attempts(&self) -> u32 { self.m_login_attempts }
    pub fn get_sync_counter(&self) -> u32 { self.m_sync_counter }
    pub fn get_join_time(&self) -> u32 { self.m_join_time }
    pub fn get_last_map_part_sent(&self) -> u32 { self.m_last_map_part_sent }
    pub fn get_last_map_part_acked(&self) -> u32 { self.m_last_map_part_acked }
    pub fn get_started_downloading_ticks(&self) -> u32 { self.m_started_downloading_ticks }
    pub fn get_finished_downloading_time(&self) -> u32 { self.m_finished_downloading_time }
    pub fn get_finished_loading_ticks(&self) -> u32 { self.m_finished_loading_ticks }
    pub fn get_started_lagging_ticks(&self) -> u32 { self.m_started_lagging_ticks }
    pub fn get_stats_sent_time(&self) -> u32 { self.m_stats_sent_time }
    pub fn get_stats_dota_sent_time(&self) -> u32 { self.m_stats_dota_sent_time }
    pub fn get_last_gproxy_wait_notice_sent_time(&self) -> u32 { self.m_last_gproxy_wait_notice_sent_time }
    pub fn get_load_in_game_data(&self) -> VecDeque<ByteArray> { self.m_load_in_game_data.clone() }
    pub fn get_score(&self) -> f64 { self.m_score }
    pub fn get_logged_in(&self) -> bool { self.m_logged_in }
    pub fn get_spoofed(&self) -> bool { self.m_spoofed }
    pub fn get_reserved(&self) -> bool { self.m_reserved }
    pub fn get_whois_should_be_sent(&self) -> bool { self.m_whois_should_be_sent }
    pub fn get_whois_sent(&self) -> bool { self.m_whois_sent }
    pub fn get_download_allowed(&self) -> bool { self.m_download_allowed }
    pub fn get_download_started(&self) -> bool { self.m_download_started }
    pub fn get_download_finished(&self) -> bool { self.m_download_finished }
    pub fn get_finished_loading(&self) -> bool { self.m_finished_loading }
    pub fn get_lagging(&self) -> bool { self.m_lagging }
    pub fn get_drop_vote(&self) -> bool { self.m_drop_vote }
    pub fn get_kick_vote(&self) -> bool { self.m_kick_vote }
    pub fn get_muted(&self) -> bool { self.m_muted }
    pub fn get_left_message_sent(&self) -> bool { self.m_left_message_sent }
    pub fn get_gproxy(&self) -> bool { self.m_gproxy }
    pub fn get_gproxy_disconnect_notice_sent(&self) -> bool { self.m_gproxy_disconnect_notice_sent }
    pub fn get_gproxy_reconnect_key(&self) -> u32 { self.m_gproxy_reconnect_key }
    pub fn get_error_string(&self) -> String { self.m_error_string.clone() }
    pub fn get_delete_me(&self) -> bool {self.m_delete_me}

    pub fn set_delete_me(&mut self, delete: bool) { self.m_delete_me = delete; }
    pub fn set_left_reason(&mut self, left_reason: String) { self.m_left_reason = left_reason; }
    pub fn set_spoofed_realm(&mut self, spoofed_realm: String) { self.m_spoofed_realm = spoofed_realm; }
    pub fn set_left_code(&mut self, left_code: u32) { self.m_left_code = left_code; }
    pub fn set_login_attempts(&mut self, login_attempts: u32) { self.m_login_attempts = login_attempts; }
    pub fn set_sync_counter(&mut self, sync_counter: u32) { self.m_sync_counter = sync_counter; }
    pub fn set_last_map_part_sent(&mut self, last_map_part_sent: u32) { self.m_last_map_part_sent = last_map_part_sent; }
    pub fn set_last_map_part_acked(&mut self, last_map_part_acked: u32) { self.m_last_map_part_acked = last_map_part_acked; }
    pub fn set_started_downloading_ticks(&mut self, started_downloading_ticks: u32) { self.m_started_downloading_ticks = started_downloading_ticks; }
    pub fn set_finished_downloading_time(&mut self, finished_downloading_time: u32) { self.m_finished_downloading_time = finished_downloading_time; }
    pub fn set_started_lagging_ticks(&mut self, started_lagging_ticks: u32) { self.m_started_lagging_ticks = started_lagging_ticks; }
    pub fn set_stats_sent_time(&mut self, stats_sent_time: u32) { self.m_stats_sent_time = stats_sent_time; }
    pub fn set_stats_dota_sent_time(&mut self, stats_dota_sent_time: u32) { self.m_stats_dota_sent_time = stats_dota_sent_time; }
    pub fn set_last_gproxy_wait_notice_sent_time(&mut self, last_gproxy_wait_notice_sent_time: u32) { self.m_last_gproxy_wait_notice_sent_time = last_gproxy_wait_notice_sent_time; }
    pub fn set_score(&mut self, score: f64) { self.m_score = score; }
    pub fn set_logged_in(&mut self, logged_in: bool) { self.m_logged_in = logged_in; }
    pub fn set_spoofed(&mut self, spoofed: bool) { self.m_spoofed = spoofed; }
    pub fn set_reserved(&mut self, reserved: bool) { self.m_reserved = reserved; }
    pub fn set_whois_should_be_sent(&mut self, whois_should_be_sent: bool) { self.m_whois_should_be_sent = whois_should_be_sent; }
    pub fn set_download_allowed(&mut self, download_allowed: bool) { self.m_download_allowed = download_allowed; }
    pub fn set_download_started(&mut self, download_started: bool) { self.m_download_started = download_started; }
    pub fn set_download_finished(&mut self, download_finished: bool) { self.m_download_finished = download_finished; }
    pub fn set_lagging(&mut self, lagging: bool) { self.m_lagging = lagging; }
    pub fn set_drop_vote(&mut self, drop_vote: bool) { self.m_drop_vote = drop_vote; }
    pub fn set_kick_vote(&mut self, kick_vote: bool) { self.m_kick_vote = kick_vote; }
    pub fn set_muted(&mut self, muted: bool) { self.m_muted = muted; }
    pub fn set_left_message_sent(&mut self, left_message_sent: bool) { self.m_left_message_sent = left_message_sent; }
    pub fn set_gproxy_disconnect_notice_sent(&mut self, gproxy_disconnect_notice_sent: bool) { self.m_gproxy_disconnect_notice_sent = gproxy_disconnect_notice_sent; }

    pub fn get_name_terminated(name: &str) -> String {
        let lower_name = name.to_lowercase();
        let start = lower_name.find("|c");
        let end = lower_name.find("|r");
    
        if let Some(start_pos) = start {
            if end.is_none() || end.unwrap() < start_pos {
                return format!("{}|r", name);
            }
        }
    
        name.to_string()
    }
    

    pub fn get_ping(&self, _lcping: bool) -> u32 {
        if self.m_pings.is_empty() {
            return 0;
        }
        let mut avg_ping = 0;

        for i in 0..self.m_pings.len() {
            avg_ping += self.m_pings[i];
        }

        avg_ping /= self.m_pings.len() as u32;

        if _lcping { return avg_ping / 2; }
        else { return avg_ping; }

    }

    pub fn add_load_in_game_data(&mut self, load_in_game_data: ByteArray) {
    }

    pub async fn update(&self) -> bool {
        true
    }

    pub async fn extract_packets(&mut self) {
    }

    pub async fn process_packets(&mut self) {
    }

    pub async fn send(&mut self, data: ByteArray) {
    }

    pub fn event_gproxy_reconnect(&mut self, _new_socket: TcpClient, _last_packet: u32) {
    }
}