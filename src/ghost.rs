use crate::game;
use crate::logger::{log_info, log_warning};
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
use tokio::sync::{RwLock, Mutex as AsyncMutex};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};
use once_cell::sync::Lazy;

static START: Lazy<Instant> = Lazy::new(Instant::now);

pub static m_BNETs: Lazy<RwLock<Vec<Arc<AsyncMutex<BNET>>>>> =
    Lazy::new(|| RwLock::new(Vec::new()));

pub static m_GAMES: Lazy<RwLock<Vec<Arc<AsyncMutex<BaseGame>>>>> =
    Lazy::new(|| RwLock::new(Vec::new()));

pub static CURRENT_GAME: Lazy<RwLock<Option<Arc<AsyncMutex<BaseGame>>>>> = 
    Lazy::new(|| RwLock::new(None));

pub static HOST_COUNTER: AtomicU32 = AtomicU32::new(2);

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
    pub m_TFT: bool,
    pub m_Exiting: bool,
    pub m_ExitingNicely: bool,
    pub m_Enabled: bool,
    pub m_Version: String,
    pub m_HostCounter: u32,
    pub m_MaxGames: u32,
    pub m_ReconnectWaitTime: u32,
    pub m_BindAddress: String,
    pub m_Games: Vec<String>,
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
            m_TFT: self.m_TFT,
            m_Exiting: self.m_Exiting,
            m_ExitingNicely: self.m_ExitingNicely,
            m_Enabled: self.m_Enabled,
            m_Version: self.m_Version.clone(),
            m_HostCounter: self.m_HostCounter,
            m_MaxGames: self.m_MaxGames,
            m_ReconnectWaitTime: self.m_ReconnectWaitTime,
            m_BindAddress: self.m_BindAddress.clone(),
            m_Games: self.m_Games.clone(),
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
            m_TFT: false,
            m_Exiting: false,
            m_ExitingNicely: false,
            m_Enabled: false,
            m_Version: String::new(),
            m_HostCounter: 1,
            m_MaxGames: 0,
            m_ReconnectWaitTime: 0,
            m_BindAddress: "0.0.0.0".to_owned(),
            m_Games: Vec::new(),
        }
    }

    pub fn add_game(&mut self, _game: BaseGame) {
        self.m_Games.push(_game.get_game_name());
    }

    pub fn set_current_game(&mut self, _game: Option<BaseGame>) {
        let self_ptr = self as *mut Ghost;
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
        let admin_string = config::get_string("bnet_rootadmin", "slash -_-");
        let mut admins: Vec<String> = Vec::new();
        for i in admin_string.split(" ") {
            admins.push(i.to_owned());
        }

        log_info("[GHOST] Creating BNET instance");
        let ghost_clone = (*self).clone();
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
            admins
        );
        m_BNETs.write().await.push(Arc::new(AsyncMutex::new(bnet.await)));
        log_info("[GHOST] BNET instance added");

        log_info("[GHOST] Extracting scripts");
        self.extract_scripts().await;
        log_info("[GHOST] Initialization completed");
        println!("BNET on start: {:?}", std::ptr::addr_of!(m_BNETs.read().await[0]) );
    }

    pub async fn update(&mut self) -> bool {
        let bnets = m_BNETs.read().await;
        for bnet_arc in bnets.iter() {
            let mut bnet = bnet_arc.lock().await;
            bnet.update().await;
        }
    
        let game_arcs;
        {
            let games = m_GAMES.read().await;
            game_arcs = games.iter().map(Arc::clone).collect::<Vec<_>>();
        } 

        for game_arc in game_arcs {
            let mut game = game_arc.lock().await;
            //println!("{:?}", game.m_socket);
            game.update().await;
        }


    
        let need_init = {
            let current = CURRENT_GAME.read().await;
            current.as_ref().map_or(false, |g| {
                !tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        let game = g.lock().await;
                        game.m_inited
                    })
                })
            })
        };
    
        if need_init {
            let game_arc = CURRENT_GAME.read().await.clone();
            if let Some(g) = game_arc {
                let mut game = g.lock().await;
                log_info(&format!("[GHOST] Initializing game [{}]", game.get_game_name()));
                game.init().await;
                log_info(&format!("[GHOST] Game [{}] initialized", game.get_game_name()));
            }
        }
    
        let game_option = {
            let current_game_lock = CURRENT_GAME.write().await;
            current_game_lock.as_ref().map(Arc::clone)
            // lock отпускается тут
        };
        
        let should_delete = if let Some(game_arc) = game_option {
            let mut game = game_arc.lock().await;
            timeout(Duration::from_millis(500), game.update())
                .await
                .unwrap_or(false)
        } else {
            false
        };
        
    
        if should_delete {
            let mut current_game = CURRENT_GAME.write().await;
            if current_game.is_some() {
                log_info("[GHOST] deleting current game");
                *current_game = None;

                let bnets = m_BNETs.read().await;
                for bnet_arc in bnets.iter() {
                    let mut bnet = bnet_arc.lock().await;
                    bnet.queue_game_uncreate().await;
                    bnet.queue_enter_chat().await;
                }
            }
        }
    
        self.m_Exiting
    }
    

    pub async fn extract_scripts(&mut self) {
        let war3path = config::get_string("bot_war3path", "default");
        let mut extract_casc = false;
        let mut patch_mpq_file = war3path.clone()  + "War3x.mpq";

        if !file_exists(&patch_mpq_file) {
            patch_mpq_file = war3path.clone() + "War3Patch.mpq";
        }
        if !file_exists(&patch_mpq_file) {
            extract_casc = true;
            patch_mpq_file = war3path + "/Data";
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
        let map_path = config::get_string("bot_mappath", "");
        let mut war3 = Archive::open(&patch_mpq_file_name).unwrap();

        log_info(&format!("[GHOST] loading MPQ file [{}]", patch_mpq_file_name));

        if let Ok(mut commonj) = war3.open_file("Scripts\\common.j") {
            let file_length = commonj.size();
            if file_length > 0 && file_length != 0xFFFFFFFF {
                let mut buf = vec![0; file_length as usize];
                if commonj.read(&mut war3, &mut buf).is_ok() && !buf.is_empty() {
                    log_info("[GHOST] extracting Scripts\\common.j from MPQ file");
                    let _ = file_write(&(map_path.to_owned() + "/common.j"), &buf);
                } else {
                    log_warning("[GHOST] warning - unable to extract Scripts\\common.j from MPQ file");
                }
            } else {
                log_warning("[GHOST] couldn't find Scripts\\common.j in MPQ file");
            }
        }

        if let Ok(mut blizzardj) = war3.open_file("Scripts\\blizzard.j") {
            let file_length = blizzardj.size();
            if file_length > 0 && file_length != 0xFFFFFFFF {
                let mut buf = vec![0; file_length as usize];
                if blizzardj.read(&mut war3, &mut buf).is_ok() && !buf.is_empty() {
                    log_info("[GHOST] extracting Scripts\\blizzard.j from MPQ file");
                    let _ = file_write(&(map_path.to_owned() + "/blizzard.j"), &buf);
                } else {
                    log_warning("[GHOST] warning - unable to extract Scripts\\blizzard.j from MPQ file");
                }
            } else {
                log_warning("[GHOST] couldn't find Scripts\\blizzard.j in MPQ file");
            }
        }
    }
}