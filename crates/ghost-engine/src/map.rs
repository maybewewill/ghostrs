use std::fs;
use std::io::{self, Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use mpq::Archive;
use sha1::{Digest, Sha1};

use ghost_protocol::w3gs::SlotInfo;
use crate::slots::SlotStatus;
use crate::state::MapInfo;

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

pub const MAPFILTER_MAKER_USER: u8 = 1;
pub const MAPFILTER_MAKER_BLIZZARD: u8 = 2;
pub const MAPFILTER_TYPE_MELEE: u8 = 1;
pub const MAPFILTER_TYPE_SCENARIO: u8 = 2;
pub const MAPFILTER_SIZE_SMALL: u8 = 1;
pub const MAPFILTER_SIZE_MEDIUM: u8 = 2;
pub const MAPFILTER_SIZE_LARGE: u8 = 4;
pub const MAPFILTER_OBS_NONE: u8 = 4;

pub const MAPGAMETYPE_TYPEMELEE: u32 = 1 << 15;
pub const MAPGAMETYPE_TYPESCENARIO: u32 = 1 << 16;
pub const MAPGAMETYPE_SIZESMALL: u32 = 1 << 17;
pub const MAPGAMETYPE_SIZEMEDIUM: u32 = 1 << 18;
pub const MAPGAMETYPE_SIZELARGE: u32 = 1 << 19;
pub const MAPGAMETYPE_OBSNONE: u32 = 1 << 22;

/// Standard Warcraft III polynomial checksum calculation.
pub fn xor_rotate_left(data: &[u8]) -> u32 {
    let mut i = 0;
    let mut val: u32 = 0;
    while i + 3 < data.len() {
        let chunk = u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        val = (val ^ chunk).rotate_left(3);
        i += 4;
    }
    while i < data.len() {
        val = (val ^ u32::from(data[i])).rotate_left(3);
        i += 1;
    }
    val
}

fn read_cstring(cursor: &mut Cursor<&[u8]>) -> io::Result<String> {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    loop {
        if cursor.read_exact(&mut b).is_err() {
            break;
        }
        if b[0] == 0 {
            break;
        }
        buf.push(b[0]);
        if buf.len() > 512 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "CString too long"));
        }
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

pub struct ParsedMap {
    pub info: MapInfo,
    pub slots: Vec<SlotInfo>,
    pub layout_style: u8,
}

impl ParsedMap {
    pub fn load_mpq(
        path: &Path,
        common_j: Option<&[u8]>,
        blizzard_j: Option<&[u8]>,
    ) -> io::Result<Self> {
        let map_data = fs::read(path)?;
        let map_size = map_data.len() as u32;

        let mut map_archive = Archive::open(path)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("failed to open MPQ archive: {e}")))?;

        let mut val: u32 = 0;
        let mut hasher = Sha1::new();

        // 1. Process common.j
        let mut overrode_common_j = false;
        if let Ok(file) = map_archive.open_file("Scripts\\common.j") {
            let mut buf = vec![0u8; file.size() as usize];
            if let Ok(read_bytes) = file.read(&mut map_archive, &mut buf)
                && read_bytes > 0
            {
                overrode_common_j = true;
                val ^= xor_rotate_left(&buf[..read_bytes]);
                hasher.update(&buf[..read_bytes]);
            }
        }
        if !overrode_common_j
            && let Some(cj) = common_j
        {
            val ^= xor_rotate_left(cj);
            hasher.update(cj);
        }

        // 2. Process blizzard.j
        let mut overrode_blizzard_j = false;
        if let Ok(file) = map_archive.open_file("Scripts\\blizzard.j") {
            let mut buf = vec![0u8; file.size() as usize];
            if let Ok(read_bytes) = file.read(&mut map_archive, &mut buf)
                && read_bytes > 0
            {
                overrode_blizzard_j = true;
                val ^= xor_rotate_left(&buf[..read_bytes]);
                hasher.update(&buf[..read_bytes]);
            }
        }
        if !overrode_blizzard_j
            && let Some(bj) = blizzard_j
        {
            val ^= xor_rotate_left(bj);
            hasher.update(bj);
        }

        // 3. Transform with magic constants
        val = val.rotate_left(3);
        val = (val ^ 0x03F1379E).rotate_left(3);
        hasher.update([0x9E, 0x37, 0xF1, 0x03]);

        // 4. Process internal map files
        let file_list = [
            "war3map.j",
            "scripts\\war3map.j",
            "war3map.w3e",
            "war3map.wpm",
            "war3map.doo",
            "war3map.w3u",
            "war3map.w3b",
            "war3map.w3d",
            "war3map.w3a",
            "war3map.w3q",
        ];

        let mut found_script = false;
        for fname in file_list {
            if found_script && fname == "scripts\\war3map.j" {
                continue;
            }
            if let Ok(file) = map_archive.open_file(fname) {
                let size = file.size() as usize;
                if size > 0 && size != 0xFFFF_FFFF {
                    let mut buf = vec![0u8; size];
                    if let Ok(read_bytes) = file.read(&mut map_archive, &mut buf)
                        && read_bytes > 0
                    {
                        if fname == "war3map.j" || fname == "scripts\\war3map.j" {
                            found_script = true;
                        }
                        val = (val ^ xor_rotate_left(&buf[..read_bytes])).rotate_left(3);
                        hasher.update(&buf[..read_bytes]);
                    }
                }
            }
        }

        let map_crc = val;
        let mut map_sha1 = [0u8; 20];
        map_sha1.copy_from_slice(&hasher.finalize());

        // 5. Parse war3map.w3i
        let mut width = 128u16;
        let mut height = 128u16;
        let mut num_players = 12u8;
        let mut num_teams = 2u8;
        let mut slots = Vec::new();
        let mut map_options = 0u32;

        if let Ok(w3i_file) = map_archive.open_file("war3map.w3i") {
            let size = w3i_file.size() as usize;
            if size > 0 {
                let mut buf = vec![0u8; size];
                if w3i_file.read(&mut map_archive, &mut buf).is_ok() {
                    let mut cursor = Cursor::new(&buf[..]);
                    let mut u32_buf = [0u8; 4];

                    if cursor.read_exact(&mut u32_buf).is_ok() {
                        let file_format = u32::from_le_bytes(u32_buf);
                        if file_format == 18 || file_format == 25 {
                            let _ = cursor.seek(SeekFrom::Current(8)); // saves + editor version
                            for _ in 0..4 {
                                let _ = read_cstring(&mut cursor);
                            }
                            let _ = cursor.seek(SeekFrom::Current(32 + 16));

                            let _ = cursor.read_exact(&mut u32_buf);
                            let raw_w = u32::from_le_bytes(u32_buf) as u16;
                            let _ = cursor.read_exact(&mut u32_buf);
                            let raw_h = u32::from_le_bytes(u32_buf) as u16;
                            let _ = cursor.read_exact(&mut u32_buf);
                            let raw_flags = u32::from_le_bytes(u32_buf);

                            width = raw_w;
                            height = raw_h;
                            map_options = raw_flags & (MAPOPT_MELEE | MAPOPT_FIXEDPLAYERSETTINGS | MAPOPT_CUSTOMFORCES);

                            let _ = cursor.seek(SeekFrom::Current(1));
                            let _ = cursor.seek(SeekFrom::Current(4));
                            if file_format == 25 {
                                let _ = read_cstring(&mut cursor);
                            }
                            for _ in 0..3 {
                                let _ = read_cstring(&mut cursor);
                            }
                            let _ = cursor.seek(SeekFrom::Current(4));
                            if file_format == 25 {
                                let _ = read_cstring(&mut cursor);
                            }
                            for _ in 0..3 {
                                let _ = read_cstring(&mut cursor);
                            }
                            if file_format == 25 {
                                let _ = cursor.seek(SeekFrom::Current(24));
                                let _ = read_cstring(&mut cursor);
                                let _ = cursor.seek(SeekFrom::Current(5));
                            }

                            if cursor.read_exact(&mut u32_buf).is_ok() {
                                let raw_players = u32::from_le_bytes(u32_buf).min(24);
                                for i in 0..raw_players {
                                    let mut slot = SlotInfo {
                                        pid: 0,
                                        download_status: 255,
                                        slot_status: SlotStatus::Open as u8,
                                        computer: 0,
                                        team: (i / 6) as u8,
                                        colour: i as u8,
                                        race: 0x20, // random
                                        computer_type: 1,
                                        handicap: 100,
                                    };

                                    let _ = cursor.read_exact(&mut u32_buf); // player type / colour
                                    let colour = u32::from_le_bytes(u32_buf) as u8;
                                    slot.colour = colour;

                                    let _ = cursor.read_exact(&mut u32_buf); // status
                                    let status = u32::from_le_bytes(u32_buf);
                                    if status == 2 {
                                        slot.slot_status = SlotStatus::Occupied as u8;
                                        slot.computer = 1;
                                    } else if status != 1 {
                                        slot.slot_status = SlotStatus::Closed as u8;
                                    }

                                    let _ = cursor.read_exact(&mut u32_buf); // race
                                    let race = u32::from_le_bytes(u32_buf);
                                    slot.race = match race {
                                        1 => 0x01, // Human
                                        2 => 0x02, // Orc
                                        3 => 0x04, // Undead
                                        4 => 0x08, // NightElf
                                        _ => 0x20, // Random
                                    };

                                    let _ = cursor.seek(SeekFrom::Current(4));
                                    let _ = read_cstring(&mut cursor); // player name
                                    let _ = cursor.seek(SeekFrom::Current(16)); // start pos

                                    if slot.slot_status != SlotStatus::Closed as u8 {
                                        slots.push(slot);
                                    }
                                }

                                if cursor.read_exact(&mut u32_buf).is_ok() {
                                    let raw_teams = u32::from_le_bytes(u32_buf).min(12);
                                    num_teams = raw_teams as u8;
                                    for team in 0..raw_teams {
                                        let _ = cursor.read_exact(&mut u32_buf); // flags
                                        let _ = cursor.read_exact(&mut u32_buf);
                                        let player_mask = u32::from_le_bytes(u32_buf);
                                        for j in 0..24 {
                                            if (player_mask & (1 << j)) != 0 {
                                                for slot in &mut slots {
                                                    if slot.colour == j as u8 {
                                                        slot.team = team as u8;
                                                    }
                                                }
                                            }
                                        }
                                        let _ = read_cstring(&mut cursor); // team name
                                    }
                                }

                                num_players = slots.len() as u8;
                            }
                        }
                    }
                }
            }
        }

        if slots.is_empty() {
            slots = (0..12)
                .map(|i| SlotInfo {
                    pid: 0,
                    download_status: 255,
                    slot_status: SlotStatus::Open as u8,
                    computer: 0,
                    team: (i / 6) as u8,
                    colour: i as u8,
                    race: 0x20,
                    computer_type: 1,
                    handicap: 100,
                })
                .collect();
            num_players = 12;
            num_teams = 2;
        }

        let layout_style = if (map_options & MAPOPT_CUSTOMFORCES) == 0 {
            0
        } else if (map_options & MAPOPT_FIXEDPLAYERSETTINGS) == 0 {
            1
        } else {
            3
        };

        let mut game_type = MAPGAMETYPE_TYPESCENARIO | MAPGAMETYPE_SIZELARGE | MAPGAMETYPE_OBSNONE;
        if map_options & MAPOPT_MELEE != 0 {
            game_type = MAPGAMETYPE_TYPEMELEE | MAPGAMETYPE_SIZELARGE | MAPGAMETYPE_OBSNONE;
        }

        let mut flags: u32 = 0x0000_0002; // MAPSPEED_FAST
        flags |= 0x0000_0800;             // MAPVIS_DEFAULT
        flags |= 0x0000_4000;             // MAPFLAG_TEAMSTOGETHER
        flags |= 0x0006_0000;             // MAPFLAG_FIXEDTEAMS

        let info = MapInfo {
            path: path.to_string_lossy().to_string(),
            size: map_size,
            info: map_size,
            crc: map_crc,
            sha1: map_sha1,
            num_players,
            num_teams,
            width,
            height,
            game_type,
            flags,
            data: Some(Arc::new(map_data)),
        };

        Ok(Self {
            info,
            slots,
            layout_style,
        })
    }
}
