use crate::game;
use crate::logger::log_info;
use crate::logger::log_warning;
use crate::socket::*;
use crate::gpsprotocol::*;
use crate::config;
#[allow(unused_imports)]
use crate::util::*;
use crate::crc32::*;
use crate::game::*;
use crate::game_base::*;
use mpq::{Archive, File};
use tokio::time::timeout;
use crate::map::*;
use crate::sha1::*;
use crate::gameprotocol::*;
use crate::bnet::*;
use tokio::net::UdpSocket;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};


use std::time::Duration;
use std::time::{Instant};
use once_cell::sync::Lazy;

static START: Lazy<Instant> = Lazy::new(Instant::now);

pub static CURRENT_GAME: Lazy<RwLock<Option<BaseGame>>> =
    Lazy::new(|| RwLock::new(None));

// Статический счётчик для m_HostCounter
pub static HOST_COUNTER: AtomicU32 = AtomicU32::new(1);

pub fn get_ticks() -> u128 {
    START.elapsed().as_millis()
}

pub fn get_time() -> u32 {
    (get_ticks() / 1000) as u32
}

#[allow(unused)]
#[allow(non_snake_case)]
struct GProxyReconnector {
    socket: TcpClient,
    PID: u8,
    ReconnectKey: u32,
    LastPacket: u32,
    PostedTime: u32,
}

#[allow(dead_code)]
#[allow(non_snake_case)]
#[derive(Debug)]
#[derive(Default)]
pub struct Ghost {
    pub m_UDPSocket: Option<UdpSocket>,
    pub m_ReconnectSocket: TcpClient,
    pub m_ReconnectSockets: Vec<TcpClient>,
    pub m_GPSProtocol: CGPSProtocol,
    pub m_CurrentGame: Option<BaseGame>,
    pub m_CRC: CRC32,
    pub m_SHA: SHA1,
    pub m_BNETs: Vec<BNET>,
    pub m_TFT: bool,
    pub m_Exiting: bool,
    pub m_ExitingNicely: bool,
    pub m_Enabled: bool,
    pub m_Version: String,
    pub m_HostCounter: u32,
    pub m_MaxGames: u32,
    pub m_ReconnectWaitTime: u32,
    pub m_BindAddress: String,
    pub m_Games: Vec<BaseGame>
}

impl Clone for Ghost {
    fn clone(&self) -> Self {
        Ghost {
            m_UDPSocket: None,
            m_ReconnectSocket: TcpClient::new(),
            m_ReconnectSockets: Vec::new(),
            m_GPSProtocol: self.m_GPSProtocol.clone(),
            m_CurrentGame: self.m_CurrentGame.clone(),
            m_CRC: self.m_CRC.clone(),
            m_SHA: self.m_SHA.clone(),
            m_BNETs: self.m_BNETs.clone(),
            m_TFT: self.m_TFT,
            m_Exiting: self.m_Exiting,
            m_ExitingNicely: self.m_ExitingNicely,
            m_Enabled: self.m_Enabled,
            m_Version: self.m_Version.clone(),
            m_HostCounter: self.m_HostCounter,
            m_MaxGames: self.m_MaxGames,
            m_ReconnectWaitTime: self.m_ReconnectWaitTime,
            m_BindAddress: self.m_BindAddress.clone(),
            m_Games: self.m_Games.clone()
        }
    }
}

impl Ghost {
    pub async fn new() -> Self {
        Ghost {
            m_UDPSocket: Some(UdpSocket::bind("0.0.0.0:0").await.expect("Failed to bind UDP socket")),
            m_ReconnectSocket: TcpClient::new(),
            m_ReconnectSockets: Vec::new(),
            m_GPSProtocol: CGPSProtocol::new(),
            m_CurrentGame: None,
            m_CRC: CRC32::new(),
            m_SHA: SHA1::new(),
            m_BNETs: Vec::new(),
            m_TFT: false,
            m_Exiting: false,
            m_ExitingNicely: false,
            m_Enabled: false,
            m_Version: String::new(),
            m_HostCounter: 0,
            m_MaxGames: 0,
            m_ReconnectWaitTime: 0,
            m_BindAddress: "0.0.0.0".to_owned(),
            m_Games: Vec::new()
        }
    }

    pub fn add_game(&mut self, _game: BaseGame) {
        self.m_Games.push(_game);
    }

    pub fn set_current_game(&mut self, _game: Option<BaseGame>) {
        let self_ptr = self as *mut Ghost; // Сырой указатель на self
    log_info(&format!(
        "[GHOST] Setting current game: {:?}, self ptr: {:p}",
        _game.as_ref().map(|g| g.get_game_name()),
            self_ptr
        ));
        self.m_CurrentGame = _game;
    }

    pub async fn init(&mut self) {
        log_info("[GHOST] Starting initialization");
        self.m_GPSProtocol = CGPSProtocol::new();
        self.m_CRC = CRC32::new();
        self.m_CRC.initialize();
        self.m_SHA = SHA1::new();
    
        self.m_Exiting = false;
        self.m_ExitingNicely = false;
        self.m_Enabled = true;
        self.m_TFT = config::get_bool("bot_tft", true);
        self.m_Version = String::from("GhostRS v1.0");
        self.m_MaxGames = config::get_int("bot_maxgames", 10) as u32;
        self.m_HostCounter = 1;
    
        log_info("[GHOST] Loading configuration");
        let server = config::get_string("bnet_server", "wc3.theabyss.ru");
        let server_alias = config::get_string("bnet_serveralias", "The Abyss");
        let cdkeyroc = config::get_string("bnet_cdkeyroc", "");
        let cdkeytft = config::get_string("bnet_cdkeytft", "");
        let username = config::get_string("bnet_username", "BOT");
        let userpassword = config::get_string("bnet_password", "");
        let firstchannel = config::get_string("bnet_firstchannel", "iccup.pro");
    
        log_info("[GHOST] Creating BNET instance");
        let ghost_clone = (*self).clone(); // Dereference self to call clone on Ghost
        let ghost_arc = Arc::new(Mutex::new(ghost_clone));
        let bnet = BNET::new(
            Arc::clone(&ghost_arc),
            server.to_owned(),
            server_alias.to_owned(),
            "localhost".to_owned(),
            6112,
            1,
            cdkeyroc.to_owned(),
            cdkeytft.to_owned(),
            "USA".to_owned(),
            "United States".to_owned(),
            1033,
            username.to_owned(),
            userpassword.to_owned(),
            firstchannel.to_owned(),
            "".to_owned(),
            config::get_string("bnet_commandtrigger", "!").chars().next().unwrap(),
            config::get_bool("bnet_holdfriends", true),
            config::get_bool("bnet_holdclan", true),
            config::get_bool("bnet_publiccommands", false),
            config::get_int("bnet_custom_war3version", 26).try_into().unwrap(),
            extract_numbers(&config::get_string("bnet_custom_exeversion", "1 0 26 1"), 4),
            Vec::new(),
            "pvpgn".to_owned(),
            "PvPGN Realm".to_owned(),
            500,
            1,
        );
        self.m_BNETs.push(bnet);
        log_info("[GHOST] BNET instance added");
    
        log_info("[GHOST] Extracting scripts");
        self.extract_scripts().await;
        log_info("[GHOST] Initialization completed");
    }

    pub async fn update(&mut self) -> bool {
        // Collect BNET instances to avoid borrowing self.m_BNETs during the loop
        let bnets: Vec<_> = self.m_BNETs.drain(..).collect();
        
        // Update each BNET instance
        let mut updated_bnets = Vec::new();
        for mut bnet in bnets {
            bnet.update().await;
            updated_bnets.push(bnet);
        }
        self.m_BNETs.extend(updated_bnets);
        let games: Vec<_> = self.m_Games.drain(..).collect();

        let mut updated_games = Vec::new();
        for mut game in games {
            if game.update().await {
                log_info(&format!("[GHOST] deleting game [{}]", game.get_game_name()));
                
            }
            updated_games.push(game);
        }
        
        
        let need_init = {
            let current_game = CURRENT_GAME.read().await;
            current_game.as_ref().map_or(false, |game| {
                // log_info(&format!(
                //     "[GHOST] Processing current game [{}], inited: {}, server: {:?}",
                //     game.get_game_name(),
                //     game.m_inited,
                //     game.m_creator_server
                // ));
                !game.m_inited
            })
        };
        
        if need_init {
            {
                let mut current_game = CURRENT_GAME.write().await;
                if let Some(game) = current_game.as_mut() {
                    log_info(&format!("[GHOST] Initializing game [{}]", game.get_game_name()));
                    game.init().await;
                    log_info(&format!("[GHOST] Game [{}] initialized", game.get_game_name()));
                }
            }
        }
        
        let should_delete = {
            let mut current_game = CURRENT_GAME.write().await;
            if let Some(game) = current_game.as_mut() {
                if !game.m_inited {
                    log_info(&format!("[GHOST] Initializing game [{}]", game.get_game_name()));
                    game.init().await;
                    log_info(&format!("[GHOST] Game [{}] initialized", game.get_game_name()));
                }
        
                timeout(Duration::from_millis(150), game.update())
                    .await
                    .unwrap_or(true)
            } else {
                false
            }
        };
        
        
        self.m_Exiting
    }

    pub async fn extract_scripts(&mut self) {
        let war3path = config::get_string("bot_war3path", "default");
        let mut extract_casc = false;
        let mut patch_mpq_file =  war3path.clone() + "War3x.mpq";

        if !file_exists(&patch_mpq_file) {
            patch_mpq_file = war3path.clone() + "War3Patch.mpq";
        }
        if !file_exists(&patch_mpq_file) {
            extract_casc = true;
            patch_mpq_file = war3path.clone() + "/Data";
        }

        if !file_exists(&patch_mpq_file) {
            log_warning("[GHOST] warning - mpq file and exe file not found");

        } else {
            if extract_casc {
                todo!();
            } else {
                self.extract_scripts_pre_130(patch_mpq_file).await;
            }
        }
    }

    pub async fn extract_scripts_pre_130(&mut self, patch_mpq_file_name: String) {
        let map_path = config::get_string("bot_mappath", "maps");
        let mut war3 = Archive::open(patch_mpq_file_name.clone()).unwrap();

        
        log_info(&format!("[GHOST] loading MPQ file [{}]", patch_mpq_file_name));

        let mut commonj = war3.open_file("Scripts\\common.j").unwrap();
        let mut file_length = commonj.size();

        if file_length > 0 && file_length != 0xFFFFFFFF{
            let mut buf: Vec<u8> = vec![0; commonj.size() as usize];
            
            commonj.read(&mut war3, &mut buf).unwrap();
            
            if buf.len() > 0 {
                log_info("[GHOST] extracting Scripts\\common.j from MPQ file");
                let _ = file_write(&(map_path.to_owned() + "/common.j"), &buf);
            }
            else {
                log_warning("[GHOST] warning - unable to extract Scripts\\common.j from MPQ file");
            }
        }
        
        else {
            log_warning("[GHOST] couldn't find Scripts\\common.j in MPQ file");
        }


        let mut blizzardj = war3.open_file("Scripts\\blizzard.j").unwrap();
        let mut file_length = commonj.size();

        if file_length > 0 && file_length != 0xFFFFFFFF{
            let mut buf: Vec<u8> = vec![0; blizzardj.size() as usize];
            
            blizzardj.read(&mut war3, &mut buf).unwrap();
            
            if buf.len() > 0 {
                log_info("[GHOST] extracting Scripts\\blizzard.j from MPQ file");
                let _ = file_write(&(map_path.to_owned() + "/blizzard.j"), &buf);
            }
            else {
                log_warning("[GHOST] warning - unable to extract Scripts\\blizzard.j from MPQ file");
            }
        }
        else {
            log_warning("[GHOST] couldn't find Scripts\\blizzard.j in MPQ file");
        }
    }

    pub async fn create_game(&mut self, map: &mut Map, game_state: u8, save_game: bool, game_name: String, owner_name: String,
        creator_name: String, creator_server: String, whisper: bool
    ) {
        if !self.m_Enabled {
            for i in &mut self.m_BNETs {
                if i.get_server() == creator_server {
                    i.queue_chat_command2("Unable to create game. The bot is disabled".to_owned(), creator_name.clone(), whisper).await;
                }
            }
            return;
        }

        if game_name.len() > 31 {
            for i in &mut self.m_BNETs {
                if i.get_server() == creator_server {
                    i.queue_chat_command2("Unable to create game. Game name is too long (> 32).".to_owned(), creator_name.clone(), whisper).await;
                }
            }
            return;
        }
        if !map.get_valid() {
            for i in &mut self.m_BNETs {
                if i.get_server() == creator_server {
                    i.queue_chat_command2("Unable to create game. Invalid map.".to_owned(), creator_name.clone(), whisper).await;
                }
            }
        }

        log_info(&format!("[GHOST] creating game [{}]", game_name));



        println!("{:?}", self.m_BNETs);
        
        let host_counter = self.m_HostCounter;
        for i in &mut self.m_BNETs {
            let game_name = game_name.clone();
            if whisper && i.get_server() == creator_server {
                if game_state == GAME_PRIVATE { i.queue_chat_command2(format!("Creating private game [{}] by user [{}]", game_name, owner_name), creator_name.clone(), true).await; }
                else if game_state == GAME_PUBLIC {if game_state == GAME_PRIVATE { i.queue_chat_command2(format!("Creating public game [{}] by user [{}]", game_name, owner_name), creator_name.clone(), true).await; }}
            } else {
                if game_state == GAME_PRIVATE { i.queue_chat_command(format!("Creating private game [{}] by user [{}]", game_name.clone(), owner_name)).await; }
                else if game_state == GAME_PUBLIC { i.queue_chat_command(format!("Creating public game [{}] by user [{}]", game_name.clone(), owner_name)).await; }
            }

            i.queue_game_create(game_state, game_name, owner_name.clone(), map, host_counter).await;
        }
    }
}