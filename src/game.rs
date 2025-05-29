use crate::lang::*;
use crate::ghost::*;
use crate::game_base::*;
use crate::map::*;
use crate::socket::*;
use crate::gameprotocol::*;
use std::collections::HashMap;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use rand::Rng;



#[derive(Debug, Clone)]

pub struct Game {
    pub base: BaseGame
}

impl Game {
    pub fn new(
        ghost: Arc<Mutex<Ghost>>,
        map: Map,
        host_port: u16,
        game_state: u8,
        game_name: String,
        owner_name: String,
        creator_name: String,
        creator_server: String,
        host_counter: u32
    ) -> Self {
        let mut rng = rand::rng();
        let entry_key: u32 = rng.random();
        Game {
            base: BaseGame {
                m_potentials: Vec::new(),
                m_language: Language::new(),
                m_ghost: ghost.clone(),
                m_socket: TcpServer::new(), // Assuming TcpServer has a new() method
                m_protocol: GameProtocol::new(ghost), // Assuming GameProtocol has a new() method
                m_slots: Vec::new(),
                m_players: Vec::new(),
                m_actions: VecDeque::new(),
                m_reserved: Vec::new(),
                m_ignored_names: HashSet::new(),
                m_ip_black_list: HashSet::new(),
                m_enforce_slots: Vec::new(),
                m_map: map,

                m_exiting: false,
                m_saving: false,
                m_host_port: host_port,
                m_game_state: game_state,
                m_virtual_host_pid: 255,
                m_fake_player_pid: 255,
                m_gproxy_empty_actions: 0,
                m_game_name: game_name,
                m_last_game_name: String::new(),
                m_virtual_host_name: "iCCup".to_owned(),
                m_owner_name: owner_name,
                m_creator_name: creator_name,
                m_creator_server: creator_server,
                m_announce_message: String::new(),
                m_stat_string: String::new(),
                m_kick_vote_player: String::new(),
                m_hcl_command_string: String::new(),
                m_random_seed: get_ticks() as u32,
                m_host_counter: host_counter,
                m_entry_key: entry_key,
                m_latency: 20,
                m_sync_limit: 200,
                m_sync_counter: 0,
                m_game_ticks: 0,
                m_creation_time: get_time(),
                m_last_ping_time: get_time(),
                m_last_refresh_time: get_time(),
                m_last_download_ticks: get_time(),
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
                m_last_reserved_seen: get_time(),
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
    }

    pub async fn update(&mut self) -> bool {
        return self.base.update().await;
    }


}