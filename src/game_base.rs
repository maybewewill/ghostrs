use crate::game::Game;
use crate::gameplayer::*;
use crate::gameprotocol::*;
use crate::ghost::*;
use crate::gameslot::*;
use crate::lang::Language;
use crate::map::*;
use crate::socket::*;
use crate::util::create_byte_array;
use crate::util::create_byte_array_from_u16;
use crate::util::{create_byte_array_from_u32, byte_array_to_uint32};
use crate::logger::*;
use byteorder::ReadBytesExt;
use rand::seq::SliceRandom;
use rand::rng;
use uuid::Uuid;
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::num;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::{Arc, Mutex};
use once_cell::sync::Lazy;
use std::ptr;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::RwLock;

pub static POTENTIALS: Lazy<RwLock<Vec<PotentialPlayer>>> = Lazy::new(|| RwLock::new(Vec::new()));

#[derive(Clone)]
#[derive(Debug)]
#[derive(Default)]
pub struct BaseGame {
   pub  m_language: Language,
    pub m_ghost: Arc<Mutex<Ghost>>, // Assuming Ghost is defined elsewhere
    pub m_socket: TcpServer, // Assuming TcpServer is defined in socket
    pub m_protocol: GameProtocol,
    pub m_slots: Vec<GameSlot>,
   pub  m_players: Vec<GamePlayer>,// Assuming CallableScoreCheck is defined
   pub  m_actions: VecDeque<CIncomingAction>, // Assuming IncomingAction is defined
    pub m_reserved: Vec<String>,
    pub m_ignored_names: HashSet<String>,
   pub  m_ip_black_list: HashSet<String>,
   pub  m_enforce_slots: Vec<GameSlot>,
   pub m_potentials: Vec<PotentialPlayer>,// Assuming PidPlayer is defined
   pub  m_map: Map, // Assuming Map is defined
    pub m_exiting: bool,
    pub m_saving: bool,
    pub m_host_port: u16,
    pub m_game_state: u8,
    pub m_virtual_host_pid: u8,
    pub m_fake_player_pid: u8,
    pub m_gproxy_empty_actions: u8,
    pub m_game_name: String,
    pub m_last_game_name: String,
    pub m_virtual_host_name: String,
    pub m_owner_name: String,
    pub m_creator_name: String,
    pub m_creator_server: String,
    pub m_announce_message: String,
    pub m_stat_string: String,
    pub m_kick_vote_player: String,
    pub m_hcl_command_string: String,
    pub m_random_seed: u32,
    pub m_host_counter: u32,
    pub m_entry_key: u32,
    pub m_latency: u32,
    pub m_sync_limit: u32,
    pub m_sync_counter: u32,
    pub m_game_ticks: u32,
    pub m_creation_time: u32,
    pub m_last_ping_time: u32,
    pub m_last_refresh_time: u32,
    pub m_last_download_ticks: u32,
    pub m_download_counter: u32,
    pub m_last_download_counter_reset_ticks: u32,
    pub m_last_announce_time: u32,
    pub m_announce_interval: u32,
    pub m_last_auto_start_time: u32,
    pub m_auto_start_players: u32,
    pub m_last_count_down_ticks: u32,
    pub m_count_down_counter: u32,
    pub m_started_loading_ticks: u32,
    pub m_start_players: u32,
    pub m_last_lag_screen_reset_time: u32,
    pub m_last_action_sent_ticks: u32,
    pub m_last_action_late_by: u32,
    pub m_started_lagging_time: u32,
    pub m_last_lag_screen_time: u32,
    pub m_last_reserved_seen: u32,
    pub m_started_kick_vote_time: u32,
    pub m_game_over_time: u32,
    pub m_last_player_leave_ticks: u32,
    pub m_minimum_score: f64,
    pub m_maximum_score: f64,
    pub m_slot_info_changed: bool,
    pub m_locked: bool,
    pub m_refresh_messages: bool,
    pub m_refresh_error: bool,
    pub m_refresh_rehosted: bool,
    pub m_mute_all: bool,
    pub m_mute_lobby: bool,
    pub m_count_down_started: bool,
    pub m_game_loading: bool,
    pub m_game_loaded: bool,
    pub m_load_in_game: bool,
    pub m_lagging: bool,
    pub m_auto_save: bool,
    pub m_match_making: bool,
    pub m_local_admin_messages: bool,
    pub m_inited: bool
}

impl BaseGame {
    pub fn new(
        ghost: Arc<Mutex<Ghost>>,
        map: Map,
        host_port: u16,
        game_state: u8,
        game_name: String,
        owner_name: String,
        creator_name: String,
        creator_server: String,
    ) -> Self {
        BaseGame {
            m_language: Language::new(),
            m_ghost: ghost.clone(),
            m_socket: TcpServer::new(), // Assuming TcpServer has a new() method
            m_protocol: GameProtocol::new(ghost), // Assuming GameProtocol has a new() method
            m_slots: Vec::new(),
            m_players: Vec::new(),
            m_actions: VecDeque::new(),
            m_potentials: Vec::new(),
            m_reserved: Vec::new(),
            m_ignored_names: HashSet::new(),
            m_ip_black_list: HashSet::new(),
            m_enforce_slots: Vec::new(),
            m_map: map,

            m_exiting: false,
            m_saving: false,
            m_host_port: host_port,
            m_game_state: game_state,
            m_virtual_host_pid: 0,
            m_fake_player_pid: 0,
            m_gproxy_empty_actions: 0,
            m_game_name: game_name,
            m_last_game_name: String::new(),
            m_virtual_host_name: String::new(),
            m_owner_name: owner_name,
            m_creator_name: creator_name,
            m_creator_server: creator_server,
            m_announce_message: String::new(),
            m_stat_string: String::new(),
            m_kick_vote_player: String::new(),
            m_hcl_command_string: String::new(),
            m_random_seed: 0,
            m_host_counter: 0,
            m_entry_key: 0,
            m_latency: 0,
            m_sync_limit: 0,
            m_sync_counter: 0,
            m_game_ticks: 0,
            m_creation_time: 0,
            m_last_ping_time: 0,
            m_last_refresh_time: 0,
            m_last_download_ticks: 0,
            m_download_counter: 0,
            m_last_download_counter_reset_ticks: 0,
            m_last_announce_time: 0,
            m_announce_interval: 0,
            m_last_auto_start_time: 0,
            m_auto_start_players: 0,
            m_last_count_down_ticks: 0,
            m_count_down_counter: 0,
            m_started_loading_ticks: 0,
            m_start_players: 0,
            m_last_lag_screen_reset_time: 0,
            m_last_action_sent_ticks: 0,
            m_last_action_late_by: 0,
            m_started_lagging_time: 0,
            m_last_lag_screen_time: 0,
            m_last_reserved_seen: 0,
            m_started_kick_vote_time: 0,
            m_game_over_time: 0,
            m_last_player_leave_ticks: 0,
            m_minimum_score: 0.0,
            m_maximum_score: 0.0,
            m_slot_info_changed: false,
            m_locked: false,
            m_refresh_messages: false,
            m_refresh_error: false,
            m_refresh_rehosted: false,
            m_mute_all: false,
            m_mute_lobby: false,
            m_count_down_started: false,
            m_game_loading: false,
            m_game_loaded: false,
            m_load_in_game: false,
            m_lagging: false,
            m_auto_save: false,
            m_match_making: false,
            m_local_admin_messages: false,
            m_inited: false
        }
    }

    pub async fn init(&mut self) {
        let mut ghost = self.m_ghost.lock().unwrap();
        self.m_socket = TcpServer::new();
        
        if ghost.m_ReconnectWaitTime != 0 {
            self.m_gproxy_empty_actions = (ghost.m_ReconnectWaitTime - 1) as u8;
            if self.m_gproxy_empty_actions > 9 {
                self.m_gproxy_empty_actions = 9;
            }
        }

        self.m_slots = self.m_map.get_slots();
        
        if !ghost.m_BindAddress.is_empty() {
            log_info(&format!("[GAME: {}] attempting to bind to address [{}]", self.m_game_name, ghost.m_BindAddress));
        }

        match self.m_socket.bind(&ghost.m_BindAddress, self.m_host_port).await {
            Ok(_) => {
                
                log_info(&format!("[GAME: {}] binding to address [{}]", self.m_game_name, self.m_host_port));
            }
            Err(e) => {
                log_error(&format!("[GAME: {}] error binding to address [{}]: {}", self.m_game_name, self.m_host_port, e));
                self.m_exiting = true;
                return;
            }
        }
        //println!("{:?}", self.m_socket);
        
        self.m_inited = true;
    }

    pub fn get_enforce_slots(&self) -> Vec<GameSlot> { self.m_enforce_slots.clone() }
    pub fn get_host_port(&self) -> u16 { self.m_host_port }
    pub fn get_game_state(&self) -> u8 { self.m_game_state }
    pub fn get_gproxy_empty_actions(&self) -> u8 { self.m_gproxy_empty_actions }
    pub fn get_game_name(&self) -> String { self.m_game_name.clone() }
    pub fn get_last_game_name(&self) -> String { self.m_last_game_name.clone() }
    pub fn get_virtual_host_name(&self) -> String { self.m_virtual_host_name.clone() }
    pub fn get_owner_name(&self) -> String { self.m_owner_name.clone() }
    pub fn get_creator_name(&self) -> String { self.m_creator_name.clone() }
    pub fn get_creator_server(&self) -> String { self.m_creator_server.clone() }
    pub fn get_host_counter(&self) -> u32 { self.m_host_counter }
    pub fn get_last_lag_screen_time(&self) -> u32 { self.m_last_lag_screen_time }
    pub fn get_locked(&self) -> bool { self.m_locked }
    pub fn get_refresh_messages(&self) -> bool { self.m_refresh_messages }
    pub fn get_count_down_started(&self) -> bool { self.m_count_down_started }
    pub fn get_game_loading(&self) -> bool { self.m_game_loading }
    pub fn get_game_loaded(&self) -> bool { self.m_game_loaded }
    pub fn get_lagging(&self) -> bool { self.m_lagging }

    pub fn set_enforce_slots(&mut self, enforce_slots: Vec<GameSlot>) { self.m_enforce_slots = enforce_slots; }
    pub fn set_exiting(&mut self, exiting: bool) { self.m_exiting = exiting; }
    pub fn set_auto_start_players(&mut self, auto_start_players: u32) { self.m_auto_start_players = auto_start_players; }
    pub fn set_minimum_score(&mut self, minimum_score: f64) { self.m_minimum_score = minimum_score; }
    pub fn set_maximum_score(&mut self, maximum_score: f64) { self.m_maximum_score = maximum_score; }
    pub fn set_refresh_error(&mut self, refresh_error: bool) { self.m_refresh_error = refresh_error; }
    pub fn set_match_making(&mut self, match_making: bool) { self.m_match_making = match_making; }

    pub fn get_next_timed_action_ticks(&self) -> u32 {
        if !self.m_game_loaded || self.m_lagging {
            return 50;
        }

        let ticks_since_last_update = (get_ticks() as u32 - self.m_last_action_sent_ticks);

        if ticks_since_last_update > self.m_latency - self.m_last_action_late_by {
            return 0;
        } else {
            return self.m_latency - self.m_last_action_late_by - ticks_since_last_update;
        }
    }

    pub fn get_slots_occupied(&self) -> u32 {

        let mut num_slots_open = 0;

        for i in self.m_slots.clone() {
            if i.slot_status() == SLOTSTATUS_OCCUPIED {
                num_slots_open += 1;
            }
        }
        num_slots_open
    }

    pub fn get_slots_open(&self) -> u32 {

        let mut num_slots_open = 0;

        for i in self.m_slots.clone() {
            if i.slot_status() == SLOTSTATUS_OPEN {
                num_slots_open += 1;
            }
        }
        num_slots_open
    }
    pub fn get_num_players(&self) -> u32 {
        let mut num_players = self.get_num_human_players();

        if self.m_fake_player_pid != 255 {
            num_players += 1;
        }
        num_players
    }
    pub fn get_num_human_players(&self) -> u32 {
        let mut num_human_player = 0;
        for i in self.m_players.clone() {
            if !i.get_left_message_sent() {
                num_human_player += 1;
            }
        }
        num_human_player
        
    }
    pub fn get_description(&mut self) -> String {
        let mut description = format!(
            "{} : {} : {}/{}",
            self.m_game_name,
            self.m_owner_name,
            self.get_num_human_players(),
            if self.m_game_loading || self.m_game_loaded {
                self.m_start_players
            } else {
                self.m_slots.len() as u32
            }
        );
    
        let minutes = if self.m_game_loading || self.m_game_loaded {
            (self.m_game_ticks / 1000) / 60
        } else {
            (get_time() - self.m_creation_time) / 60
        };
    
        description += &format!(" : {}m", minutes);
        description
    }
    

    pub fn set_announce(&mut self, interval: u32, message: String) {
        self.m_announce_interval = interval;
        self.m_announce_message = message;
        self.m_last_announce_time = get_time();
    }

    pub async fn update(&mut self) -> bool {
        let mut indices_to_remove = Vec::new();
        let mut players_to_delete = Vec::new();

        for (index, player) in self.m_players.iter_mut().enumerate() {
            if player.update().await {
                indices_to_remove.push(index);
            }
        }

        // Collect players to delete after the loop to avoid double mutable borrow
        for &index in &indices_to_remove {
            if let Some(player) = self.m_players.get_mut(index) {
                players_to_delete.push(player.clone());
            }
        }
        for mut player in players_to_delete {
            self.event_player_deleted(&mut player).await;
        }

        for index in indices_to_remove.iter().rev() {
            self.m_players.remove(*index);
        }
        indices_to_remove.clear();

        // Clone the potentials to avoid holding the lock during await

        for player in self.m_potentials.iter_mut() {
            if player.update().await {
                if player.m_Socket.connected() {
                    player.m_Socket.do_send_buff().await;
                }
            }
        }

        for index in indices_to_remove.iter().rev() {
            self.m_potentials.remove(*index);
        }
        
        if !self.m_game_loading && !self.m_game_loaded && self.get_num_players() < 12 {
            self.create_virtual_host().await;
        }

        if self.m_locked && self.get_player_from_name(self.m_owner_name.clone(), false).is_some() {
            self.send_all_chat(self.m_language.game_unlocked()).await;
            self.m_locked = false;
        }
        
        let current_time_sec = get_time();
        let current_ticks_ms = get_ticks() as u32;

        if get_time() as u32 - self.m_last_ping_time >= 3 {
            self.send_all(self.m_protocol.SEND_W3GS_PING_FROM_HOST()).await;

            if !self.m_count_down_started {
                let fixed_host_counter = self.m_host_counter & 0x0FFFFFFF;

                // TODO: broadcast to LAN .....

            }
            self.m_last_ping_time = get_time() as u32;
        }
        
        // println!("| m_RefreshError = {} |---| m_CountDownstarted = {} |---| m_GameState = {} |---| slots_open: {} |---| get_time - m_last_refresh = {}",
        //     self.m_refresh_error,
        //     self.m_count_down_started,
        //     self.m_game_state,
        //     self.get_slots_open(),
        //     get_time() - self.m_last_refresh_time
        // );
        if !self.m_refresh_error && !self.m_count_down_started && self.m_game_state == GAME_PUBLIC && self.get_slots_open() > 0 && get_time() - self.m_last_refresh_time >= 3 {
            let mut refreshed = false;
            let bnets = m_BNETs.read().await;

            for bnet_arc in bnets.iter() {
                let mut bnet = bnet_arc.lock().await;

                if bnet.get_out_packets_queued() <= 1 {
                    bnet.queue_game_refresh(
                        self.m_game_state,
                        self.m_game_name.clone(),
                        "BOT".to_owned(),
                        &mut self.m_map,
                        get_time() as u32 - self.m_creation_time,
                        self.m_host_counter,
                    ).await;

                    refreshed = true;
                }
            }


            if self.m_refresh_messages && refreshed {
                self.send_all_chat(self.m_language.game_refreshed()).await;
            }
        }


        if !self.m_game_loading && !self.m_game_loaded && get_ticks() as u32 - self.m_last_download_counter_reset_ticks >= 500 {
            if self.m_slot_info_changed {
                self.send_all_slot_info_s().await;
            }
            self.m_download_counter = 0;
            self.m_last_download_counter_reset_ticks = get_ticks() as u32;
        }

        if !self.m_game_loading && !self.m_game_loaded && get_ticks() as u32 - self.m_last_download_ticks >= 100 {
            let mut downloaders: u32 = 0;
            let mut players = self.m_players.clone();
            for i in players.iter_mut() {
                if i.get_download_started() && !i.get_download_finished() {
                    downloaders += 1;
                    
                    let mut map_size : u32 = byte_array_to_uint32(&self.m_map.get_map_size(), false, 0);
                    while i.get_last_map_part_sent() < i.get_last_map_part_acked() + 1442 * 100 && i.get_last_map_part_sent() < map_size {
                        if i.get_last_map_part_sent() == 0 {
                            i.set_started_downloading_ticks(get_ticks() as u32);
                        }

                        self.send(i, self.m_protocol.SEND_W3GS_MAPPART( self.get_host_pid(), i.get_pid(), i.get_last_map_part_sent(), &self.m_map.get_map_data())).await;
                        i.set_last_map_part_sent(i.get_last_map_part_sent() + 1442);
                        self.m_download_counter += 1442;
                    } 
                }
            }
            self.m_last_download_ticks = get_ticks() as u32;
        }

        if !self.m_announce_message.is_empty() && !self.m_count_down_started && get_time() as u32 - self.m_last_announce_time >= self.m_announce_interval {
            self.send_all_chat(self.m_announce_message.clone()).await;
            self.m_last_announce_time = get_time() as u32;
        }

        if self.m_count_down_started && get_ticks() as u32 - self.m_last_count_down_ticks >= 10000 {
            if self.m_count_down_counter > 0 {
                self.send_all_chat(format!("{}. . .", self.m_count_down_counter)).await;
                self.m_count_down_counter -= 1;
            } else if !self.m_game_loading && !self.m_game_loaded {
                self.event_game_started().await;
            }
        }

        if self.m_game_loading {
            let mut finished_loading: bool = true;
            let mut players = self.m_players.clone();

            for i in players.iter_mut() {
                finished_loading = i.get_finished_loading();

                if !finished_loading {
                    break;
                }
            }

            if finished_loading {
                self.m_last_action_sent_ticks = get_ticks() as u32;
                self.m_game_loading = false;
                self.m_game_loaded = true;
                self.event_game_loaded().await;
            } else {
                if self.m_load_in_game && get_time() as u32 - self.m_last_lag_screen_reset_time >= 30 {
                    let mut using_gproxy = false;
                    let mut players = self.m_players.clone();
                    for i in players.iter_mut() {
                        if i.get_gproxy() {
                            using_gproxy = true;
                        }
                    }

                    for i in players.clone().iter_mut() {
                        if i.get_finished_loading() {
                            for j in players.iter_mut() {
                                if !j.get_finished_loading() {
                                    self.send(i, self.m_protocol.SEND_W3GS_STOP_LAG(i, true)).await;
                                }

                                if using_gproxy && !i.get_gproxy() {
                                    for _ in 0..self.m_gproxy_empty_actions {
                                        self.send(i, self.m_protocol.SEND_W3GS_INCOMING_ACTION(VecDeque::<CIncomingAction>::new(), 0)).await;
                                    }
                                }
                            }
                            self.send(i, self.m_protocol.SEND_W3GS_INCOMING_ACTION(VecDeque::<CIncomingAction>::new(), 0)).await;
                            self.send(i, self.m_protocol.SEND_W3GS_START_LAG(players.clone(), true)).await;
                        } else {
                            if using_gproxy && !i.get_gproxy() {
                                for _ in 0..self.m_gproxy_empty_actions {
                                    i.add_load_in_game_data(self.m_protocol.SEND_W3GS_INCOMING_ACTION(VecDeque::<CIncomingAction>::new(), 0));
                                }
                            }
                            i.add_load_in_game_data(self.m_protocol.SEND_W3GS_INCOMING_ACTION(VecDeque::<CIncomingAction>::new(), 0));
                        }
                    }
                    self.m_last_lag_screen_reset_time = get_time();
                }
            }
        }

        if self.m_game_loaded {
                        // В CBaseGame::update, ВНУТРИ if self.m_game_loaded { ... }

            if self.m_lagging {
                const LAG_KICK_WAIT_TIME_SECONDS: u32 = 60; // из конфига
                const LAG_SCREEN_RESET_INTERVAL_SECONDS: u32 = 30; // из конфига
                let current_time_sec = get_time();
                let current_ticks_ms = get_ticks() as u32;

                // Кик по таймауту лага
                if current_time_sec.saturating_sub(self.m_started_lagging_time) >= LAG_KICK_WAIT_TIME_SECONDS {
                    self.stop_laggers(Language::new().auto_kicked_after_seconds(&LAG_KICK_WAIT_TIME_SECONDS.to_string()));
                    // После stop_laggers нужно проверить, остались ли еще лагающие,
                    // и если нет - установить self.m_lagging = false;
                    let mut any_still_truly_lagging = false;
                    for p_check in self.m_players.iter() {
                        if p_check.get_lagging() && !p_check.get_delete_me() {
                            any_still_truly_lagging = true;
                            break;
                        }
                    }
                    if !any_still_truly_lagging {
                        self.m_lagging = false;
                        log_info(&format!("[GAME: {}] Lag screen deactivated after kicking laggers.", self.m_game_name));
                    }
                }

                // Периодический "сброс" лагскрина И ОТПРАВКА ТИКА НЕЛАГАЮЩИМ
                if current_time_sec.saturating_sub(self.m_last_lag_screen_reset_time) >= LAG_SCREEN_RESET_INTERVAL_SECONDS {
                    log_info(&format!("[GAME: {}] Resetting lag screen.", self.m_game_name));

                    let mut non_lagger_pids_to_send_tick: Vec<u8> = Vec::new();
                    let mut laggers_for_stop_start_lag: Vec<GamePlayer> = Vec::new(); // Клоны для SEND_..._LAG

                    for p_idx in 0..self.m_players.len() {
                        if !self.m_players[p_idx].get_delete_me() {
                            if self.m_players[p_idx].get_lagging() {
                                laggers_for_stop_start_lag.push(self.m_players[p_idx].clone());
                            } else {
                                non_lagger_pids_to_send_tick.push(self.m_players[p_idx].get_pid());
                            }
                        }
                    }
                    
                    // 1. Отправить STOP_LAG для актуальных лаггеров всем НЕлагающим.
                    //    Если нелагающих нет, этот шаг можно пропустить.
                    if !non_lagger_pids_to_send_tick.is_empty() {
                        for lagger_clone in laggers_for_stop_start_lag.iter() {
                            let stop_lag_packet = self.m_protocol.SEND_W3GS_STOP_LAG(lagger_clone, false);
                            for nl_pid in &non_lagger_pids_to_send_tick {
                                if let Some(nl_player) = self.get_player_from_pid_mut(*nl_pid) {
                                    // Предполагаем, что GamePlayer::send(&mut self, data)
                                    nl_player.send(stop_lag_packet.clone()).await;
                                }
                            }
                        }
                    }

                    // 2. Отправить "пустой" INCOMING_ACTION НЕлагающим.
                    // Это инкрементирует self.m_sync_counter игры и дает шанс лагающим догнать.
                    let time_passed = current_ticks_ms.saturating_sub(self.m_last_action_sent_ticks);
                    let mut send_interval_for_tick = time_passed;
                    if send_interval_for_tick > 250 || send_interval_for_tick == 0 {
                        send_interval_for_tick = self.m_latency as u32;
                    }

                    if !non_lagger_pids_to_send_tick.is_empty() {
                        self.m_sync_counter = self.m_sync_counter.wrapping_add(1);
                        let empty_actions_packet = self.m_protocol.SEND_W3GS_INCOMING_ACTION(
                            VecDeque::new(),
                            send_interval_for_tick as u16
                        );

                        for nl_pid in &non_lagger_pids_to_send_tick {
                            if let Some(nl_player) = self.get_player_from_pid_mut(*nl_pid) {
                                nl_player.put_bytes(empty_actions_packet.clone()).await;
                                nl_player.send_buff().await;
                            }
                        }
                        self.m_last_action_sent_ticks = current_ticks_ms; // Обновляем время последнего "тика"
                    }


                    // 3. Снова отправить START_LAG НЕлагающим, перечисляя тех, кто ВСЕ ЕЩЕ лагает
                    //    (laggers_for_stop_start_lag содержит актуальный список)
                    if !non_lagger_pids_to_send_tick.is_empty() {
                        let start_lag_packet = self.m_protocol.SEND_W3GS_START_LAG(laggers_for_stop_start_lag, false);
                        for nl_pid in &non_lagger_pids_to_send_tick {
                            if let Some(nl_player) = self.get_player_from_pid_mut(*nl_pid) {
                                nl_player.send(start_lag_packet.clone()).await;
                            }
                        }
                    }
                    self.m_last_lag_screen_reset_time = current_time_sec;
                }

                // Проверяем, не перестал ли кто-то из игроков лагать
                let mut any_player_still_truly_lagging = false;
                let mut pids_stopped_lagging: Vec<u8> = Vec::new();

                for player_idx_check_stop_lag in 0..self.m_players.len() {
                    let (pid, name, is_lagging, is_deleted, sync_counter_val) = {
                        let p = &self.m_players[player_idx_check_stop_lag];
                        (p.get_pid(), p.get_name(), p.get_lagging(), p.get_delete_me(), p.get_sync_counter())
                    };

                    if is_lagging && !is_deleted {
                        if self.m_sync_counter > sync_counter_val &&
                        self.m_sync_counter.saturating_sub(sync_counter_val) < self.m_sync_limit / 2 {
                            pids_stopped_lagging.push(pid); // Собираем PIDы тех, кто перестал лагать
                            if let Some(p_mut) = self.m_players.get_mut(player_idx_check_stop_lag) {
                                p_mut.set_lagging(false);
                                p_mut.set_started_lagging_ticks(0);
                            }
                            log_info(&format!("[GAME: {}] Player [{}] stopped lagging.", self.m_game_name, name));
                        } else {
                            any_player_still_truly_lagging = true;
                        }
                    }
                }

                // Отправляем STOP_LAG для тех, кто перестал лагать, всем остальным
                for stopped_lagger_pid in pids_stopped_lagging {
                    if let Some(stopped_lagger_player_clone) = self.get_player_from_pid(stopped_lagger_pid).cloned() { // Клонируем, чтобы передать в SEND_W3GS_STOP_LAG
                        if self.get_num_human_players() > 0 {
                            self.send_all(self.m_protocol.SEND_W3GS_STOP_LAG(&stopped_lagger_player_clone, false)).await;
                        }
                    }
                }

                if !any_player_still_truly_lagging && self.m_lagging {
                    log_info(&format!("[GAME: {}] Lag screen deactivated, all players caught up.", self.m_game_name));
                    self.m_lagging = false;
                    // После выхода из лагскрина, следующая итерация update вызовет send_all_actions для всех.
                }
                self.m_last_lag_screen_time = current_time_sec; // Обновляем время последнего активного лагскрина
            } else { // Если self.m_lagging == false
                // C. Отправка игровых действий (если не активен общий лагскрин)
                if self.m_last_action_sent_ticks == 0 ||
                current_ticks_ms.saturating_sub(self.m_last_action_sent_ticks) >= self.m_latency as u32 {
                    self.send_all_actions().await;
                }
            }

        }

        if self.m_game_loaded && !self.m_lagging {
            let ticks_now = get_ticks() as u32;
            let time_since_last_send = ticks_now.saturating_sub(self.m_last_action_sent_ticks);
        
            if self.m_last_action_sent_ticks == 0 || time_since_last_send >= self.m_latency as u32 {
                self.send_all_actions().await;
            }
        }

        if !self.m_kick_vote_player.is_empty() && get_time() - self.m_started_kick_vote_time >= 60 {
            log_info(&format!("[GAME: {}] votekick against player [{}] expired", self.m_game_name, self.m_kick_vote_player));
            self.send_all_chat(self.m_language.vote_kick_expired(&self.m_kick_vote_player)).await;
            self.m_kick_vote_player.clear();
            self.m_started_kick_vote_time = 0;
        }

        if self.m_players.len() == 1 && self.m_fake_player_pid == 255 && self.m_game_over_time == 0 && (self.m_game_loading || self.m_game_loaded) {
            log_info(&format!("[GAME: {}] gameover timer started (one player left)", self.m_game_name));
            self.m_game_over_time = get_time();
        }

        if self.m_game_over_time != 0 && get_time() - self.m_game_over_time >= 60 {
            let mut already_stopped = true;

            for i in self.m_players.iter_mut() {
                if i.get_delete_me() {
                    already_stopped = false;
                    break;
                }
            }
            if !already_stopped {
                log_info(&format!("[GAME: {}] is over (gameover timer finished)", self.m_game_name));
                self.stop_players("was disconnected (gameover timer finished)".to_owned());
            }
        }

        if self.m_players.is_empty() && (self.m_game_loaded || self.m_game_loading) {
            if !self.m_saving {
                log_info(&format!("[GAME: {}] is over (no player left)", self.m_game_name));
                self.save_game_data();
                self.m_saving = true;
            }
            else if self.is_game_data_saved() {
                return true;
            }
        }
        if self.m_exiting {
            return self.m_exiting;
        }
        // In game_base.rs (update method)
        if !self.m_socket.has_error() && self.m_socket.is_connected() {
            match self.m_socket.accept().await {
                Ok(Some(mut new_socket)) => {            
                    if new_socket.connected() {
                        let _ = new_socket.set_tcp_nodelay(true);
                        let game_arc = {
                            let current_game = CURRENT_GAME.read().await;
                            current_game.as_ref().map(Arc::clone)
                        };
                        if let Some(game_arc) = game_arc {
                            self.m_potentials.push(PotentialPlayer::new(
                                self.m_protocol.clone(),
                                game_arc,
                                new_socket,
                            ));
                        }
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    log_warning(&format!("Accept failed: {}", e));
                    return true;
                }
            }
        }

        return self.m_exiting;
        
    }
    pub async fn update_post(&mut self) {
        for (index, player) in self.m_players.iter_mut().enumerate() {
            if player.update().await {
                if player.get_socket().connected() {
                    player.get_socket().do_send_buff().await;
                }
            }
        }


        for player in self.m_potentials.iter_mut() {
            if player.update().await {
                if player.m_Socket.connected() {
                    player.m_Socket.do_send_buff().await;
                }
            }
        }
    }

    pub async fn send(&mut self, _player: &mut GamePlayer, _data: ByteArray) {
        _player.send(_data).await;
    }

    pub async fn send_socket(&mut self, _socket: &mut TcpClient, _data: ByteArray) {
        if _socket.connected() {
            _socket.do_send(&_data).await;
        }
    }
    
    pub async fn send_pid(&mut self, _pid: u8, _data: ByteArray) {
        if let Some(player) = self.m_players.iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == _pid) {
            player.send(_data).await;
        }
    }

    pub async fn send_pid_socket(&mut self, _pid: u8, _socket: &mut TcpClient, _data: ByteArray) {
        if _socket.connected() {
            _socket.do_send(&_data).await;
        }
    }

    pub async fn send_pids(&mut self, _pids: ByteArray, _data: ByteArray) {
        for i in _pids {
            self.send_pid(i.try_into().unwrap(), _data.clone()).await;
        }
    }
    pub async fn send_all(&mut self, _data: ByteArray) {
        for i in self.m_players.iter_mut() {
            i.send(_data.clone()).await;
        }
    }

    pub async fn send_chat_to_player(&mut self, _from_pid: u8, _player: &mut GamePlayer, _message: String) {
        let mut message = _message.clone();
        if !self.m_game_loading && !self.m_game_loaded {
            if message.len() > 254 {
                message = message[0..254].to_owned();
                self.send(_player, self.m_protocol.SEND_W3GS_CHAT_FROM_HOST(_from_pid, create_byte_array(&[_player.get_pid()]), 16, ByteArray::new(), message)).await;
            }
        }
    }

    pub async fn send_chat_to_player_socket(&mut self, _from_pid: u8, _socket: &mut TcpClient, _message: String, _pid: u8) {
        if !self.m_game_loading && !self.m_game_loaded {
            let mut message = _message.clone();
            if message.len() > 254 {
                message = message[0..254].to_owned();
                _socket.do_send(&self.m_protocol.SEND_W3GS_CHAT_FROM_HOST(_from_pid, create_byte_array(&[_pid]), 16, ByteArray::new(), message)).await;
            }
        }
    }


    pub async fn send_chat_to_pid(&mut self, _from_pid: u8, _to_pid: u8, _message: String) {
        if let Some(player) = self.m_players.clone().iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == _to_pid) {
            self.send_chat_to_player(_from_pid, player, _message).await;
        }
    }

    pub async fn send_chat_to_pid_socket(&mut self, _from_pid: u8, _to_pid: u8, _socket: &mut TcpClient, _message: String) {
        if let Some(player) = self.m_players.clone().iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == _to_pid) {
            self.send_chat_to_player_socket(_from_pid, _socket, _message, player.get_pid()).await;
        }
    }
    
    pub async fn send_chat_player(&mut self, _player: &mut GamePlayer, _message: String) {
        self.send_chat_to_player(self.get_host_pid(), _player, _message).await;
    }

    pub async fn send_chat_player_socket(&mut self, _socket: &mut TcpClient, _message: String, _pid: u8) {
        self.send_chat_to_player_socket(self.get_host_pid(), _socket, _message, _pid).await;
    }

    pub async fn send_chat_pid(&mut self, _to_pid: u8, _message: String) {
        self.send_chat_to_pid(self.get_host_pid(), _to_pid, _message).await;
    }

    pub async fn send_chat_pid_socket(&mut self, _to_pid: u8, _socket: &mut TcpClient, _message: String) {
        self.send_chat_to_pid_socket(self.get_host_pid(), _to_pid, _socket, _message).await;
    }

    pub async fn send_all_chat_from_pid(&mut self, _from_pid: u8, _message: String) {
        let mut message = _message.clone();
        if self.get_num_human_players() > 0 {
            log_info(&format!("[GAME: {}] [Local]: {}", self.m_game_name, message));

            if !self.m_game_loading && !self.m_game_loaded {
                if message.len() > 254 {
                    message = message[0..254].to_string();
                }
                self.send_all(self.m_protocol.SEND_W3GS_CHAT_FROM_HOST(
                    _from_pid,
                    self.pids( ), 
                    16, 
                    ByteArray::new(), 
                    message)
                ).await;
            } else {
                if message.len() > 127 {
                    message = message[0..127].to_string();
                }
                self.send_all(self.m_protocol.SEND_W3GS_CHAT_FROM_HOST(
                    _from_pid,
                    self.pids( ), 
                    32, 
                    create_byte_array_from_u32(0, false), 
                    message)
                ).await;
            }
        }
    }
    pub async fn send_all_chat(&mut self, _message: String) {
        self.send_all_chat_from_pid(self.get_host_pid(), _message).await;
    }
    pub async fn send_local_admin_chat(&mut self, _message: String) {}
    pub async fn send_all_slot_info(&mut self) {
        if !self.m_game_loading && !self.m_game_loaded {
            let map_layout_style = self.m_map.get_map_layout_style();
            let map_num_players = self.m_map.get_map_num_players();
            for i in self.m_players.iter_mut() {
                i.put_bytes(self.m_protocol.SEND_W3GS_SLOTINFO(
                    &self.m_slots, 
                    self.m_random_seed, 
                    map_layout_style, 
                    map_num_players)
                ).await;
            }   
            self.m_slot_info_changed = false;
        }
    }

    pub async fn send_all_slot_info_s(&mut self) {
        if !self.m_game_loading && !self.m_game_loaded {
            let map_layout_style = self.m_map.get_map_layout_style();
            let map_num_players = self.m_map.get_map_num_players();
            for i in self.m_players.iter_mut() {
                i.send(self.m_protocol.SEND_W3GS_SLOTINFO(
                    &self.m_slots, 
                    self.m_random_seed, 
                    map_layout_style, 
                    map_num_players)
                ).await;
            }   
            self.m_slot_info_changed = false;
        }
    }
    pub async  fn send_virtual_host_player_info(&mut self, _player: &mut GamePlayer) {
        if self.m_virtual_host_pid == 255 {
            return;
        }
        let ip: ByteArray = vec![0,0,0,0];
        self.send(_player, self.m_protocol.SEND_W3GS_PLAYERINFO( self.m_virtual_host_pid, self.m_virtual_host_name.clone(), ip.clone(), ip)).await;
    }
    pub async fn send_fake_player_info(&mut self, _player: &mut GamePlayer) {
        if self.m_virtual_host_pid == 255 {
            return;
        }
        let ip: ByteArray = vec![0,0,0,0];
        self.send(_player, self.m_protocol.SEND_W3GS_PLAYERINFO( self.m_virtual_host_pid, "FakePlayer".to_owned(), ip.clone(), ip)).await;
    }
    // В BaseGame
    pub async fn send_all_actions(&mut self) {
       // log_info(&format!("[GAME: {}] send_all_actions START. Actions in queue: {}", self.m_game_name.clone(), self.m_actions.len()));
        self.m_game_ticks = self.m_game_ticks.wrapping_add(self.m_latency as u32);
        self.m_sync_counter = self.m_sync_counter.wrapping_add(1);

        let mut current_send_interval: u16;
        if self.m_last_action_sent_ticks == 0 {
            current_send_interval = self.m_latency as u16;
        } else {
            current_send_interval = (get_ticks() as u32).saturating_sub(self.m_last_action_sent_ticks) as u16;
        }

        if current_send_interval > 50 /* 250 */ {
            current_send_interval = self.m_latency as u16;
        }
        if current_send_interval == 0 && self.m_last_action_sent_ticks != 0 {
            current_send_interval = self.m_latency as u16;
        }


        let mut actions_for_main_packet: VecDeque<CIncomingAction> = VecDeque::new();
        let mut current_packet_actions_size: usize = 0;
        const MAX_ACTIONS_PAYLOAD_SIZE: usize = 1400;

        while let Some(action) = self.m_actions.pop_front() {
            let action_length = action.get_length() as usize;

            if current_packet_actions_size + action_length > MAX_ACTIONS_PAYLOAD_SIZE && !actions_for_main_packet.is_empty() {
                let overflow_packet_data = self.m_protocol.SEND_W3GS_INCOMING_ACTION2(
                    actions_for_main_packet.clone()
                );
                self.put_bytes_all(overflow_packet_data).await;

                actions_for_main_packet.clear();
                current_packet_actions_size = 0;
            }

            actions_for_main_packet.push_back(action.clone());
            current_packet_actions_size += action_length;
        }

        let main_packet_data = self.m_protocol.SEND_W3GS_INCOMING_ACTION(
            actions_for_main_packet.clone(), // Может быть пустым
            current_send_interval,
        );
        self.put_bytes_all(main_packet_data).await;

        self.send_bytes_all().await;

        self.m_last_action_sent_ticks = get_ticks() as u32;
     //   log_info(&format!("[GAME: {}] send_all_actions END.", self.m_game_name.clone()));
    }

    pub async fn put_bytes_all(&mut self, _data: ByteArray) {
        for i in self.m_players.iter_mut() {
            i.put_bytes(_data.clone()).await;
        }
    }

    pub async fn send_bytes_all(&mut self) {
        for i in self.m_players.iter_mut() {
            i.send_buff().await;
        }
    }

    pub async fn send_welcome_message(&mut self, mut player: GamePlayer) {
        let motd_path = Path::new("motd.txt");
        if let Ok(file) = File::open(motd_path) {
            let reader = BufReader::new(file);
            for (i, line) in reader.lines().enumerate() {
                if i >= 8 {
                    break;
                }
                match line {
                    Ok(content) if content.trim().is_empty() => {
                        self.send_chat_player(&mut player, " ".to_owned()).await;
                    }
                    Ok(content) => {
                        self.send_chat_player(&mut player, content).await;
                    }
                    Err(_) => break, // при ошибке чтения строки — выход
                }
            }
        } else {
            // Дефолтное приветствие
            if self.m_hcl_command_string.is_empty() {
                self.send_chat_player(&mut player, " ".to_owned()).await;
            }

            self.send_chat_player(&mut player, " ".to_owned()).await;
            self.send_chat_player(&mut player, " ".to_owned()).await;
            self.send_chat_player(&mut player, " ".to_owned()).await;
            self.send_chat_player(&mut player, "GHostRS by *****                                         https://discord.gg/iccup".to_owned()).await;
            self.send_chat_player(&mut player, "-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-=-".to_owned()).await;
            self.send_chat_player(&mut player, format!("     Game Name:                 {}", self.m_game_name)).await;

            if !self.m_hcl_command_string.is_empty() {
                self.send_chat_player(&mut player, format!("     HCL Command String:  {}", self.m_hcl_command_string)).await;
            }
        }
    }
    pub async fn send_end_message(&mut self) {
        let gameover_path = Path::new("gameover.txt");
        if let Ok(file) = File::open(gameover_path) {
            let reader = BufReader::new(file);

            for (count, line) in reader.lines().enumerate() {
                if count >= 8 {
                    break;
                }

                match line {
                    Ok(ref content) if content.trim().is_empty() => {
                        self.send_all_chat(" ".to_owned()).await;
                    }
                    Ok(content) => {
                        self.send_all_chat(content).await;
                    }
                    Err(_) => break, // Прерывание при ошибке чтения строки
                }
            }
        }
    }

    async fn event_player_deleted(&mut self, _player: &mut GamePlayer) {
        let reason = _player.get_left_reason();
        if reason.is_empty() {
            log_warning(&format!("[GAME: {}] Player [{}] is being deleted with an EMPTY reason! Backtrace:\n{:?}", self.m_game_name, _player.get_name(), std::backtrace::Backtrace::capture()));
            // Возможно, здесь стоит установить какую-то дефолтную причину, чтобы не сбивать с толку.
            // player.set_left_reason("Unknown reason or internal error".to_string()); // Осторожно с &mut player здесь
        }
        let mut player = _player.clone();
        log_info(&format!("[GAME: {}] deleting player [{}]: {}", self.m_game_name, player.get_name(), player.get_left_reason()));

        self.m_last_player_leave_ticks = get_ticks() as u32;

        if player.get_left_message_sent() {
            return;
        }

        if self.m_game_loaded {
            self.send_all_chat(format!("{} {}.", player.get_name(), player.get_left_reason())).await;
        }

        if player.get_lagging() {
            self.send_all(self.m_protocol.SEND_W3GS_STOP_LAG(&player, false)).await;
        }
        let mut players = self.m_players.clone();

        if self.m_game_loading && self.m_load_in_game {
            // Collect PIDs of finished loading players to avoid double mutable borrow
            let finished_loading_pids: Vec<u8> = players
                .iter()
                .filter(|p| p.get_finished_loading())
                .map(|p| p.get_pid())
                .collect();

            // Collect actions to perform after the mutable borrow ends
            let mut stop_lag_pids = Vec::new();
            let mut playerleave_others_pids = Vec::new();
            for pid in &finished_loading_pids {
                if !player.get_finished_loading() {
                    stop_lag_pids.push(*pid);
                }
                playerleave_others_pids.push(*pid);
            }
            // Now perform the actions outside the borrow
            for pid in stop_lag_pids {
                if let Some(index) = players.iter().position(|p| p.get_pid() == pid) {
                    let protocol = self.m_protocol.clone();
                    {
                        let mut p = players[index].clone();
                        self.send(&mut p, protocol.SEND_W3GS_STOP_LAG(&player, false)).await;
                    }
                }
            }
            for pid in playerleave_others_pids {
                if let Some(p) = players.iter_mut().find(|p| p.get_pid() == pid) {
                    let data = self.m_protocol.SEND_W3GS_PLAYERLEAVE_OTHERS(player.get_pid(), player.get_left_code());
                    self.send(p, data).await;
                }
            }
    
            let player_leave_data = self.m_protocol.SEND_W3GS_PLAYERLEAVE_OTHERS(player.get_pid(), player.get_left_code());
            for p in self.m_players.iter_mut() {
                if !p.get_finished_loading() {
                    p.add_load_in_game_data(player_leave_data.clone());
                }
            }
        } else {
            self.send_all(self.m_protocol.SEND_W3GS_PLAYERLEAVE_OTHERS(player.get_pid(), player.get_left_code())).await;
        }

        if self.m_count_down_started && !self.m_game_loading && !self.m_game_loaded {
            self.send_all_chat(self.m_language.count_down_aborted()).await;
            self.m_count_down_started = false;
        }

        if !self.m_kick_vote_player.is_empty() {
            self.send_all_chat(self.m_language.vote_kick_cancelled(&self.m_kick_vote_player)).await;
            self.m_kick_vote_player.clear();
            self.m_started_kick_vote_time = 0;
        }
    }

    async fn event_player_disconnect_timed_out(&mut self, player: &mut GamePlayer) {
        if player.get_gproxy() && self.m_game_loaded {
            if !player.get_gproxy_disconnect_notice_sent() {
                self.send_all_chat(format!("{} {}.", player.get_name(), self.m_language.lost_connection_timeout_gproxy())).await;
                player.set_gproxy_disconnect_notice_sent(true);
            }

            if get_time() - player.get_last_gproxy_wait_notice_sent_time() >= 20 {
                let mut time_remaining = (self.m_gproxy_empty_actions + 1) * 60 - ((get_time() - self.m_started_lagging_time) as u8);
                if time_remaining > ((self.m_gproxy_empty_actions + 1) * 60) as u8 {
                    time_remaining = ((self.m_gproxy_empty_actions + 1) * 60) as u8;
                }
                self.send_all_chat_from_pid(player.get_pid(), self.m_language.waiting_to_reconnect(&format!("{}", time_remaining))).await;
                player.set_last_gproxy_wait_notice_sent_time(get_time());
            }
            return;
        }

        if get_time() - self.m_last_lag_screen_time >= 10 {
            println!("on m_last_lag_screen_time");

            player.set_delete_me(true);
            player.set_left_reason(self.m_language.has_lost_connection_timed_out());
            player.set_left_code(PLAYERLEAVE_DISCONNECT as u32);

            if !self.m_game_loading && !self.m_game_loaded {
                self.open_slot(self.get_sid_from_pid(player.get_pid()), false).await;
            }
        }
    }

    pub async fn event_player_disconnect_player_error(&mut self, pid: u8, error_string: String) {
        println!("event_player_disconnect_player_error");
        self.set_delete_me(pid, true).await;
        self.set_left_reason(pid, self.m_language.has_lost_connection_player_error(&error_string)).await;
        self.set_left_code(pid, PLAYERLEAVE_DISCONNECT as u32).await;

        if !self.m_game_loading && !self.m_game_loaded {
            self.open_slot(self.get_sid_from_pid(pid), false).await;
        }
    }

    pub async fn event_player_disconnect_socket_error(&mut self, error_string: String, name: String, pid: u8, gproxy: bool, gproxy_sent: bool, gproxy_wait: u32) {
        if gproxy && self.m_game_loaded {
            if !gproxy_sent {
                self.send_all_chat(format!("{} {}.", name, self.m_language.lost_connection_error_gproxy(&error_string))).await;
                self.set_gproxy_disconnect_notice_sent(pid, true).await;
            }

            if get_time() - gproxy_wait >= 20 {
                let mut time_remaining = (self.m_gproxy_empty_actions + 1) * 60 - ((get_time() - self.m_started_lagging_time) as u8);
                if time_remaining > ((self.m_gproxy_empty_actions + 1) * 60) as u8 {
                    time_remaining = ((self.m_gproxy_empty_actions + 1) * 60) as u8;
                }
                self.send_all_chat_from_pid(pid, self.m_language.waiting_to_reconnect(&format!("{}", time_remaining))).await;
                self.set_last_gproxy_wait_notice_sent_time(pid, get_time()).await;
            }
            return;
        }
        
        self.set_delete_me(pid, true).await;
        self.set_left_reason(pid, self.m_language.has_lost_connection_socket_error(&error_string)).await;
        self.set_left_code(pid, PLAYERLEAVE_DISCONNECT as u32).await;

        if !self.m_game_loading && !self.m_game_loaded {
            self.open_slot(self.get_sid_from_pid(pid), false).await;
        }
    }

    pub async fn event_player_disconnect_connection_closed(&mut self, name: String, pid: u8, gproxy_wait: u32, gproxy_sent: bool, gproxy: bool) {
        if gproxy && self.m_game_loaded {
            if !gproxy_sent {
                self.send_all_chat(format!("{} {}.", name, self.m_language.lost_connection_closed_gproxy())).await;
                self.set_gproxy_disconnect_notice_sent(pid, true).await;
            }

            if get_time() - gproxy_wait >= 20 {
                let mut time_remaining = (self.m_gproxy_empty_actions + 1) * 60 - ((get_time() - self.m_started_lagging_time) as u8);
                if time_remaining > ((self.m_gproxy_empty_actions + 1) * 60) as u8 {
                    time_remaining = ((self.m_gproxy_empty_actions + 1) * 60) as u8;
                }
                self.send_all_chat_from_pid(pid, self.m_language.waiting_to_reconnect(&format!("{}", time_remaining))).await;
                self.set_last_gproxy_wait_notice_sent_time(pid, get_time()).await;
            }
            return;
        }
        println!("event_player_disconnect_connection_closed");

        self.set_delete_me(pid, true).await;
        self.set_left_reason(pid, self.m_language.has_lost_connection_closed_by_remote_host()).await;
        self.set_left_code(pid,PLAYERLEAVE_DISCONNECT as u32).await;

        if !self.m_game_loading && !self.m_game_loaded {
            self.open_slot(self.get_sid_from_pid(pid), false).await;
        }
    }

    pub async fn event_player_joined(&mut self,  potential:&mut PotentialPlayer, join_player: &IncomingJoinPlayer) {
        if join_player.get_name().is_empty() || join_player.get_name().len() > 15 {
            log_info(&format!("[GAME: {}] player [{}|{}] is trying to join the game with an invalid name of length {}", self.m_game_name, join_player.get_name(), potential.get_external_ip_string(), join_player.get_name().len()));
            potential.send(self.m_protocol.SEND_W3GS_REJECTJOIN(REJECTJOIN_FULL.into())).await;
            potential.set_delete_me(true);
            return;
        }

        if join_player.get_name() == self.m_virtual_host_name {
            log_info(&format!("[GAME: {}] player [{}|{}] is trying to join the game with the virtual host name", self.m_game_name, join_player.get_name(), potential.get_external_ip_string()));
            potential.send(self.m_protocol.SEND_W3GS_REJECTJOIN(REJECTJOIN_FULL.into())).await;
            potential.set_delete_me(true);
            return;
        }

        if self.get_player_from_name(join_player.get_name(), false).is_some() {
            log_info(&format!("[GAME: {}] player [{}|{}] is trying to join the game but that name is already taken", self.m_game_name, join_player.get_name(), potential.get_external_ip_string()));
            potential.send(self.m_protocol.SEND_W3GS_REJECTJOIN(REJECTJOIN_FULL.into())).await;
            potential.set_delete_me(true);
            return;
        }

        let host_counter_id = join_player.get_host_counter() >> 28;
        let mut joined_realm = String::new();

        if host_counter_id == 0 {
            if join_player.get_entry_key() != self.m_entry_key {
                log_info(&format!("[GAME: {}] player [{}|{}] is trying to join the game over LAN but used an incorrect entry key", self.m_game_name, join_player.get_name(), potential.get_external_ip_string()));
                potential.send(self.m_protocol.SEND_W3GS_REJECTJOIN(REJECTJOIN_WRONGPASSWORD.into())).await;
                potential.set_delete_me(true);
                return;
            }
        } else {
            let bnets = m_BNETs.read().await;
            for bnet_arc in bnets.iter() {
                let bnet = bnet_arc.lock().await;

                if bnet.get_host_counter_id() == host_counter_id {
                    joined_realm = bnet.get_server(); // здесь get_server должен вернуть clone()
                    break;
                }
            }


        }

        let any_admin_check = true;
        let reserved = true;

        let mut sid = 255;
        let mut enforce_pid = 255;
        let mut enforce_slot = GameSlot::new(255, 0, 0, 0, 0, 0, 0, SLOTCOMP_NORMAL, 100);
        let mut enforce_sid = 0;

        if sid == 255 && reserved {
            sid = self.get_empty_slot(true);
            if sid != 255 {
                let reason = self.m_language.was_kicked_for_reserved_player(&join_player.get_name());
                if let Some(kicked_player) = self.get_player_from_sid_mut(sid) {
                    println!("on kicked_player");
                    kicked_player.set_delete_me(true);
                    kicked_player.set_left_reason(reason);
                    kicked_player.set_left_code(PLAYERLEAVE_LOBBY as u32);
                    kicked_player.set_left_message_sent(true);
                }
                let protocol = self.m_protocol.clone();
                if let Some(kicked_player) = self.get_player_from_sid(sid) {
                    let data = protocol.SEND_W3GS_PLAYERLEAVE_OTHERS(kicked_player.get_pid(), kicked_player.get_left_code().into());
                    self.send_all(data).await;
                }
            }
        }

        if sid == 255 && self.is_owner(join_player.get_name().clone()) {
            sid = 0;
            for i in 0..self.m_slots.len() {
                if self.m_slots[i].slot_status() == SLOTSTATUS_OCCUPIED && self.m_slots[i].computer() == 0 {
                    sid = i as u8;
                    break;
                }
            }
            let reason = self.m_language.was_kicked_for_owner_player(&join_player.get_name());
            if let Some(kicked_player) = self.get_player_from_sid_mut(sid) {
                println!("on kicked_player2");
                kicked_player.set_delete_me(true);
                kicked_player.set_left_reason(reason);
                kicked_player.set_left_code(PLAYERLEAVE_LOBBY as u32);
                kicked_player.set_left_message_sent(true);
            } let protocol = self.m_protocol.clone();
            if let Some(kicked_player) = self.get_player_from_sid(sid) {
                let data = protocol.SEND_W3GS_PLAYERLEAVE_OTHERS(kicked_player.get_pid(), kicked_player.get_left_code().into());
                self.send_all(data).await;
            }
        }

        if sid >= self.m_slots.len() as u8 {
            potential.send(self.m_protocol.SEND_W3GS_REJECTJOIN(REJECTJOIN_FULL as u32)).await;
            potential.set_delete_me(true);
            return;
        }

        if self.get_num_players() >= 11 {
            self.delete_virtual_host().await;
        }
        log_info(&format!("[GAME: {}] player [{}|{}] joined the game", self.m_game_name, join_player.get_name(), potential.get_external_ip_string()));
    let mut new_player = GamePlayer::new_from_potential(
        std::mem::take(potential),
        self.get_new_pid(),
        joined_realm.clone(),
        join_player.get_name().clone(),
        join_player.get_internal_ip().clone(),
        reserved
    ).await;
    if joined_realm.is_empty() {
        new_player.set_spoofed(true);
    }
    let new_player_pid = new_player.get_pid();
    // new_player.set_whois_should_be_sent(self.m_ghost.m_spoof_checks == 1 || (self.m_ghost.m_spoof_checks == 2 && any_admin_check));
    self.m_players.push(new_player);
    potential.set_socket(TcpClient::new());
    println!("izza potentiala");
    potential.set_delete_me(true);

    if self.m_map.get_map_options() & MAPOPT_CUSTOMFORCES > 0 {
        self.m_slots[sid as usize] = GameSlot::new(new_player_pid, 255, SLOTSTATUS_OCCUPIED, 0, self.m_slots[sid as usize].team(), self.m_slots[sid as usize].colour(), self.m_slots[sid as usize].race(), SLOTCOMP_NORMAL, 100);
    } else {
        if self.m_map.get_map_flags() & MAPFLAG_RANDOMRACES > 0 {
            self.m_slots[sid as usize] = GameSlot::new(new_player_pid, 255, SLOTSTATUS_OCCUPIED, 0, 12, 12, SLOTRACE_RANDOM, SLOTCOMP_NORMAL, 100);
        } else {
            self.m_slots[sid as usize] = GameSlot::new(new_player_pid, 255, SLOTSTATUS_OCCUPIED, 0, 12, 12, SLOTRACE_RANDOM | SLOTRACE_SELECTABLE, SLOTCOMP_NORMAL, 100);
        }
        let mut num_other_players = 0;
        for slot in self.m_slots.iter() {
            if slot.slot_status() == SLOTSTATUS_OCCUPIED && slot.team() != 12 {
                num_other_players += 1;
            }
        }
        if num_other_players < self.m_map.get_map_num_players() {
            if sid < self.m_map.get_map_num_players() as u8 {
                self.m_slots[sid as usize].set_team(sid);
            } else {
                self.m_slots[sid as usize].set_team(0);
            }
            let new_colour = self.get_new_colour();
            let sid_usize = sid as usize;
            self.m_slots[sid_usize].set_colour(new_colour);
        }
    }

    // Get the index of the newly pushed player
    let new_player_index = self.m_players.len() - 1;

    // First prepare all the data we need
    let new_player = &mut self.m_players[new_player_index];
    let player_pid = new_player.get_pid();
    let player_port = new_player.m_socket.get_port().unwrap_or(0);
    let player_external_ip = new_player.get_external_ip();
    
    // Send the slot info join
    {
        let data = self.m_protocol.SEND_W3GS_SLOTINFOJOIN(
            player_pid,
            create_byte_array_from_u16(player_port, false),
            player_external_ip.clone(),
            self.m_slots.clone(),
            self.m_random_seed,
            self.m_map.get_map_layout_style(),
            self.m_map.get_map_num_players()
        );
        new_player.put_bytes(data).await;
    }

    // Send virtual host and fake player info
    {
        let vh_data = self.m_protocol.SEND_W3GS_PLAYERINFO(
            self.m_virtual_host_pid,
            self.m_virtual_host_name.clone(),
            vec![0, 0, 0, 0],
            vec![0, 0, 0, 0]
        );
        new_player.put_bytes(vh_data).await;
    }

    let blank_ip = vec![0, 0, 0, 0];
    // First collect all the player info we need
    let player_infos: Vec<(u8, String, ByteArray, ByteArray)> = self.m_players.iter()
        .filter(|p| !p.get_left_message_sent() && p.get_pid() != new_player_pid)
        .map(|p| (p.get_pid(), p.get_name(), p.get_external_ip(), p.get_internal_ip()))
        .collect();

        let player_pid;
        let player_name;
        let player_external_ip;
        let player_internal_ip;

        {
            // временный scope, чтобы ограничить immutable borrow
            let player = &self.m_players[new_player_index];
            player_pid = player.get_pid();
            player_name = player.get_name().clone();
            
            player_external_ip = if false {
                vec![0, 0, 0, 0]
            } else {
                player.get_external_ip()
            };
            player_internal_ip = if false {
                vec![0, 0, 0, 0]
            } else {
                player.get_internal_ip()
            };
        }

        for i in 0..self.m_players.len() {
            let other_player = &mut self.m_players[i];
            if !other_player.get_left_message_sent() && other_player.get_pid() != player_pid {
                // Отправить info другим об новом игроке
                if other_player.m_socket.connected() {
                    let msg = self.m_protocol.SEND_W3GS_PLAYERINFO(
                        player_pid,
                        player_name.clone(),
                        player_external_ip.clone(),
                        player_internal_ip.clone(),
                    );
                    //println!("→ to {}: {:x?}", other_player.get_name(), msg);
                    other_player.put_bytes(msg).await;
                }
                // Отправить info новому игроку об других
                let msg_back = self.m_protocol.SEND_W3GS_PLAYERINFO(
                    other_player.get_pid(),
                    other_player.get_name().clone(),
                    if false { vec![0, 0, 0, 0] } else { other_player.get_external_ip() },
                    if false { vec![0, 0, 0, 0] } else { other_player.get_internal_ip() },
                );
                //println!("← from {}: {:x?}", other_player.get_name(), msg_back);
                self.m_players[new_player_index].put_bytes(msg_back).await;
            }
        }
        

        

    
    // Now use the mutable reference to send the map check
    self.m_players.get_mut(new_player_index).unwrap().put_bytes(self.m_protocol.SEND_W3GS_MAPCHECK(format!("Maps\\Download\\{}", self.m_map.get_map_path()), self.m_map.get_map_size(), self.m_map.get_map_info(), self.m_map.get_map_crc(), self.m_map.get_map_sha1())).await;
    
    self.send_all_slot_info().await;
    for p in self.m_players.iter_mut() {
        p.send_buff().await;
    }

    let player_index = new_player_index;
    let message = "GHostRS by ***** https://discord.gg/iccup".to_owned();

    let player_ptr = self.m_players.get_mut(player_index).unwrap() as *mut _; // raw pointer чтобы избежать borrow check

    // вызвать функцию без `&mut self`, если возможно
    unsafe {
        let player = &mut *player_ptr;
        self.send_chat_player(player, message).await;
    }




        if self.m_count_down_started && !self.m_game_loading && !self.m_game_loaded {
            self.send_all_chat(self.m_language.count_down_aborted()).await;
            self.m_count_down_started = false;
        }

        if false && !self.m_locked && self.is_owner(join_player.get_name()) {
            self.send_all_chat(self.m_language.game_locked()).await;
            self.m_locked = true;
        }
    }

    pub async fn event_player_left(&mut self, pid: u8) {
        if !self.m_game_loading && !self.m_game_loaded {
            self.open_slot(self.get_sid_from_pid(pid), false).await;
        }
    }

    pub async fn event_player_loaded(&mut self, pid: u8, name: String, finished_loading_ticks: u32) {
        log_info(&format!("[GAME: {}] player [{}] finished loading in {} seconds", self.m_game_name, name, ((finished_loading_ticks - self.m_started_loading_ticks) as f32 / 1000.0)));
        let mut players = self.m_players.clone();
        // Find the index of the player with the given pid
        let player_index = players.iter().position(|p| p.get_pid() == pid).unwrap();
        if self.m_load_in_game {
            // Take the player out to avoid borrow conflicts
            let mut player = players.remove(player_index);
            let mut load_in_game_data = player.get_load_in_game_data();
            while !load_in_game_data.is_empty() {
                self.send(&mut player, load_in_game_data.pop_front().unwrap()).await;
            }

            let mut finished_loading = true;
            for p in &players {
                if !p.get_finished_loading() {
                    finished_loading = false;
                    break;
                }
            }

            if !finished_loading {
                self.send(&mut player, self.m_protocol.SEND_W3GS_START_LAG(players.clone(), true)).await;
            }

            for p in &mut players {
                if p.get_finished_loading() {
                    self.send(p, self.m_protocol.SEND_W3GS_STOP_LAG(&player, false)).await;
                }
            }

            for p in &mut players {
                if p.get_finished_loading() {
                    self.send_all_chat(self.m_language.player_finished_loading(&player.get_name())).await;
                }
            }

            // Optionally, put the player back if needed elsewhere
        } else {
            // Find the player again in self.m_players for correct pid
            if let Some(player) = self.m_players.iter().find(|p| p.get_pid() == pid) {
                self.send_all(self.m_protocol.SEND_W3GS_GAMELOADED_OTHERS(player.get_pid())).await;
            }
        }
    }

    pub async fn event_player_action(&mut self, name: String, action: &CIncomingAction) {
        self.m_actions.push_back(action.clone());

        if !action.get_action().is_empty() && action.get_action()[0] == 6 {
            log_info(&format!("[GAME: {}] player [{}] is saving the game", self.m_game_name, name));
            self.send_all_chat(self.m_language.player_is_saving_the_game(&name)).await;
        }
    }

    pub async fn set_sync_counter(&mut self, pid: u8, counter: u32) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_sync_counter(counter);
        }
    }
    // В CBaseGame
pub async fn event_player_keep_alive(&mut self, pid_from_player: u8, checksum_from_player: u32) {
    // 1. Найти игрока и сохранить его чексумму и обновить его sync_counter
    let mut player_found_and_updated = false;
    if let Some(player) = self.get_player_from_pid_mut(pid_from_player) {
        if !player.get_delete_me() { // Обрабатываем только активных игроков
            player.m_check_sums.push_back(checksum_from_player); // Добавляем в очередь чексумм игрока
            self.set_sync_counter(pid_from_player, self.m_sync_counter);     // Помечаем, что игрок ответил на текущий игровой тик
            player_found_and_updated = true;

            // Ограничиваем размер очереди чексумм, если нужно (например, хранить только последние N)
            // const MAX_CHECKSUMS_PER_PLAYER: usize = 5;
            // if player.m_check_sums.len() > MAX_CHECKSUMS_PER_PLAYER {
            //     player.m_check_sums.pop_front();
            // }
        }
    }

    if !player_found_and_updated {
        log_warning(&format!("[GAME: {}] Received keepalive from unknown or deleted PID: {}", self.m_game_name, pid_from_player));
        return;
    }

    // 2. Проверить, все ли активные игроки прислали чексумму для текущего m_sync_counter игры
    let mut all_active_players_responded = true;
    let mut num_active_players_for_check = 0;

    for p in self.m_players.iter() {
        if !p.get_delete_me() && !p.get_lagging() { // Игнорируем удаляемых и уже лагающих (их чексуммы могут не приходить)
            num_active_players_for_check += 1;
            if p.get_sync_counter() < self.m_sync_counter || p.m_check_sums.is_empty() {
                // Этот игрок еще не прислал чексумму для текущего m_sync_counter игры
                // или его очередь чексумм пуста (что не должно быть, если sync_counter обновлен)
                all_active_players_responded = false;
                break;
            }
        }
    }

    // Если игроков для проверки нет (например, все вышли или игра только началась и еще никто не ответил на первый action)
    if num_active_players_for_check == 0 {
        return;
    }

    // 3. Если все ответили, можно сравнивать чексуммы
        if all_active_players_responded {
            log_info(&format!("[GAME: {}] All active players responded for sync_counter: {}. Comparing checksums.", self.m_game_name, self.m_sync_counter));

            let mut first_checksum: Option<u32> = None;
            let mut desync_detected_this_tick = false;
            let mut player_checksums_for_log: Vec<(String, u32)> = Vec::new();

            // Извлекаем последние чексуммы (те, что соответствуют текущему m_sync_counter)
            // и проверяем их на совпадение.
            for p in self.m_players.iter_mut() {
                if !p.get_delete_me() && !p.get_lagging() && p.get_sync_counter() == self.m_sync_counter {
                    if let Some(chk) = p.m_check_sums.pop_front() { // Берем самую старую из полученных (FIFO)
                        player_checksums_for_log.push((p.get_name(), chk));
                        if first_checksum.is_none() {
                            first_checksum = Some(chk);
                        } else if first_checksum != Some(chk) {
                            desync_detected_this_tick = true;
                        }
                    } else {
                        // Этого не должно произойти, если all_active_players_responded == true
                        log_error(&format!("[GAME: {}] Player {} responded for tick {} but checksum queue is empty!", self.m_game_name, p.get_name(), self.m_sync_counter));
                        desync_detected_this_tick = true; // Считаем это десинхроном
                    }
                }
            }

            if desync_detected_this_tick {
                log_warning(&format!("[GAME: {}] DESYNC DETECTED at sync_counter: {}", self.m_game_name, self.m_sync_counter));
                for (name, chk) in player_checksums_for_log.clone() {
                    log_warning(&format!("[GAME: {}]   Player [{}]: Checksum 0x{:X}", self.m_game_name, name, chk));
                }
                self.send_all_chat(self.m_language.desync_detected()).await;

                // Логика определения "виновных" (как в C++ GHost++)
                // Группируем игроков по чексуммам
                let mut bins: std::collections::HashMap<u32, Vec<u8>> = std::collections::HashMap::new();
                for p_info in player_checksums_for_log { // Используем уже извлеченные чексуммы
                    if let Some(player_obj) = self.get_player_from_name(p_info.0.clone(), true) { // Находим игрока по имени
                        bins.entry(p_info.1).or_default().push(player_obj.get_pid());
                    }
                }


                if bins.len() <= 1 {
                    // Все чексуммы одинаковы (или только один игрок прислал), или нет данных для сравнения.
                    // Этого не должно произойти, если desync_detected_this_tick == true и есть >1 игрока.
                    // Возможно, это означает, что проблема была только в одном игроке, который отвалился.
                    log_info(&format!("[GAME: {}] Desync detected, but bins logic found <= 1 unique checksum groups. No one kicked by bin logic.", self.m_game_name));
                } else {
                    // Находим самый большой "бин" (группу игроков с одинаковой чексуммой)
                    let mut largest_bin_checksum: Option<u32> = None;
                    let mut largest_bin_size = 0;
                    let mut tied = false;

                    for (chk, pids_in_bin) in bins.iter() {
                        self.send_all_chat(Language::new().players_in_game_number(
                            &format!("0x{:X}", chk), // Отображаем чексумму состояния
                            &pids_in_bin.iter().map(|pid| self.get_player_from_pid(*pid).map_or_else(|| format!("PID:{}", pid), |p| p.get_name())).collect::<Vec<_>>().join(", ")
                        )).await;

                        if pids_in_bin.len() > largest_bin_size {
                            largest_bin_size = pids_in_bin.len();
                            largest_bin_checksum = Some(*chk);
                            tied = false;
                        } else if pids_in_bin.len() == largest_bin_size {
                            tied = true;
                        }
                    }

                    if tied || largest_bin_checksum.is_none() {
                        log_warning(&format!("[GAME: {}] Desync: Can't kick desynced players due to a tie or no majority. Kicking all players.", self.m_game_name));
                        self.send_all_chat(Language::new().desync_detected()).await; // Добавьте такую строку в Language
                        self.stop_players(Language::new().desync_detected());
                    } else {
                        let majority_checksum = largest_bin_checksum.unwrap();
                        log_info(&format!("[GAME: {}] Desync: Kicking players not matching majority checksum 0x{:X}", self.m_game_name, majority_checksum));
                        let mut players_to_kick_pids = Vec::new();
                        for (chk, pids_in_bin) in bins.iter() {
                            if *chk != majority_checksum {
                                players_to_kick_pids.extend(pids_in_bin);
                            }
                        }
                        let game_name = self.m_game_name.clone();
                        for pid_to_kick in players_to_kick_pids {
                            if let Some(player_to_kick) = self.get_player_from_pid_mut(pid_to_kick) {
                                if !player_to_kick.get_delete_me() { // Проверяем, что еще не помечен на удаление
                                    log_info(&format!("[GAME: {}] Kicking player [{}] (PID: {}) due to desync.", game_name, player_to_kick.get_name(), pid_to_kick));
                                    player_to_kick.set_delete_me(true);
                                    player_to_kick.set_left_reason(Language::new().kicked_due_to_desync());
                                    player_to_kick.set_left_code(PLAYERLEAVE_LOST as u32); // Или другой код для десинхрона
                                }
                            }
                        }
                    }
                }
            } else {
                // Десинхрон не обнаружен для этого тика
                log_info(&format!("[GAME: {}] Checksums match for sync_counter: {}", self.m_game_name, self.m_sync_counter));
            }
        }
        // else: не все игроки еще прислали чексумму для этого m_sync_counter, ждем.
    }

    pub async fn event_player_chat_to_host(&mut self, chat_player: &CIncomingChatPlayer, pid: u8, muted: bool, name: String) {
        // Find the index of the player to avoid borrow conflicts
        let player_index = self.m_players.iter().position(|p| !p.get_left_message_sent() && p.get_pid() == pid);
        if let Some(player_index) = player_index {
            if chat_player.get_from_pid() == pid {
                if chat_player.get_type() == ChatToHostType::CTH_MESSAGE || chat_player.get_type() == ChatToHostType::CTH_MESSAGEEXTRA {
                    let mut relay = !muted;
                    let extra_flags = chat_player.get_extra_flags();

                    let min_string = ((self.m_game_ticks / 1000) / 60).to_string();
                    let sec_string = ((self.m_game_ticks / 1000) % 60).to_string();
                    let min_string = if min_string.len() == 1 { format!("0{}", min_string) } else { min_string };
                    let sec_string = if sec_string.len() == 1 { format!("0{}", sec_string) } else { sec_string };

                    if !extra_flags.is_empty() {
                        if extra_flags[0] == 0 {
                            log_info(&format!("[GAME: {}] ({}:{}) [All] [{}]: {}", self.m_game_name, min_string, sec_string, name, chat_player.get_message()));
                            if self.m_mute_all {
                                relay = false;
                            }
                        } else if extra_flags[0] == 2 {
                            log_info(&format!("[GAME: {}] ({}:{}) [Obs/Ref] [{}]: {}", self.m_game_name, min_string, sec_string, name, chat_player.get_message()));
                        }
                    } else {
                        log_info(&format!("[GAME: {}] [Lobby] [{}]: {}", self.m_game_name, name, chat_player.get_message()));
                        if self.m_mute_lobby {
                            relay = false;
                        }
                    }

                    let mut message = chat_player.get_message();
                    if message == "?trigger" {
                        // Avoid double mutable borrow by collecting the pid and message, then sending after borrow ends
                        let player_pid = self.m_players[player_index].get_pid();
                        let trigger_message = self.m_language.command_trigger_is("!");
                        // End the borrow of self.m_players before mutably borrowing self
                        self.send_chat_pid(pid, message).await;
                    } else if !message.is_empty() && message.starts_with("!") {
                        let command = message[1..].split(' ').next().unwrap_or("").to_lowercase();
                        let payload = message[1..].split(' ').skip(1).collect::<Vec<&str>>().join(" ");
                        if self.event_player_bot_command(&command, &payload) {
                            relay = false;
                        }
                    }

                    if relay {
                        //println!("TO_PIDS = {:x?}, FROM PID = {}, EXTRA_FLAGS = {:x?}", chat_player.get_to_pids(), chat_player.get_from_pid(), chat_player.get_extra_flags());
                        self.send_pids(chat_player.get_to_pids(), self.m_protocol.SEND_W3GS_CHAT_FROM_HOST(chat_player.get_from_pid(), chat_player.get_to_pids(), chat_player.get_flag(), chat_player.get_extra_flags(), chat_player.get_message())).await;
                    }
                } else if chat_player.get_type() == ChatToHostType::CTH_TEAMCHANGE && !self.m_count_down_started {
                    self.event_player_change_team(chat_player.get_byte(), pid).await;
                } else if chat_player.get_type() == ChatToHostType::CTH_COLOURCHANGE && !self.m_count_down_started {
                    self.event_player_change_colour(chat_player.get_byte(), pid).await;
                } else if chat_player.get_type() == ChatToHostType::CTH_RACECHANGE && !self.m_count_down_started {
                    self.event_player_change_race(chat_player.get_byte(), pid).await;
                } else if chat_player.get_type() == ChatToHostType::CTH_HANDICAPCHANGE && !self.m_count_down_started {
                    self.event_player_change_handicap(chat_player.get_byte(), pid).await;
                }
            }
        }
    }

    fn event_player_bot_command(&mut self, _command: &str, _payload: &str) -> bool {
        false
    }

    async fn event_player_change_team(&mut self, team: u8, pid: u8) {
        if self.m_map.get_map_options() & MAPOPT_CUSTOMFORCES > 0 {
            let old_sid = self.get_sid_from_pid(pid);
            let new_sid = self.get_empty_slot_team(team, pid);
            self.swap_slots(old_sid, new_sid).await;
        } else {
            if team > 12 {
                return;
            }
            if team == 12 && self.m_map.get_map_observers() != MAPOBS_ALLOWED && self.m_map.get_map_observers() != MAPOBS_REFEREES {
                return;
            }
            if team != 12 && team >= self.m_map.get_map_num_players() {
                return;
            }
            let mut num_other_players = 0;
            for slot in self.m_slots.iter() {
                if slot.slot_status() == SLOTSTATUS_OCCUPIED && slot.team() != 12 && slot.pid() != pid {
                    num_other_players += 1;
                }
            }
            if num_other_players >= self.m_map.get_map_num_players() {
                return;
            }
            let sid = self.get_sid_from_pid(pid);
            if sid < self.m_slots.len() as u8 {
                self.m_slots[sid as usize].set_team(team);
                if team == 12 {
                    self.m_slots[sid as usize].set_colour(12);
                } else if self.m_slots[sid as usize].colour() == 12 {
                    let new_colour = self.get_new_colour();
                    self.m_slots[sid as usize].set_colour(new_colour);
                }
                self.send_all_slot_info_s().await;
            }
        }
    }

    async fn event_player_change_colour(&mut self, pid: u8, colour: u8) {
        if self.m_map.get_map_options() & MAPOPT_FIXEDPLAYERSETTINGS > 0 {
            return;
        }
        if colour > 11 {
            return;
        }
        let sid = self.get_sid_from_pid(pid);
        if sid < self.m_slots.len() as u8 && self.m_slots[sid as usize].team() != 12 {
            self.colour_slot(sid, colour).await;
        }
    }

    async fn event_player_change_race(&mut self, race: u8, pid: u8) {
        if self.m_map.get_map_options() & MAPOPT_FIXEDPLAYERSETTINGS > 0 || self.m_map.get_map_flags() & MAPFLAG_RANDOMRACES > 0 {
            return;
        }
        if race != SLOTRACE_HUMAN && race != SLOTRACE_ORC && race != SLOTRACE_NIGHTELF && race != SLOTRACE_UNDEAD && race != SLOTRACE_RANDOM {
            return;
        }
        let sid = self.get_sid_from_pid(pid);
        if sid < self.m_slots.len() as u8 {
            self.m_slots[sid as usize].set_race(race | SLOTRACE_SELECTABLE);
            self.send_all_slot_info().await;
        }
    }

    async fn event_player_change_handicap(&mut self, pid: u8, handicap: u8) {
        if self.m_map.get_map_options() & MAPOPT_FIXEDPLAYERSETTINGS > 0 {
            return;
        }
        if handicap != 50 && handicap != 60 && handicap != 70 && handicap != 80 && handicap != 90 && handicap != 100 {
            return;
        }
        let sid = self.get_sid_from_pid(pid);
        if sid < self.m_slots.len() as u8 {
            self.m_slots[sid as usize].set_handicap(handicap);
            self.send_all_slot_info().await;
        }
    }

    pub async fn event_player_drop_request(&mut self, name: String) {
        if self.m_lagging {
            log_info(&format!("[GAME: {}] player [{}] voted to drop laggers", self.m_game_name, name));
            self.send_all_chat(self.m_language.player_voted_to_drop_laggers(&name)).await;

            let mut votes = 0;
            for p in self.m_players.iter() {
                if p.get_drop_vote() {
                    votes += 1;
                }
            }
            if (votes as f32 / self.m_players.len() as f32) > 0.49 {
                self.stop_laggers(self.m_language.lagged_out_dropped_by_vote());
            }
        }
    }

    pub async fn set_download_started(&mut self, pid: u8, download_status: bool) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_download_started(download_status);
        }
    }

    pub async fn set_started_downloading_ticks(&mut self, pid: u8, ticks: u32) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_started_downloading_ticks(ticks);
        }
    }

    pub async fn set_last_map_part_acked(&mut self, pid: u8, map_size: u32) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_last_map_part_acked(map_size);
        }
    }

    pub async fn set_delete_me(&mut self, pid: u8, delete_me: bool) {
        println!("set_delete_me2: {}", delete_me);

        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_delete_me(delete_me);
        }
    }

    pub async fn set_gproxy_disconnect_notice_sent(&mut self, pid: u8, delete_me: bool) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_gproxy_disconnect_notice_sent(delete_me);
        }
    }

    pub async fn set_last_gproxy_wait_notice_sent_time(&mut self, pid: u8, _p: u32) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_last_gproxy_wait_notice_sent_time(_p);
        }
    }

    pub async fn set_left_reason(&mut self, pid: u8, reason: String) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_left_reason(reason);
        }
    }

    pub async fn set_left_code(&mut self, pid: u8, code: u32) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_left_code(code);
        }
    }

    pub async fn set_download_finished(&mut self, pid: u8, download_finished: bool) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_download_finished(download_finished);
        }
    }

    pub async fn set_finished_downloading_time(&mut self, pid: u8, time: u32) {
        let player = self.get_player_from_pid_mut(pid);
        if let Some(player) = player {
            player.set_finished_downloading_time(time);
        }
    }

    pub async fn event_player_map_size(&mut self, map_size: &CIncomingMapSize, pid: u8, name: String, download_started: bool,
        started_downloading_ticks: u32
    ) {
        if self.m_game_loading || self.m_game_loaded {
            return;
        }
        let map_size_value = byte_array_to_uint32(&self.m_map.get_map_size(), false, 0);
        if map_size.get_size_flag() != 1 || map_size.get_map_size() != map_size_value {
            if true {
                let map_data = self.m_map.get_map_data();
                if !map_data.is_empty() {
                    if true {
                        if !download_started && map_size.get_size_flag() == 1 {
                            log_info(&format!("[GAME: {}] map download started for player [{}]", self.m_game_name, name));
                            self.send_pid(pid, self.m_protocol.SEND_W3GS_STARTDOWNLOAD(self.get_host_pid())).await;
                            self.set_download_started(pid, true).await;
                            self.set_started_downloading_ticks(pid, get_ticks() as u32).await;
                        } else {
                            self.set_last_map_part_acked(pid, map_size.get_map_size()).await;
                        }
                    }
                } else {
                    println!("on map_size");

                    self.set_delete_me(pid, true).await;
                    self.set_left_reason(pid, "doesn't have the map and there is no local copy of the map to send".to_owned()).await;
                    self.set_left_code(pid, PLAYERLEAVE_LOBBY.into()).await;
                    self.open_slot(self.get_sid_from_pid(pid), false).await;
                }
            } else {
                self.set_delete_me(pid, true).await;
                self.set_left_reason(pid, "doesn't have the map and map downloads are disabled".to_owned()).await;
                self.set_left_code(pid, PLAYERLEAVE_LOBBY.into()).await;
                self.open_slot(self.get_sid_from_pid(pid), false).await;
            }
        } else {
            if download_started {
                let seconds = (get_ticks() as u32 - started_downloading_ticks) as f32 / 1000.0;
                let rate = map_size_value as f32 / 1024.0 / seconds;
                log_info(&format!("[GAME: {}] map download finished for player [{}] in {} seconds", self.m_game_name, name,seconds));
                self.send_all_chat(self.m_language.player_downloaded_the_map(&name, &seconds.to_string(), &rate.to_string())).await;
                self.set_download_finished(pid, true).await;
                self.set_finished_downloading_time(pid, get_time()).await;
                //self.m_ghost.m_callables.push(self.m_ghost.m_db.threaded_download_add(&self.m_map.get_map_path(), map_size_value, &player.get_name(), &player.get_external_ip_string(), if player.get_spoofed() { 1 } else { 0 }, &player.get_spoofed_realm(), get_ticks() - player.get_started_downloading_ticks()));
            }
        }

        let mut new_download_status = ((map_size.get_map_size() as f32 / map_size_value as f32) * 100.0) as u8;
        if new_download_status > 100 {
            new_download_status = 100;
        }
        let sid = self.get_sid_from_pid(pid);
        if sid < self.m_slots.len() as u8 && self.m_slots[sid as usize].download_status() != new_download_status {
            self.m_slots[sid as usize].set_download_status(new_download_status);
            self.send_all_slot_info_s().await;
        }
    }

    pub async fn event_player_pong_to_host(&mut self, _pong: u32, reserved: bool, pid: u8, ping: u32, delete_me: bool, num_pings: u8, name: String) {
        if !self.m_game_loading && !self.m_game_loaded && !delete_me && !reserved && num_pings >= 3 && ping > 599 {
            self.send_all_chat(self.m_language.autokicking_player_for_excessive_ping(&name, &ping.to_string())).await;
            println!("on event_player_pong_to_host");

            self.set_delete_me(pid ,true).await;
            self.set_left_reason(pid,format!("was autokicked for excessive ping of {}", ping)).await;
            self.set_left_code(pid, PLAYERLEAVE_LOBBY.into()).await;
            self.open_slot(self.get_sid_from_pid(pid), false).await;
        }
    }

    async fn event_game_refreshed(&mut self, _server: &str) {
        if self.m_refresh_rehosted {
            self.send_all_chat(self.m_language.rehost_was_successful()).await;
            self.m_refresh_rehosted = false;
        }
    }

    async fn event_game_started(&mut self) {
        log_info(&format!("[GAME: {}] started loading with {} players", self.m_game_name, self.get_num_human_players()));

        if !self.m_hcl_command_string.is_empty() && self.m_hcl_command_string.len() <= self.get_slots_occupied().try_into().unwrap() {
            let hcl_chars = "abcdefghijklmnopqrstuvwxyz0123456789 -=,.";
            if self.m_hcl_command_string.chars().all(|c| hcl_chars.contains(c)) {
                let mut encoding_map = [0u8; 256];
                let mut j = 0;
                for i in 0..256 {
                    if j == 0 || j == 50 || j == 60 || j == 70 || j == 80 || j == 90 || j == 100 {
                        j += 1;
                    }
                    encoding_map[i] = j;
                    j += 1;
                }

                let mut current_slot = 0;
                for c in self.m_hcl_command_string.chars() {
                    while self.m_slots[current_slot].slot_status() != SLOTSTATUS_OCCUPIED {
                        current_slot += 1;
                    }
                    let handicap_index = (self.m_slots[current_slot].handicap() - 50) / 10;
                    let char_index = hcl_chars.find(c).unwrap() as u8;
                    self.m_slots[current_slot].set_handicap(encoding_map[(handicap_index + char_index * 6) as usize]);
                    current_slot += 1;
                }
                self.send_all_slot_info_s().await;
                log_info(&format!("[GAME: {}] successfully encoded HCL command string [{}]", self.m_game_name, self.m_hcl_command_string));
            } else {
                log_info(&format!("[GAME: {}] encoding HCL command string [{}] failed because it contains invalid characters", self.m_game_name, self.m_hcl_command_string));
            }
        } else if !self.m_hcl_command_string.is_empty() {
            log_info(&format!("[GAME: {}] encoding HCL command string [{}] failed because there aren't enough occupied slots", self.m_game_name, self.m_hcl_command_string));
        }

        if self.m_slot_info_changed {
            self.send_all_slot_info_s().await;
        }

        self.m_started_loading_ticks = get_ticks() as u32;
        self.m_last_lag_screen_reset_time = get_time();
        self.m_game_loading = true;
        self.send_all(self.m_protocol.SEND_W3GS_COUNTDOWN_START()).await;
        self.delete_virtual_host().await;
        self.send_all(self.m_protocol.SEND_W3GS_COUNTDOWN_END()).await;

        if self.m_fake_player_pid != 255 {
            self.send_all(self.m_protocol.SEND_W3GS_GAMELOADED_OTHERS(self.m_fake_player_pid)).await;
        }

        
        println!("PREPARE SAVING!");

        let maybe_current_game = {
            let current_game_guard = CURRENT_GAME.read().await;
            println!("LOCKED READ CURRENT_GAME");
            current_game_guard.as_ref().map(|game| Arc::clone(game))
        };

        println!("GOT CURRENT_GAME REF");

        if let Some(current_game) = maybe_current_game {
            let mut games = m_GAMES.write().await;
            println!("LOCKED WRITE GAMES");
            games.push(current_game);
            println!("GAMES LENGTH: {:?}", games.len());
        }

        println!("BEFORE START PLAYER");
        self.m_start_players = self.get_num_human_players();
        println!("AFTER START PLAYER");

        self.m_socket.shutdown();


        self.m_potentials.clear();
        println!("AFTER CLEAR");

        let mut current_game_del = CURRENT_GAME.write().await;
        *current_game_del = None;
        println!("DELETED!");


        let bnets = m_BNETs.read().await;
        for bnet_arc in bnets.iter() {
            let mut bnet = bnet_arc.lock().await;
            bnet.queue_game_uncreate().await;
            bnet.queue_enter_chat().await;
        }

    }

    async fn event_game_loaded(&mut self) {
        log_info(&format!("[GAME: {}] finished loading with {} players", self.m_game_name, self.get_num_human_players()));

        let mut shortest: Option<&GamePlayer> = None;
        let mut longest: Option<&GamePlayer> = None;
        for p in self.m_players.iter() {
            println!("Name: {}, Socket: {:?}", p.get_name(), p.m_socket);
            if shortest.is_none() || p.get_finished_loading_ticks() < shortest.unwrap().get_finished_loading_ticks() {
                shortest = Some(p);
            }
            if longest.is_none() || p.get_finished_loading_ticks() > longest.unwrap().get_finished_loading_ticks() {
                longest = Some(p);
            }
        }

        if let (Some(shortest), Some(longest)) = (shortest, longest) {
            let shortest_name_clone = shortest.get_name().clone();
            let shortest_time_clone = ((shortest.get_finished_loading_ticks() - self.m_started_loading_ticks) as f32 / 1000.0).to_string();
            let longest_name_clone = longest.get_name().clone();
            let longest_time_clone = ((longest.get_finished_loading_ticks() - self.m_started_loading_ticks) as f32 / 1000.0).to_string();

            self.send_all_chat(self.m_language.shortest_load_by_player(&shortest_name_clone, &shortest_time_clone)).await;
            self.send_all_chat(self.m_language.longest_load_by_player(&longest_name_clone, &longest_time_clone)).await;
        }

        let player_data: Vec<_> = self
            .m_players
            .iter()
            .map(|p| (p.get_name().clone(), (p.get_finished_loading_ticks() - self.m_started_loading_ticks) as f32 / 1000.0))
            .collect();

        for (name, loading_time) in player_data {
            self.send_all_chat(self.m_language.your_loading_time_was(&loading_time.to_string())).await;
        }

        let mut file = File::open("gameloaded.txt").ok();
        if let Some(mut file) = file {
            let mut count = 0;
            let mut line = String::new();
            let mut reader = BufReader::new(file);
            while count < 8 && reader.read_line(&mut line).is_ok() {
                if line.is_empty() {
                    self.send_all_chat(" ".to_owned()).await;
                } else if !line.is_empty() {
                    self.send_all_chat((&line.trim()).to_string()).await;
                }
                count += 1;
                line.clear();
            }
        }
    }

    pub fn get_sid_from_pid(&self, pid: u8) -> u8 {
        if self.m_slots.len() > 255 {
            return 255;
        }
        for (i, slot) in self.m_slots.iter().enumerate() {
            if slot.pid() == pid {
                return i as u8;
            }
        }
        255
    }

    pub fn get_player_from_pid(&self, pid: u8) -> Option<&GamePlayer> {
        self.m_players.iter().find(|p| !p.get_left_message_sent() && p.get_pid() == pid)
    }
    pub fn get_player_from_pid_mut(&mut self, pid: u8) -> Option<&mut GamePlayer> {
        self.m_players.iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == pid)
    }

    pub fn get_player_from_sid(&self, sid: u8) -> Option<&GamePlayer> {
        if (sid as usize) < self.m_slots.len() {
            self.get_player_from_pid(self.m_slots[sid as usize].pid())
        } else {
            None
        }
    }

    pub fn get_player_from_sid_mut(&mut self, sid: u8) -> Option<&mut GamePlayer> {
        if (sid as usize) < self.m_slots.len() {
            let pid = self.m_slots[sid as usize].pid();
            self.m_players.iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == pid)
        } else {
            None
        }
    }

    pub fn get_player_from_name(&self, name: String, sensitive: bool) -> Option<&GamePlayer> {
        let name_lower = if sensitive { name } else { name.to_lowercase() };
        self.m_players.iter().find(|p| {
            if p.get_left_message_sent() {
                return false;
            }
            let test_name = if sensitive { p.get_name() } else { p.get_name().to_lowercase() };
            test_name == name_lower
        })
    }

    pub fn get_player_from_name_partial(&self, name: String) -> (u32, Option<&GamePlayer>) {
        let name_lower = name.to_lowercase();
        let mut matches = 0;
        let mut found_player = None;
        for p in self.m_players.iter() {
            if p.get_left_message_sent() {
                continue;
            }
            let test_name = p.get_name().to_lowercase();
            if test_name.contains(&name_lower) {
                matches += 1;
                found_player = Some(p);
                if test_name == name_lower {
                    matches = 1;
                    break;
                }
            }
        }
        (matches, found_player)
    }

    pub fn get_player_from_colour(&self, colour: u8) -> Option<&GamePlayer> {
        for (i, slot) in self.m_slots.iter().enumerate() {
            if slot.colour() == colour {
                return self.get_player_from_sid(i as u8);
            }
        }
        None
    }

    pub fn get_new_pid(&self) -> u8 {
        for test_pid in 1..255 {
            if test_pid == self.m_virtual_host_pid || test_pid == self.m_fake_player_pid {
                continue;
            }
            if !self.m_players.iter().any(|p| !p.get_left_message_sent() && p.get_pid() == test_pid) {
                return test_pid;
            }
        }
        255
    }

    pub fn get_new_colour(&self) -> u8 {
        for test_colour in 0..12 {
            if !self.m_slots.iter().any(|s| s.colour() == test_colour) {
                return test_colour;
            }
        }
        12
    }

    pub fn pids(&self) -> Vec<u8> {
        self.m_players.iter().filter(|p| !p.get_left_message_sent()).map(|p| p.get_pid()).collect()
    }

    pub fn pids_exclude(&self, exclude_pid: u8) -> Vec<u8> {
        self.m_players.iter().filter(|p| !p.get_left_message_sent() && p.get_pid() != exclude_pid).map(|p| p.get_pid()).collect()
    }

    pub fn get_host_pid(&self) -> u8 {
        if self.m_virtual_host_pid != 255 {
            return self.m_virtual_host_pid;
        }
        if self.m_fake_player_pid != 255 {
            return self.m_fake_player_pid;
        }
        if let Some(p) = self.m_players.iter().find(|p| !p.get_left_message_sent() && self.is_owner(p.get_name())) {
            return p.get_pid();
        }
        self.m_players.iter().find(|p| !p.get_left_message_sent()).map(|p| p.get_pid()).unwrap_or(255)
    }

    pub fn get_empty_slot(&self, reserved: bool) -> u8 {
        if self.m_slots.len() > 255 {
            return 255;
        }
        for (i, slot) in self.m_slots.iter().enumerate() {
            if slot.slot_status() == SLOTSTATUS_OPEN {
                return i as u8;
            }
        }
        if reserved {
            for (i, slot) in self.m_slots.iter().enumerate() {
                if slot.slot_status() == SLOTSTATUS_CLOSED {
                    return i as u8;
                }
            }
            let mut least_downloaded = 100;
            let mut least_sid = 255;
            for (i, slot) in self.m_slots.iter().enumerate() {
                if let Some(player) = self.get_player_from_sid(i as u8) {
                    if !player.get_reserved() && slot.download_status() < least_downloaded {
                        least_downloaded = slot.download_status();
                        least_sid = i as u8;
                    }
                }
            }
            if least_sid != 255 {
                return least_sid;
            }
            for (i, slot) in self.m_slots.iter().enumerate() {
                if let Some(player) = self.get_player_from_sid(i as u8) {
                    if !player.get_reserved() {
                        return i as u8;
                    }
                }
            }
        }
        
        255
    }

    pub fn get_empty_slot_team(&self, team: u8, pid: u8) -> u8 {
        if self.m_slots.len() > 255 {
            return 255;
        }
        let mut start_slot = self.get_sid_from_pid(pid);
        if start_slot as usize >= self.m_slots.len() {
            start_slot = 0;
        } else if self.m_slots[start_slot as usize].team() != team {
            start_slot = 0;
        }
        
        for i in start_slot..self.m_slots.len() as u8 {
            if self.m_slots[i as usize].slot_status() == SLOTSTATUS_OPEN && self.m_slots[i as usize].team() == team {
                return i;
            }
        }
        for i in 0..start_slot {
            if self.m_slots[i as usize].slot_status() == SLOTSTATUS_OPEN && self.m_slots[i as usize].team() == team {
                return i;
            }
        }
    
        255
    }

    pub async fn swap_slots(&mut self, sid1: u8, sid2: u8) {
        if sid1 as usize >= self.m_slots.len() || sid2 as usize >= self.m_slots.len() || sid1 == sid2 {
            return;
        }
        let slot1 = self.m_slots[sid1 as usize].clone();
        let slot2 = self.m_slots[sid2 as usize].clone();
        if self.m_map.get_map_options() & MAPOPT_FIXEDPLAYERSETTINGS != 0 {
            self.m_slots[sid1 as usize] = GameSlot::new(slot2.pid(), slot2.download_status(), slot2.slot_status(), slot2.computer(), slot1.team(), slot1.colour(), slot1.race(), slot2.computer_type(), slot1.handicap());
            self.m_slots[sid2 as usize] = GameSlot::new(slot1.pid(), slot1.download_status(), slot1.slot_status(), slot1.computer(), slot2.team(), slot2.colour(), slot2.race(), slot1.computer_type(), slot2.handicap());
        } else {
            if self.m_map.get_map_options() & MAPOPT_CUSTOMFORCES != 0 {
                self.m_slots[sid1 as usize] = GameSlot::new(slot2.pid(), slot2.download_status(), slot2.slot_status(), slot2.computer(), slot2.team(), slot2.colour(), slot2.race(), slot2.computer_type(), slot2.handicap());
                self.m_slots[sid2 as usize] = GameSlot::new(slot1.pid(), slot1.download_status(), slot1.slot_status(), slot1.computer(), slot1.team(), slot1.colour(), slot1.race(), slot1.computer_type(), slot1.handicap());
            } else {
                self.m_slots[sid1 as usize] = slot2;
                self.m_slots[sid2 as usize] = slot1;
            }
        }
        self.send_all_slot_info_s().await;
    }

    pub async fn open_slot(&mut self, sid: u8, kick: bool) {
        if sid as usize >= self.m_slots.len() {
            return;
        }
        if kick {
            if let Some(player) = self.m_players.iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == self.m_slots[sid as usize].pid()) {
                println!("on open_slot");
                player.set_delete_me(true);
                player.set_left_reason("was kicked when opening a slot".to_string());
                player.set_left_code(PLAYERLEAVE_LOBBY.into());
            }
        }
        let slot = self.m_slots[sid as usize].clone();
        self.m_slots[sid as usize] = GameSlot::new(0, 255, SLOTSTATUS_OPEN, 0, slot.team(), slot.colour(), slot.race(), slot.computer_type(), slot.handicap());
        self.send_all_slot_info_s().await;
    }

    pub async fn close_slot(&mut self, sid: u8, kick: bool) {
        if sid as usize >= self.m_slots.len() {
            return;
        }
        if kick {
            if let Some(player) = self.m_players.iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == self.m_slots[sid as usize].pid()) {
                println!("on close_slot");
                
                player.set_delete_me(true);
                player.set_left_reason("was kicked when closing a slot".to_string());
                player.set_left_code(PLAYERLEAVE_LOBBY as u32);
            }
        }
        let slot = self.m_slots[sid as usize].clone();
        self.m_slots[sid as usize] = GameSlot::new(0, 255, SLOTSTATUS_CLOSED, 0, slot.team(), slot.colour(), slot.race(), slot.computer_type(), slot.handicap());
        self.send_all_slot_info_s().await;
    }

    pub async fn computer_slot(&mut self, sid: u8, skill: u8, kick: bool) {
        if sid as usize >= self.m_slots.len() || skill >= 3 {
            return;
        }
        if kick {
            if let Some(slot_pid) = self.m_slots.get(sid as usize).map(|slot| slot.pid()) {
                if let Some(player) = self.m_players.iter_mut().find(|p| !p.get_left_message_sent() && p.get_pid() == slot_pid) {
                    println!("on computer slot");

                    player.set_delete_me(true);
                    player.set_left_reason("was kicked when creating a computer in a slot".to_string());
                    player.set_left_code(PLAYERLEAVE_LOBBY as u32);
                }
            }
        }
        let slot = self.m_slots[sid as usize].clone();
        self.m_slots[sid as usize] = GameSlot::new(0, 100, SLOTSTATUS_OCCUPIED, 1, slot.team(), slot.colour(), slot.race(), skill, slot.handicap());
        self.send_all_slot_info().await;
    }

    pub async fn colour_slot(&mut self, sid: u8, colour: u8) {
        if sid as usize >= self.m_slots.len() || colour >= 12 {
            return;
        }
        let mut taken = None;
        for (i, slot) in self.m_slots.iter().enumerate() {
            if slot.colour() == colour {
                taken = Some(i as u8);
                break;
            }
        }
        if let Some(taken_sid) = taken {
            if self.m_slots[taken_sid as usize].slot_status() != SLOTSTATUS_OCCUPIED {
                let sid_colour = self.m_slots[sid as usize].colour();
                self.m_slots[taken_sid as usize].set_colour(sid_colour);
                self.m_slots[sid as usize].set_colour(colour);
                self.send_all_slot_info_s().await;
            }
        } else {
            self.m_slots[sid as usize].set_colour(colour);
            self.send_all_slot_info_s().await;
        }
    }

    pub async fn open_all_slots(&mut self) {
        let mut changed = false;
        for slot in &mut self.m_slots {
            if slot.slot_status() == SLOTSTATUS_CLOSED {
                slot.set_slot_status(SLOTSTATUS_OPEN);
                changed = true;
            }
        }
        if changed {
            self.send_all_slot_info().await;
        }
    }

    pub async fn close_all_slots(&mut self) {
        let mut changed = false;
        for slot in &mut self.m_slots {
            if slot.slot_status() == SLOTSTATUS_OPEN {
                slot.set_slot_status(SLOTSTATUS_CLOSED);
                changed = true;
            }
        }
        if changed {
            self.send_all_slot_info().await;
        }
    }

    pub async fn shuffle_slots(&mut self) {
        let mut player_slots: Vec<GameSlot> = self.m_slots.iter().filter(|s| s.slot_status() == SLOTSTATUS_OCCUPIED && s.computer() == 0 && s.team() != 12).cloned().collect();
        if self.m_map.get_map_options() & MAPOPT_CUSTOMFORCES != 0 {
            let mut sids: Vec<usize> = (0..player_slots.len()).collect();
            sids.shuffle(&mut rng());
            let mut slots = Vec::new();
            for i in sids {
                slots.push(GameSlot::new(player_slots[i].pid(), player_slots[i].download_status(), player_slots[i].slot_status(), player_slots[i].computer(), player_slots[i].team(), player_slots[i].colour(), player_slots[i].race(), player_slots[i].computer_type(), player_slots[i].handicap()));
            }
            player_slots = slots;
        } else {
            player_slots.shuffle(&mut rng());
        }
        let mut slots = Vec::new();
        let mut current_player = player_slots.into_iter();
        for slot in &self.m_slots {
            if slot.slot_status() == SLOTSTATUS_OCCUPIED && slot.computer() == 0 && slot.team() != 12 {
                slots.push(current_player.next().unwrap());
            } else {
                slots.push(slot.clone());
            }
        }
        self.m_slots = slots;
        self.send_all_slot_info().await;
    }

    pub fn balance_slots_recursive(&self, player_ids: Vec<u8>, team_sizes: &mut [u8; 12], player_scores: &mut [f64; 13], start_team: u8) -> Vec<u8> {
        fn next_combination(mut v: &mut [u8], mid: usize) -> bool {
            if mid >= v.len() {
                return false;
            }
            v[0..mid].sort();
            let mut i = mid;
            while i > 0 && v[i - 1] >= v[i] {
                i -= 1;
            }
            if i == 0 {
                return false;
            }
            let mut j = v.len() - 1;
            while j >= i && v[j] <= v[i - 1] {
                j -= 1;
            }
            v.swap(i - 1, j);
            v[i..].sort();
            true
        }

        let mut best_ordering = player_ids.clone();
        let mut best_difference = -1.0;
        for i in start_team..12 {
            if team_sizes[i as usize] > 0 {
                let mid = team_sizes[i as usize] as usize;
                let mut temp_ids = player_ids.clone();
                while next_combination(&mut temp_ids, mid) {
                    let sub_ordering = self.balance_slots_recursive(temp_ids[mid..].to_vec(), team_sizes, player_scores, i + 1);
                    let mut test_ordering = temp_ids[..mid].to_vec();
                    test_ordering.extend_from_slice(&sub_ordering);
                    let mut current_pid = test_ordering.iter();
                    let mut team_scores = [0.0; 12];
                    for j in start_team..12 {
                        for _ in 0..team_sizes[j as usize] {
                            if let Some(pid) = current_pid.next() {
                                team_scores[j as usize] += player_scores[*pid as usize];
                            }
                        }
                    }
                    let mut largest_difference = 0.0;
                    for j in start_team..12 {
                        if team_sizes[j as usize] > 0 {
                            for k in j + 1..12 {
                                if team_sizes[k as usize] > 0 {
                                    let difference = (team_scores[j as usize] - team_scores[k as usize]).abs();
                                    if difference > largest_difference {
                                        largest_difference = difference;
                                    }
                                }
                            }
                        }
                    }
                    if best_difference < 0.0 || largest_difference < best_difference {
                        best_ordering = test_ordering;
                        best_difference = largest_difference;
                    }
                }
            }
        }
        best_ordering
    }

    pub async fn add_to_spoofed(&mut self, server: String, name: String, send_message: bool) {
        if let Some(player) = self.m_players.iter_mut().find(|p| !p.get_left_message_sent() && p.get_name() == name) {
            player.set_spoofed_realm(server.clone());
            player.set_spoofed(true);
            if send_message {
                self.send_all_chat(self.m_language.spoof_check_accepted_for(&server, &name)).await;
            }
        }
    }

    pub fn add_to_reserved(&mut self, name: String) {
        let name_lower = name.to_lowercase();
        if !self.m_reserved.iter().any(|n| n == &name_lower) {
            self.m_reserved.push(name_lower.clone());
            for player in &mut self.m_players {
                if player.get_name().to_lowercase() == name_lower {
                    player.set_reserved(true);
                }
            }
        }
    }

    pub fn is_owner(&self, name: String) -> bool {
        name.to_lowercase() == self.m_owner_name.to_lowercase()
    }

    pub fn is_reserved(&self, name: String) -> bool {
        self.m_reserved.iter().any(|n| n == &name.to_lowercase())
    }

    pub fn is_downloading(&self) -> bool {
        self.m_players.iter().any(|p| p.get_download_started() && !p.get_download_finished())
    }

    pub fn is_game_data_saved(&self) -> bool {
        true
    }

    pub fn save_game_data(&mut self) {}

    pub async fn start_count_down(&mut self, force: bool) {
        if self.m_count_down_started {
            return;
        }
        if force {
            self.m_count_down_started = true;
            self.m_count_down_counter = 5;
        } else {
            if self.m_hcl_command_string.len() as u32 > self.get_slots_occupied() {
                self.send_all_chat(self.m_language.the_hcl_is_too_long_use_force_to_start()).await;
                return;
            }
            let mut still_downloading = String::new();
            for slot in &self.m_slots {
                if slot.slot_status() == SLOTSTATUS_OCCUPIED && slot.computer() == 0 && slot.download_status() != 100 {
                    if let Some(player) = self.get_player_from_pid(slot.pid()) {
                        if still_downloading.is_empty() {
                            still_downloading = player.get_name();
                        } else {
                            still_downloading.push_str(", ");
                            still_downloading.push_str(&player.get_name());
                        }
                    }
                }
            }
            if !still_downloading.is_empty() {
                self.send_all_chat(self.m_language.players_still_downloading(&still_downloading)).await;
            }
            let mut not_pinged = String::new();
            for player in &self.m_players {
                if !player.get_reserved() && player.get_num_pings() < 3 {
                    if not_pinged.is_empty() {
                        not_pinged = player.get_name();
                    } else {
                        not_pinged.push_str(", ");
                        not_pinged.push_str(&player.get_name());
                    }
                }
            }
            if !not_pinged.is_empty() {
                self.send_all_chat(self.m_language.players_not_yet_pinged(&not_pinged)).await;
            }
            if still_downloading.is_empty() && not_pinged.is_empty() {
                self.m_count_down_started = true;
                self.m_count_down_counter = 5;
            }
        }
    }

    pub async fn start_count_down_auto(&mut self, require_spoof_checks: bool) {
        if self.m_count_down_started {
            return;
        }
        if self.get_num_human_players() < self.m_auto_start_players {
            self.send_all_chat(self.m_language.waiting_for_players_before_auto_start(
                &self.m_auto_start_players.to_string(),
                &(self.m_auto_start_players - self.get_num_human_players()).to_string(),
            )).await;
            return;
        }
        let mut still_downloading = String::new();
        for slot in &self.m_slots {
            if slot.slot_status() == SLOTSTATUS_OCCUPIED && slot.computer() == 0 && slot.download_status() != 100 {
                if let Some(player) = self.get_player_from_pid(slot.pid()) {
                    if still_downloading.is_empty() {
                        still_downloading = player.get_name();
                    } else {
                        still_downloading.push_str(", ");
                        still_downloading.push_str(&player.get_name());
                    }
                }
            }
        }
        if !still_downloading.is_empty() {
            self.send_all_chat(self.m_language.players_still_downloading(&still_downloading)).await;
            return;
        }
        let mut not_spoof_checked = String::new();
        if require_spoof_checks {
            for player in &self.m_players {
                if !player.get_spoofed() {
                    if not_spoof_checked.is_empty() {
                        not_spoof_checked = player.get_name();
                    } else {
                        not_spoof_checked.push_str(", ");
                        not_spoof_checked.push_str(&player.get_name());
                    }
                }
            }
            if !not_spoof_checked.is_empty() {
                self.send_all_chat(self.m_language.players_not_yet_spoof_checked(&not_spoof_checked)).await;
            }
        }
        let mut not_pinged = String::new();
        for player in &self.m_players {
            if !player.get_reserved() && player.get_num_pings() < 3 {
                if not_pinged.is_empty() {
                    not_pinged = player.get_name();
                } else {
                    not_pinged.push_str(", ");
                    not_pinged.push_str(&player.get_name());
                }
            }
        }
        if !not_pinged.is_empty() {
            self.send_all_chat(self.m_language.players_not_yet_pinged_auto_start(&not_pinged)).await;
            return;
        }
        if still_downloading.is_empty() && not_spoof_checked.is_empty() && not_pinged.is_empty() {
            self.m_count_down_started = true;
            self.m_count_down_counter = 10;
        }
    }

    pub fn stop_players(&mut self, reason: String) {
        println!("stop_players");
        for player in self.m_players.iter_mut() {
            player.set_delete_me(true);
            player.set_left_reason(reason.clone());
            player.set_left_code(PLAYERLEAVE_LOST as u32);
        }
    }

    pub fn stop_laggers(&mut self, reason: String) {
        for player in self.m_players.iter_mut() {
            if player.get_lagging() {
                println!("stop_laggers");
                player.set_delete_me(true);
                player.set_left_reason(reason.clone());
                player.set_left_code(PLAYERLEAVE_DISCONNECT as u32);
            }
        }
    }

    pub async fn create_virtual_host(&mut self) {
        if self.m_virtual_host_pid != 255 {
            return;
        }
        self.m_virtual_host_pid = self.get_new_pid();
        let ip = vec![0, 0, 0, 0];
        self.send_all(self.m_protocol.SEND_W3GS_PLAYERINFO(self.m_virtual_host_pid, self.m_virtual_host_name.clone(), ip.clone(), ip)).await;
    }

    pub async fn delete_virtual_host(&mut self) {
        if self.m_virtual_host_pid == 255 {
            return;
        }
        self.send_all(self.m_protocol.SEND_W3GS_PLAYERLEAVE_OTHERS(self.m_virtual_host_pid, PLAYERLEAVE_LOBBY.into())).await;
        self.m_virtual_host_pid = 255;
    }

    pub async fn create_fake_player(&mut self) {
        if self.m_fake_player_pid != 255 {
            return;
        }
        let sid = self.get_empty_slot(false);
        if sid as usize >= self.m_slots.len() {
            return;
        }
        if self.get_num_players() >= 11 {
            self.delete_virtual_host().await;
        }
        self.m_fake_player_pid = self.get_new_pid();
        let ip = vec![0, 0, 0, 0];
        self.send_all(self.m_protocol.SEND_W3GS_PLAYERINFO(self.m_fake_player_pid, "FakePlayer".to_string(), ip.clone(), ip)).await;
        let slot = self.m_slots[sid as usize].clone();
        self.m_slots[sid as usize] = GameSlot::new(self.m_fake_player_pid, 100, SLOTSTATUS_OCCUPIED, 0, slot.team(), slot.colour(), slot.race(), slot.computer_type(), slot.handicap());
        self.send_all_slot_info().await;
    }

    pub async fn delete_fake_player(&mut self) {
        if self.m_fake_player_pid == 255 {
            return;
        }
        for slot in &mut self.m_slots {
            if slot.pid() == self.m_fake_player_pid {
                *slot = GameSlot::new(0, 255, SLOTSTATUS_OPEN, 0, slot.team(), slot.colour(), slot.race(), slot.computer_type(), slot.handicap());
            }
        }
        self.send_all(self.m_protocol.SEND_W3GS_PLAYERLEAVE_OTHERS(self.m_fake_player_pid, PLAYERLEAVE_LOBBY.into())).await;
        self.send_all_slot_info().await;
        self.m_fake_player_pid = 255;
    }
}