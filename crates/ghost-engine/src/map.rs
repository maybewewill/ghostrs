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

        let file_name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("map.w3x");
        let wc3_map_path = format!("Maps\\Download\\{file_name}");

        let info = MapInfo {
            path: wc3_map_path,
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
            layout_style,
        };

        Ok(Self {
            info,
            slots,
            layout_style,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_parse_iccup_dota_map() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_dir = manifest_dir.parent().unwrap().parent().unwrap();
        let map_path = workspace_dir.join("maps").join("iCCup DotA 454.w3x");

        if !map_path.exists() {
            eprintln!("Map file not found at {:?}", map_path);
            return;
        }

        let common_j_path = workspace_dir.join("maps").join("common.j");
        let blizzard_j_path = workspace_dir.join("maps").join("blizzard.j");

        let common_j = fs::read(&common_j_path).ok();
        let blizzard_j = fs::read(&blizzard_j_path).ok();

        let parsed = ParsedMap::load_mpq(&map_path, common_j.as_deref(), blizzard_j.as_deref())
            .expect("Failed to parse iCCup DotA 454.w3x");

        println!("=== MAP INFO ===");
        println!("Path: {}", parsed.info.path);
        println!("Size: {} bytes", parsed.info.size);
        println!("CRC: 0x{:08X}", parsed.info.crc);
        println!("SHA1: {:02x?}", parsed.info.sha1);
        println!("Dimensions: {}x{}", parsed.info.width, parsed.info.height);
        println!("Players: {}, Teams: {}", parsed.info.num_players, parsed.info.num_teams);
        println!("Layout Style: {}", parsed.layout_style);
        println!("Game Type: 0x{:08X}", parsed.info.game_type);
        println!("Flags: 0x{:08X}", parsed.info.flags);
        println!("Slots count: {}", parsed.slots.len());
        for (i, slot) in parsed.slots.iter().enumerate() {
            println!("  Slot {}: team={}, colour={}, status={}, race=0x{:02X}, computer={}", 
                i, slot.team, slot.colour, slot.slot_status, slot.race, slot.computer);
        }

        assert_eq!(parsed.info.path, "Maps\\Download\\iCCup DotA 454.w3x");
        assert_eq!(parsed.info.size, 17020779);
        assert_eq!(parsed.info.crc, 0x4308685B);
        assert_eq!(parsed.info.width, 118);
        assert_eq!(parsed.info.height, 120);
        assert_eq!(parsed.info.num_players, 10);
        assert_eq!(parsed.info.num_teams, 2);
        assert_eq!(parsed.layout_style, 3);
        assert_eq!(parsed.slots.len(), 10);

        // Sentinel team 0 (slots 0..5), colours 1..5
        for s in &parsed.slots[0..5] {
            assert_eq!(s.team, 0);
            assert_eq!(s.race, 0x08); // NightElf
        }
        // Scourge team 1 (slots 5..10), colours 7..11
        for s in &parsed.slots[5..10] {
            assert_eq!(s.team, 1);
            assert_eq!(s.race, 0x04); // Undead
        }
    }

    #[test]
    fn test_iccup_dota_game_simulation_and_packets() {
        use std::time::Duration;
        use bytes::{BufMut, BytesMut};
        use tokio::sync::mpsc;
        use ghost_net::{AnyFrame, PlayerLink};
        use ghost_protocol::frame::Frame;
        use ghost_protocol::w3gs::ids;
        use crate::actor::tests_support::reqjoin_bytes;
        use crate::state::{GameConfig, GamePhase, GameState};
        use crate::handle::GameCmd;

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_dir = manifest_dir.parent().unwrap().parent().unwrap();
        let map_path = workspace_dir.join("maps").join("iCCup DotA 454.w3x");

        if !map_path.exists() {
            return;
        }

        let common_j = fs::read(workspace_dir.join("maps").join("common.j")).ok();
        let blizzard_j = fs::read(workspace_dir.join("maps").join("blizzard.j")).ok();

        let parsed = ParsedMap::load_mpq(&map_path, common_j.as_deref(), blizzard_j.as_deref())
            .expect("Failed to parse map");

        let game_cfg = GameConfig {
            name: "DotA 5v5 -ap #1".into(),
            owner: "slash".into(),
            host_counter: 12345,
            num_slots: parsed.slots.len(),
            latency: Duration::from_millis(50),
            sync_limit: 50,
            map: parsed.info.clone(),
            virtual_host_name: "|cFFEB0000iCCup".into(),
            reconnect_wait: Duration::from_secs(180),
            custom_slots: Some(parsed.slots.clone()),
            relay: None,
        };

        let mut st = GameState::new(game_cfg);

        // Verify DotA stats tracker and HCL initialized
        assert!(st.dota.is_some());
        assert_eq!(st.phase, GamePhase::Lobby);
        assert_eq!(st.slots.len(), 10);

        // 1. Connect Player 1 (Alice)
        let (tx1, mut rx1) = mpsc::channel(128);
        st.add_conn(1, PlayerLink::for_test(tx1), [192, 168, 1, 10]);
        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Alice"))));

        // Verify Alice seated in Sentinel Slot 0
        let p1 = st.players.by_name_partial("Alice").unwrap();
        assert_eq!(p1.pid, 1);
        let wire_slots = st.slots.as_wire();
        assert_eq!(wire_slots[0].slot_status, 2); // Occupied
        assert_eq!(wire_slots[0].pid, 1);
        assert_eq!(wire_slots[0].team, 0); // Sentinel
        assert_eq!(wire_slots[0].colour, 1); // Blue

        // Drain Alice's packets: should contain SLOT_INFO_JOIN, MAP_CHECK, GPS_INIT, SLOT_INFO
        let mut alice_packets = Vec::new();
        while let Ok(b) = rx1.try_recv() {
            alice_packets.push(b[1]); // Packet ID byte
        }
        assert!(alice_packets.contains(&ids::SLOT_INFO_JOIN));
        assert!(alice_packets.contains(&ids::MAP_CHECK));

        // 2. Connect Player 2 (Bob)
        let (tx2, mut rx2) = mpsc::channel(128);
        st.add_conn(2, PlayerLink::for_test(tx2), [192, 168, 1, 11]);
        st.on_frame(2, AnyFrame::W3gs(Frame::new(ids::REQ_JOIN, reqjoin_bytes("Bob"))));

        let p2 = st.players.by_name_partial("Bob").unwrap();
        assert_eq!(p2.pid, 2);
        let wire_slots = st.slots.as_wire();
        assert_eq!(wire_slots[1].slot_status, 2); // Occupied
        assert_eq!(wire_slots[1].pid, 2);
        assert_eq!(wire_slots[1].team, 0); // Sentinel
        assert_eq!(wire_slots[1].colour, 2); // Teal

        // 3. Start game with !start command
        st.handle_cmd(GameCmd::Start { by: "slash".into() });
        assert!(matches!(st.phase, GamePhase::Countdown { .. }));

        // Tick countdown down to 0
        for _ in 0..6 {
            st.on_tick(0);
        }
        assert_eq!(st.phase, GamePhase::Loading);

        // 4. Both players report GAME_LOADED_SELF
        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::GAME_LOADED_SELF, bytes::Bytes::new())));
        st.on_frame(2, AnyFrame::W3gs(Frame::new(ids::GAME_LOADED_SELF, bytes::Bytes::new())));

        // All players loaded -> Game is live Playing!
        assert_eq!(st.phase, GamePhase::Playing);

        // Drain pending packets from rx1 and rx2
        while rx1.try_recv().is_ok() {}
        while rx2.try_recv().is_ok() {}

        // 5. Game tick in Playing state: Action packet W3GS_INCOMING_ACTION is broadcast
        st.on_tick(0);

        let p1_pkt = rx1.try_recv().expect("Alice must receive INCOMING_ACTION clock tick");
        let p2_pkt = rx2.try_recv().expect("Bob must receive INCOMING_ACTION clock tick");
        assert_eq!(p1_pkt[1], ids::INCOMING_ACTION);
        assert_eq!(p2_pkt[1], ids::INCOMING_ACTION);

        // 6. Alice sends an action (e.g. hero order)
        let mut action_body = BytesMut::new();
        action_body.put_u32_le(0); // crc
        action_body.put_slice(&[0x10, 0x01, 0x02, 0x03]); // arbitrary action payload
        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::OUTGOING_ACTION, action_body.freeze())));

        // Alice sends keepalive
        let mut keepalive = BytesMut::new();
        keepalive.put_u32_le(0);
        st.on_frame(1, AnyFrame::W3gs(Frame::new(ids::OUTGOING_KEEPALIVE, keepalive.freeze())));

        // Next tick: the action is bundled into INCOMING_ACTION and sent to all players
        st.on_tick(0);

        let p1_action_tick = rx1.try_recv().expect("Alice must receive action tick");
        let p2_action_tick = rx2.try_recv().expect("Bob must receive action tick");
        assert_eq!(p1_action_tick[1], ids::INCOMING_ACTION);
        assert_eq!(p2_action_tick[1], ids::INCOMING_ACTION);

        println!("Simulation completed successfully: Map resolved, Lobby seated, Game started, Loading finished, Playing ticks & Actions streamed!");
    }
}



