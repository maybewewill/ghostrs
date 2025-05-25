use std::io::Cursor;
use std::io::Seek;
use std::io::SeekFrom;
use std::sync::{Arc, Mutex};
use crate::ghost::*;
use crate::util::*;
use crate::logger::*;
use crate::crc32::*;
use crate::sha1::*;
use crate::config;
use crate::gameslot::*;
use byteorder::LittleEndian;
use byteorder::ReadBytesExt;
use mpq::{Archive, File};



fn rotl32(x: u32, n: u32) -> u32 {
    x.rotate_left(n)
}

fn rotr32(x: u32, n: u32) -> u32 {
    x.rotate_right(n)
}

fn read_cstring(cursor: &mut Cursor<&[u8]>) -> std::io::Result<String> {
    let mut buf = Vec::new();
    loop {
        let byte = cursor.read_u8()?;
        if byte == 0 {
            break;
        }
        buf.push(byte);
        if buf.len() > 512 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "CString too long"));
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}


pub const MAPSPEED_SLOW: u8 = 1;
pub const MAPSPEED_NORMAL: u8 = 2;
pub const MAPSPEED_FAST: u8 = 3;

pub const MAPVIS_HIDETERRAIN: u8 = 1;
pub const MAPVIS_EXPLORED: u8 = 2;
pub const MAPVIS_ALWAYSVISIBLE: u8 = 3;
pub const MAPVIS_DEFAULT: u8 = 4;

pub const MAPOBS_NONE: u8 = 1;
pub const MAPOBS_ONDEFEAT: u8 = 2;
pub const MAPOBS_ALLOWED: u8 = 3;
pub const MAPOBS_REFEREES: u8 = 4;

pub const MAPFLAG_TEAMSTOGETHER: u8 = 1;
pub const MAPFLAG_FIXEDTEAMS: u8 = 2;
pub const MAPFLAG_UNITSHARE: u8 = 4;
pub const MAPFLAG_RANDOMHERO: u8 = 8;
pub const MAPFLAG_RANDOMRACES: u8 = 16;

pub const MAPOPT_HIDEMINIMAP: u32 = 1 << 0;
pub const MAPOPT_MODIFYALLYPRIORITIES: u32 = 1 << 1;
pub const MAPOPT_MELEE: u32 = 1 << 2;
pub const MAPOPT_REVEALTERRAIN: u32 = 1 << 4;
pub const MAPOPT_FIXEDPLAYERSETTINGS: u32 = 1 << 5;
pub const MAPOPT_CUSTOMFORCES: u32 = 1 << 6;
pub const MAPOPT_CUSTOMTECHTREE: u32 = 1 << 7;
pub const MAPOPT_CUSTOMABILITIES: u32 = 1 << 8;
pub const MAPOPT_CUSTOMUPGRADES: u32 = 1 << 9;
pub const MAPOPT_WATERWAVESONCLIFFSHORES: u32 = 1 << 11;
pub const MAPOPT_WATERWAVESONSLOPESHORES: u32 = 1 << 12;

pub const MAPFILTER_MAKER_USER: u8 = 1;
pub const MAPFILTER_MAKER_BLIZZARD: u8 = 2;

pub const MAPFILTER_TYPE_MELEE: u8 = 1;
pub const MAPFILTER_TYPE_SCENARIO: u8 = 2;

pub const MAPFILTER_SIZE_SMALL: u8 = 1;
pub const MAPFILTER_SIZE_MEDIUM: u8 = 2;
pub const MAPFILTER_SIZE_LARGE: u8 = 4;

pub const MAPFILTER_OBS_FULL: u8 = 1;
pub const MAPFILTER_OBS_ONDEATH: u8 = 2;
pub const MAPFILTER_OBS_NONE: u8 = 4;

pub const MAPGAMETYPE_UNKNOWN0: u32 = 1;
pub const MAPGAMETYPE_SAVEDGAME: u32 = 1 << 9;
pub const MAPGAMETYPE_PRIVATEGAME: u32 = 1 << 11;
pub const MAPGAMETYPE_MAKERUSER: u32 = 1 << 13;
pub const MAPGAMETYPE_MAKERBLIZZARD: u32 = 1 << 14;
pub const MAPGAMETYPE_TYPEMELEE: u32 = 1 << 15;
pub const MAPGAMETYPE_TYPESCENARIO: u32 = 1 << 16;
pub const MAPGAMETYPE_SIZESMALL: u32 = 1 << 17;
pub const MAPGAMETYPE_SIZEMEDIUM: u32 = 1 << 18;
pub const MAPGAMETYPE_SIZELARGE: u32 = 1 << 19;
pub const MAPGAMETYPE_OBSFULL: u32 = 1 << 20;
pub const MAPGAMETYPE_OBSONDEATH: u32 = 1 << 21;
pub const MAPGAMETYPE_OBSNONE: u32 = 1 << 22;

fn read_null_terminated_string(cursor: &mut Cursor<&Vec<u8>>) -> std::io::Result<String> {
    let mut buf = Vec::new();
    loop {
        let byte = cursor.read_u8()?;
        if byte == 0 { break; }
        buf.push(byte);
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[derive(Debug)]
#[derive(Clone)]
pub struct Map {
    pub ghost: Arc<Mutex<Ghost>>,
    pub valid: bool,
    pub cfg_file: String,
    pub map_path: String,
    pub map_size: Vec<u8>,
    pub map_info: Vec<u8>,
    pub map_crc: Vec<u8>,
    pub map_sha1: Vec<u8>,
    pub map_speed: u8,
    pub map_visibility: u8,
    pub map_observers: u8,
    pub map_flags: u8,
    pub map_filter_maker: u8,
    pub map_filter_type: u8,
    pub map_filter_size: u8,
    pub map_filter_obs: u8,
    pub map_options: u32,
    pub map_width: Vec<u8>,
    pub map_height: Vec<u8>,
    pub map_type: String,
    pub map_matchmaking_category: String,
    pub map_stats_w3mmd_category: String,
    pub map_default_hcl: String,
    pub map_default_player_score: u32,
    pub map_local_path: String,
    pub map_load_in_game: bool,
    pub map_data: Vec<u8>,
    pub map_num_players: u32,
    pub map_num_teams: u32,
    pub slots: Vec<GameSlot>,
}

impl Map {
    #[allow(clippy::too_many_arguments)]
    pub fn new(_ghost: Ghost, map_path: String) -> Self {
        Map {
            ghost: Arc::new(Mutex::new(_ghost)),
            valid: false,
            cfg_file: String::new(),
            map_path: String::new(),
            map_size: Vec::new(),
            map_info: Vec::new(),
            map_crc: Vec::new(),
            map_sha1: Vec::new(),
            map_speed: 0,
            map_visibility: 0,
            map_observers: 0,
            map_flags: 0,
            map_filter_maker: 0,
            map_filter_type: 0,
            map_filter_size: 0,
            map_filter_obs: 0,
            map_options: 0,
            map_width: Vec::new(),
            map_height: Vec::new(),
            map_type: String::new(),
            map_matchmaking_category: String::new(),
            map_stats_w3mmd_category: String::new(),
            map_default_hcl: String::new(),
            map_default_player_score: 0,
            map_local_path: String::new(),
            map_load_in_game: false,
            map_data: Vec::new(),
            map_num_players: 0,
            map_num_teams: 0,
            slots: Vec::new(),
        }
    }

    pub fn get_map_game_flags(&self) -> Vec<u8> {
        let mut game_flags: u32 = 0;

        if self.map_speed == MAPSPEED_SLOW { game_flags = 0x00000000; }
        else if self.map_speed == MAPSPEED_NORMAL { game_flags = 0x00000001; }
        else { game_flags = 0x00000002; }


        if self.map_visibility == MAPVIS_HIDETERRAIN { game_flags |= 0x00000100; }
        else if self.map_visibility == MAPVIS_EXPLORED { game_flags |= 0x00000200; }
        else if self.map_visibility == MAPVIS_ALWAYSVISIBLE { game_flags |= 0x00000400; }
        else { game_flags |= 0x00000800; }


        if self.map_observers == MAPOBS_ONDEFEAT { game_flags |= 0x00002000; }
        else if self.map_observers == MAPOBS_ALLOWED { game_flags |= 0x00003000; }
        else if self.map_observers == MAPOBS_REFEREES { game_flags |= 0x40000000; }


        if (self.map_flags & MAPFLAG_TEAMSTOGETHER) != 0 {
            game_flags |= 0x00004000;
        } if (self.map_flags & MAPFLAG_FIXEDTEAMS) != 0 {
            game_flags |= 0x00060000;
        } if (self.map_flags & MAPFLAG_UNITSHARE) != 0 {
            game_flags |= 0x01000000;
        } if (self.map_flags & MAPFLAG_RANDOMHERO) != 0 {
            game_flags |= 0x02000000;
        } if (self.map_flags & MAPFLAG_TEAMSTOGETHER) != 0 {
            game_flags |= 0x04000000;
        }

        return create_byte_array_from_u32(game_flags, false);
    }

    pub fn get_map_game_type(&self) -> u32 {
        let mut game_type: u32 = 0;
    
        // Maker
        if (self.map_filter_maker & MAPFILTER_MAKER_USER) != 0 {
            game_type |= MAPGAMETYPE_MAKERUSER;
        }
        if (self.map_filter_maker & MAPFILTER_MAKER_BLIZZARD) != 0 {
            game_type |= MAPGAMETYPE_MAKERBLIZZARD;
        }
    
        // Type
        if (self.map_filter_type & MAPFILTER_TYPE_MELEE) != 0 {
            game_type |= MAPGAMETYPE_TYPEMELEE;
        }
        if (self.map_filter_type & MAPFILTER_TYPE_SCENARIO) != 0 {
            game_type |= MAPGAMETYPE_TYPESCENARIO;
        }
    
        // Size
        if (self.map_filter_size & MAPFILTER_SIZE_SMALL) != 0 {
            game_type |= MAPGAMETYPE_SIZESMALL;
        }
        if (self.map_filter_size & MAPFILTER_SIZE_MEDIUM) != 0 {
            game_type |= MAPGAMETYPE_SIZEMEDIUM;
        }
        if (self.map_filter_size & MAPFILTER_SIZE_LARGE) != 0 {
            game_type |= MAPGAMETYPE_SIZELARGE;
        }
    
        // Observers
        if (self.map_filter_obs & MAPFILTER_OBS_FULL) != 0 {
            game_type |= MAPGAMETYPE_OBSFULL;
        }
        if (self.map_filter_obs & MAPFILTER_OBS_ONDEATH) != 0 {
            game_type |= MAPGAMETYPE_OBSONDEATH;
        }
        if (self.map_filter_obs & MAPFILTER_OBS_NONE) != 0 {
            game_type |= MAPGAMETYPE_OBSNONE;
        }
    
        game_type
    }

    pub fn get_map_layout_style(&mut self) -> u8 {
        if (self.map_options & MAPOPT_CUSTOMFORCES) == 0 {
            return 0;
        }
        
        if (self.map_options & MAPOPT_FIXEDPLAYERSETTINGS) == 0 {
            return 1;
        }
        return 3;
    }
    
    pub fn get_map_num_players(&mut self) -> u8 {
        self.map_num_players as u8
    }
    pub fn get_map_flags(&mut self) -> u8 {
        self.map_flags
    }

    pub fn get_map_size(&mut self) -> Vec<u8> {
        self.map_size.clone()
    }

    pub fn get_map_info(&mut self) -> Vec<u8> {
        self.map_info.clone()
    }

    pub async fn load(&mut self, local_path: String) -> Result<(), std::io::Error> {
        self.valid = true;
        self.cfg_file = String::new();


        self.map_local_path = config::get_string("map_localpath", "");
        self.map_local_path = local_path.clone();
        self.map_data.clear();
        
        if !self.map_local_path.is_empty() {
            self.map_data = file_read_full_bytes(&format!("maps/{}", self.map_local_path)).unwrap_or_default();
        }
            let map_mpq_file_name = format!("maps/{}", self.map_local_path);
            
            let mut map_mpq_ready = false;

            let mut map: Archive = Archive::open(map_mpq_file_name.clone()).unwrap();

            log_info(&format!("[MAP] loading MPQ file [{}]", map_mpq_file_name));
            map_mpq_ready = true;

            let mut map_size: Vec<u8> = Vec::new();
            let mut map_info: Vec<u8> = Vec::new();
            let mut map_crc: Vec<u8> = Vec::new();
            let mut map_sha1: Vec<u8> = Vec::new();

            if !self.map_data.is_empty() {
                if let Ok(mut ghost) = self.ghost.lock() {
                    ghost.m_SHA.reset();
                } else {
                    log_warning("[MAP] Failed to lock ghost for m_SHA reset");
                }
                
                map_size = create_byte_array_from_u32(self.map_data.len() as u32, false);
                log_info(&format!("[MAP] calculated map_size = {}", byte_array_to_dec_string(&map_size)));

                if let Ok(ghost) = self.ghost.lock() {
                    map_info = create_byte_array_from_u32(ghost.m_CRC.full_crc(&self.map_data, self.map_data.len().try_into().unwrap()), false);
                } else {
                    log_warning("[MAP] Failed to lock ghost for CRC calculation");
                    map_info = Vec::new();
                }
                log_info(&format!("[MAP] calculated map_info = {}", byte_array_to_dec_string(&map_info)));
            
                
                let mut commonj = file_read_full("maps/common.j").unwrap();
                if commonj.is_empty() {
                    log_info(&format!("[MAP] unable to calculate map_crc/sha1 - unable to read file [maps/common.j]"));

                }  else {
                    let mut blizzardj = file_read_full("maps/blizzard.j").unwrap();
                    
                    if blizzardj.is_empty() {
                        log_info("[MAP] unable to calculate map_crc/sha1 - unable to read file [maps/blizzard.j]");
                    } else {
                        let mut val: u32 = 0;

                        let mut overrode_commonj: bool = false;
                        let mut overrode_blizzardj: bool = false;

                        if map_mpq_ready {
                            if let Ok(mut cmnj) = map.open_file("Scripts\\common.j") {
                                let mut file_length = cmnj.size();

                                if file_length > 0 && file_length != 0xFFFFFFFF {
                                    let mut buf: Vec<u8> = vec![0; cmnj.size() as usize];
                
                                    let bytes_read = cmnj.read(&mut map, &mut buf).unwrap();
                                    
                                    if buf.len() > 0 {
                                        log_warning("[MAP] overriding default common.j with map copy while calculating map_crc/sha1");
                                        overrode_commonj = true;
                                        let val = val ^ self.xor_rotate_left(&buf[..bytes_read]);
                                        if let Ok(mut ghost) = self.ghost.lock() {
                                            ghost.m_SHA.update(&buf[..bytes_read]);
                                        } else {
                                            log_warning("[MAP] Failed to lock ghost for SHA update");
                                        }
                                    }
                                }
                            } 
                        }

                        if !overrode_commonj {
                            val = val ^ self.xor_rotate_left(commonj.as_bytes());
                            if let Ok(mut ghost) = self.ghost.lock() {
                                  ghost.m_SHA.update(commonj.as_bytes());
                            } else {

                            }
                        }

                        if map_mpq_ready {
                            if let Ok(mut blrj) = map.open_file("Scripts\\blizzard.j") {
                                let mut file_length = blrj.size();

                                if file_length > 0 && file_length != 0xFFFFFFFF {
                                    let mut buf: Vec<u8> = vec![0; blrj.size() as usize];
                
                                    let bytes_read = blrj.read(&mut map, &mut buf).unwrap();
                                    
                                    if buf.len() > 0 {
                                        log_warning("[MAP] overriding default blizzard.j with map copy while calculating map_crc/sha1");
                                        overrode_commonj = true;
                                        let val = val ^ self.xor_rotate_left(&buf[..bytes_read]);
                                        if let Ok(mut ghost) = self.ghost.lock() {
                                        ghost.m_SHA.update(&buf[..bytes_read]);}
                                    }
                                }
                            }
                        }

                        if !overrode_blizzardj {
                            val = val ^ self.xor_rotate_left(blizzardj.as_bytes());
                            if let Ok(mut ghost) = self.ghost.lock() {ghost.m_SHA.update(blizzardj.as_bytes());}
                        }

                        val = rotl32(val, 3);
                        val = rotl32(val ^ 0x03F1379E, 3);
                        if let Ok(mut ghost) = self.ghost.lock() {ghost.m_SHA.update(&[0x9E, 0x37, 0xF1, 0x03]);}
                        
                        if map_mpq_ready {
                            let mut file_list = Vec::new();
                            file_list.push("war3map.j");
                            file_list.push("scripts\\war3map.j");
                            file_list.push("war3map.w3e");
                            file_list.push("war3map.wpm");
                            file_list.push("war3map.doo");
                            file_list.push("war3map.w3u");
                            file_list.push("war3map.w3b");
                            file_list.push("war3map.w3d");
                            file_list.push("war3map.w3a");
                            file_list.push("war3map.w3q");
                            let mut found_script = false;

                            for i in file_list {
                                if found_script && i == "scripts\\war3map.j" {
                                    continue;
                                }

                                if let Ok(mut file) = map.open_file(i) {
                                let mut length = file.size();

                                if length > 0 && length != 0xFFFFFFFF {
                                    let mut buf: Vec<u8> = vec![0; file.size() as usize];
            
                                    let bytes_read = file.read(&mut map, &mut buf).unwrap();

                                    if bytes_read > 0 {
                                        if i == "war3map.j" || i == "scripts\\war3map.j" {
                                            found_script = true;
                                        }

                                        val = (val ^ self.xor_rotate_left(&buf[..bytes_read])).rotate_left(3);
                                        if let Ok(mut ghost) = self.ghost.lock() {ghost.m_SHA.update(&buf[..bytes_read]);}
                                    }
                                }
                            }
                        }

                            if !found_script {log_info("[MAP] couldn't find war3map.j or scripts\\war3map.j in MPQ file, calculated map_crc/sha1 is probably wrong")}

                            map_crc = create_byte_array_from_u32(val, false);
                            log_info(&format!("[MAP] calculated map_crc = {}", byte_array_to_dec_string(&map_crc)));

                            if let Ok(mut ghost) = self.ghost.lock() {ghost.m_SHA.finalise();}
                            if let Ok(mut ghost) = self.ghost.lock() { 
                            let sha1 = ghost.m_SHA.get_hash().unwrap_or_default();

                            map_sha1 = create_byte_array(&sha1);
                            log_info(&format!("[MAP] calculated map_sha1 = {}", byte_array_to_dec_string(&map_sha1)));
                            }
                                                      
                        }
                    }
                }
            }
        else {
            log_info( "[MAP] no map data available, using config file for map_size, map_info, map_crc, map_sha1" );
        } 
        let mut map_options: u32 = 0;
        let mut map_width = Vec::new();
        let mut map_height = Vec::new();
        let mut map_num_players: u32 = 0;
        let mut map_num_teams: u32 = 0;
        let mut map_filter_type: u8 = MAPFILTER_TYPE_SCENARIO;
        let mut slots = Vec::new();
        let mut editor_version: u32 = 0;

        if !self.map_data.is_empty() && map_mpq_ready {
            if let Ok(mut w3i_file) = map.open_file("war3map.w3i") {
                let file_length = w3i_file.size();
                if file_length > 0 && file_length != 0xFFFFFFFF {
                    let mut buf = vec![0; file_length as usize];
                    if w3i_file.read(&mut map, &mut buf).is_ok() {
                        let mut iss = Cursor::new(&buf[..]);
                        let file_format = iss.read_u32::<LittleEndian>().unwrap_or(0);
                        if file_format == 18 || file_format == 25 {
                            iss.seek(SeekFrom::Current(4))?;
                            editor_version = iss.read_u32::<LittleEndian>().unwrap_or(0);
                            for _ in 0..4 { read_cstring(&mut iss)?; }
                            iss.seek(SeekFrom::Current(32 + 16))?;
                            let raw_map_width = iss.read_u32::<LittleEndian>().unwrap_or(0);
                            let raw_map_height = iss.read_u32::<LittleEndian>().unwrap_or(0);
                            let raw_map_flags = iss.read_u32::<LittleEndian>().unwrap_or(0);
                            iss.seek(SeekFrom::Current(1))?;
                            if file_format == 18 {
                                iss.seek(SeekFrom::Current(4))?;
                            } else {
                                iss.seek(SeekFrom::Current(4))?;
                                read_cstring(&mut iss)?;
                            }
                            for _ in 0..3 { read_cstring(&mut iss)?; }
                            if file_format == 18 {
                                iss.seek(SeekFrom::Current(4))?;
                            } else {
                                iss.seek(SeekFrom::Current(4))?;
                                read_cstring(&mut iss)?;
                            }
                            for _ in 0..3 { read_cstring(&mut iss)?; }
                            if file_format == 25 {
                                iss.seek(SeekFrom::Current(24))?;
                                read_cstring(&mut iss)?;
                                iss.seek(SeekFrom::Current(5))?;
                            }
                            let raw_map_num_players = iss.read_u32::<LittleEndian>().unwrap_or(0);
                            
                            let mut closed_slots = 0;
                            let mut slots = Vec::new();
                            for i in 0..raw_map_num_players {
                                let mut slot = GameSlot::new(0, 255, SLOTSTATUS_OPEN, 0, 0, 0, SLOTRACE_RANDOM, SLOTCOMP_NORMAL, 100);
                                let colour = iss.read_u32::<LittleEndian>().unwrap_or(0);
                                slot.set_colour(colour as u8);
                                let status = iss.read_u32::<LittleEndian>().unwrap_or(0);
                                match status {
                                    1 => slot.set_slot_status(SLOTSTATUS_OPEN),
                                    2 => {
                                        slot.set_slot_status(SLOTSTATUS_OCCUPIED);
                                        slot.set_computer(1);
                                        slot.set_computer_type(SLOTCOMP_NORMAL);
                                    }
                                    _ => {
                                        slot.set_slot_status(SLOTSTATUS_CLOSED);
                                        closed_slots += 1;
                                    }
                                }
                                let race = iss.read_u32::<LittleEndian>().unwrap_or(0);
                                match race {
                                    1 => slot.set_race(SLOTRACE_HUMAN),
                                    2 => slot.set_race(SLOTRACE_ORC),
                                    3 => slot.set_race(SLOTRACE_UNDEAD),
                                    4 => slot.set_race(SLOTRACE_NIGHTELF),
                                    _ => slot.set_race(SLOTRACE_RANDOM),
                                }
                                iss.seek(SeekFrom::Current(4))?;
                                read_cstring(&mut iss)?;
                                iss.seek(SeekFrom::Current(16))?;
                                if slot.slot_status() != SLOTSTATUS_CLOSED {
                                    slots.push(slot);
                                }
                            }
                            let raw_map_num_teams = iss.read_u32::<LittleEndian>().unwrap_or(0);
                            for team in 0..raw_map_num_teams {
                                iss.read_u32::<LittleEndian>().unwrap_or(0);
                                let player_mask = iss.read_u32::<LittleEndian>().unwrap_or(0);
                                let mut mask = player_mask;
                                for j in 0..24 {
                                    if mask & 1 != 0 {
                                        for slot in &mut slots {
                                            if slot.colour() == j as u8 {
                                                slot.set_team(team as u8);
                                            }
                                        }
                                    }
                                    mask >>= 1;
                                }
                                read_cstring(&mut iss)?;
                            }
                            map_options = raw_map_flags & (MAPOPT_MELEE | MAPOPT_FIXEDPLAYERSETTINGS | MAPOPT_CUSTOMFORCES);
                            log_info(&format!("[MAP] calculated map_options = {}", map_options));
                            map_width = create_byte_array_from_u16(raw_map_width as u16, false);
                            log_info(&format!("[MAP] calculated map_width = {}", byte_array_to_dec_string(&map_width)));
                            map_height = create_byte_array_from_u16(raw_map_height as u16, false);
                            log_info(&format!("[MAP] calculated map_height = {}", byte_array_to_dec_string(&map_height)));
                            map_num_players = raw_map_num_players - closed_slots;
                            log_info(&format!("[MAP] calculated map_numplayers = {}", map_num_players));
                            map_num_teams = raw_map_num_teams;
                            log_info(&format!("[MAP] calculated map_numteams = {}", map_num_teams));
                            for (i, slot) in slots.iter().enumerate() {
                                log_info(&format!(
                                    "[MAP] calculated map_slot{} = {}",
                                    i + 1,
                                    byte_array_to_dec_string(&slot.to_bytes())
                                ));
                            }
                            if self.map_options & MAPOPT_MELEE != 0 {
                                log_info("[MAP] found melee map, initializing slots");
                                let mut team = 0;
                                for slot in &mut slots {
                                    slot.set_team(team);
                                    slot.set_race(SLOTRACE_RANDOM);
                                    team += 1;
                                }
                                self.map_filter_type = MAPFILTER_TYPE_MELEE;
                            }
                            if self.map_options & MAPOPT_FIXEDPLAYERSETTINGS == 0 {
                                for slot in &mut slots {
                                    slot.set_race(slot.race() | SLOTRACE_SELECTABLE);
                                }
                            }
                            self.slots = slots;
                        } else {
                            log_info("[MAP] invalid war3map.w3i format");
                        }
                    } else {
                        log_info("[MAP] unable to read war3map.w3i");
                    }
                } else {
                    log_info("[MAP] invalid war3map.w3i file length");
                }
            } else {
                log_info("[MAP] couldn't find war3map.w3i in MPQ file");
            }
        } else {
            log_info("[MAP] no map data available, using config file");
        }

        // Load configuration values and override if necessary
        self.map_path = local_path;
        self.map_size = map_size;
        
        self.map_info = map_info;
        self.map_crc = map_crc;

        self.map_sha1 = map_sha1;

        self.map_speed = config::get_int("map_speed", i32::from(MAPSPEED_FAST)) as u8;
        self.map_visibility = config::get_int("map_visibility", i32::from(MAPVIS_DEFAULT)) as u8;
        self.map_observers = config::get_int("map_observers", i32::from(MAPOBS_NONE)) as u8;
        self.map_flags = config::get_int("map_flags", i32::from(MAPFLAG_TEAMSTOGETHER | MAPFLAG_FIXEDTEAMS)) as u8;
        self.map_filter_maker = config::get_int("map_filter_maker", i32::from(MAPFILTER_MAKER_USER)) as u8;
        self.map_filter_type = map_filter_type;

        self.map_filter_size = config::get_int("map_filter_size", i32::from(MAPFILTER_SIZE_LARGE)) as u8;
        self.map_filter_obs = config::get_int("map_filter_obs", i32::from(MAPFILTER_OBS_NONE)) as u8;

        self.map_options = map_options;

        self.map_width = map_width;

        self.map_height = map_height;

        self.map_type = config::get_string("map_type", "");
        self.map_matchmaking_category = config::get_string("map_matchmakingcategory", "");
        self.map_stats_w3mmd_category = config::get_string("map_statsw3mmdcategory", "");
        self.map_default_hcl = config::get_string("map_defaulthcl", "");
        self.map_default_player_score = config::get_int("map_defaultplayerscore", 1000) as u32;
        self.map_load_in_game = config::get_int("map_loadingame", 0) != 0;

        self.map_num_players = map_num_players;

        self.map_num_teams = map_num_teams;
        if slots.is_empty() {
            for slot in 1..=24 {
                let slot_key = format!("map_slot{}", slot);
                let slot_string = config::get_string(&slot_key, "");
                if slot_string.is_empty() {
                    break;
                }
                let slot_data = extract_numbers(&slot_string, 9);
                slots.push(GameSlot::new_from_byte_array(&slot_data));
            }
        } else if !config::get_string("map_slot1", "").is_empty() {
            log_info(&format!("[MAP] overriding slots"));
            slots.clear();
            for slot in 1..=24 {
                let slot_key = format!("map_slot{}", slot);
                let slot_string = config::get_string(&slot_key, "");
                if slot_string.is_empty() {
                    break;
                }
                let slot_data = extract_numbers(&slot_string, 9);
                slots.push(GameSlot::new_from_byte_array(&slot_data));
            }
        }

        if self.map_flags & MAPFLAG_RANDOMRACES != 0 {
            log_info("[MAP] forcing races to random");
            for slot in &mut self.slots {
                slot.set_race(SLOTRACE_RANDOM);
            }
        }

        if self.map_observers == MAPOBS_ALLOWED || self.map_observers == MAPOBS_REFEREES {
            let default_max_slots = if editor_version < 6060 { 12 } else { 12 }; // Adjust if MAX_SLOTS differs
            let max_slots = config::get_int("map_maxslots", default_max_slots) as u32;
            log_info(&format!("[MAP] adding {} observer slots", max_slots - self.slots.len() as u32));
            while self.slots.len() < max_slots as usize {
                self.slots.push(GameSlot::new(
                    0,
                    255,
                    SLOTSTATUS_OPEN,
                    0,
                    12, // Assuming MAX_SLOTS is 12
                    12,
                    SLOTRACE_RANDOM,
                    SLOTCOMP_NORMAL,
                    100
                ));
            }
        }

        self.check_valid();

        Ok(())
    }

    pub fn check_valid(&mut self) {
        if self.map_path.is_empty() || self.map_path.len() > 53 {
            self.valid = false;
            log_warning("[MAP] invalid map_path detected");
        }
        else if self.map_path.chars().nth(0) == Some('\\') {
            log_warning("[MAP] warning - map_path starts with '\\', any replays saved by GHostRS will not be playable in War3");
        }

        if self.map_path.find('/') != None {
            log_warning("[MAP] warning - map_path contains forward slashes '/' but it must use Windows style backslashes");
        }

        if self.map_size.len() != 4
        {
            self.valid = false;
            log_warning("[MAP] invalid map_size detected");
        }
        else if !self.map_data.is_empty() && self.map_data.len() != byte_array_to_u32(&self.map_size, false, 0) as usize {
            self.valid = false;
            log_warning("[MAP] invalid map_size detected - size mismatch with actual map data");
        }

        if self.map_info.len() != 4 {
            self.valid = false;
            log_warning("[MAP] invalid map_info detected");
        }

        if self.map_crc.len() != 4 {
            self.valid = false;
            log_warning("[MAP] invalid map_crc detected");
        }

        if self.map_sha1.len() != 20 {
            self.valid = false;
            log_warning("[MAP] invalid map_sha1 detected");
        }

        if self.map_speed != MAPSPEED_SLOW && self.map_speed != MAPSPEED_NORMAL && self.map_speed != MAPSPEED_FAST {
            self.valid = false;
            log_warning("[MAP] invalid map_speed detected");
        }

        if self.map_visibility != MAPVIS_HIDETERRAIN && self.map_visibility != MAPVIS_EXPLORED && self.map_visibility != MAPVIS_ALWAYSVISIBLE && self.map_visibility != MAPVIS_DEFAULT {
            self.valid = false;
            log_warning("[MAP] invalid map_visibility detected");
        }

        if self.map_observers != MAPOBS_NONE &&  self.map_observers != MAPOBS_ONDEFEAT &&  self.map_observers != MAPOBS_ALLOWED &&  self.map_observers != MAPOBS_REFEREES {
            self.valid = false;
            log_warning("[MAP] invalid map_observers detected");
        }


        if self.map_width.len() != 2 {
            self.valid = false;
            log_warning("[MAP] invalid map_width detected");
        }
        if self.map_height.len() != 2 {
            self.valid = false;
            log_warning("[MAP] invalid map_height detected");
        }

        if self.map_num_players == 0 || self.map_num_players > 24 {
            self.valid = false;
            log_warning("[MAP] invalid map_numplayers detected");
        }
        if self.map_num_teams == 0 || self.map_num_teams > 24 {
            self.valid = false;
            log_warning("[MAP] invalid map_num_teams detected");
        }
        if self.slots.is_empty() || self.slots.len() > 24 {
            self.valid = false;
            log_warning("[MAP] invalid map_slot<x> detected");
        }
    }

    pub fn xor_rotate_left(&mut self, data: &[u8]) -> u32 {
        fn rotl(value: u32, bits: u32) -> u32 {
            value.rotate_left(bits)
        }
    
        let mut i = 0;
        let mut val: u32 = 0;
    
        while i + 3 < data.len() {
            let chunk = u32::from(data[i])
                + (u32::from(data[i + 1]) << 8)
                + (u32::from(data[i + 2]) << 16)
                + (u32::from(data[i + 3]) << 24);
            val = rotl(val ^ chunk, 3);
            i += 4;
        }
    
        while i < data.len() {
            val = rotl(val ^ u32::from(data[i]), 3);
            i += 1;
        }
    
        val
    }
    pub fn get_valid(&mut self) -> bool {
        self.valid
    }

    pub fn get_map_height(&mut self) -> Vec<u8> {
        self.map_height.clone()
    }
    pub fn get_map_width(&mut self) -> Vec<u8> {
        self.map_width.clone()
    }

    pub fn get_map_path(&mut self) -> String {
        self.map_path.clone()
    }

    pub fn get_map_crc(&mut self) -> Vec<u8>{
        self.map_crc.clone()
    }

    pub fn get_map_sha1(&mut self) -> Vec<u8>{
        self.map_sha1.clone()
    }

    pub fn get_slots(&mut self) -> Vec<GameSlot> {
        self.slots.clone()
    }

    pub fn get_map_options(&self) -> u32 {
        self.map_options
    }
    
    pub fn get_map_observers(&self) -> u8 {
        self.map_observers
    }
    pub fn get_map_data(&self) -> Vec<u8> {
        self.map_data.clone()
    }
    
}