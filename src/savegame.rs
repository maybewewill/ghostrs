use crate::{gameslot::*, logger::log_info};
use crate::packed::Packed;
use std::io::{Read, Result, BufRead, Cursor};

fn readb<R: Read>(reader: &mut R, buffer: &mut [u8]) -> Result<usize> {
    reader.read(buffer)
}

fn readstr<R: BufRead>(reader: &mut R, output: &mut String) -> Result<usize> {
    output.clear();
    let mut bytes = Vec::new();
    let read_bytes = reader.read_until(0, &mut bytes)?;
    if bytes.ends_with(&[0]) {
        bytes.pop();
    }
    *output = String::from_utf8(bytes).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(read_bytes)
}

pub struct SaveGame {
    base: Packed,
    m_FileName: String,
    m_FileNameNoPath: String,
    m_MapPath: String,
    m_GameName: String,
    m_NumSlots: u8,
    m_Slots: Vec<GameSlot>,
    m_RandomSeed: u32,
    m_MagicNumber: Vec<u8>,
}

impl SaveGame {
    pub fn new(file_name: String, map_path: String, game_name: String, num_slots: u8) -> Self {
        SaveGame {
            base: Packed::new(),
            m_FileName: file_name.clone(),
            m_FileNameNoPath: file_name.split('/').last().unwrap_or("").to_string(),
            m_MapPath: map_path,
            m_GameName: game_name,
            m_NumSlots: num_slots,
            m_Slots: Vec::new(),
            m_RandomSeed: 0,
            m_MagicNumber: vec![0x47, 0x48, 0x4F, 0x53],
        }
    }

    pub fn get_file_name(&self) -> &str {
        &self.m_FileName
    }

    pub fn get_file_name_no_path(&self) -> &str {
        &self.m_FileNameNoPath
    }

    pub fn get_map_path(&self) -> &str {
        &self.m_MapPath
    }

    pub fn get_game_name(&self) -> &str {
        &self.m_GameName
    }

    pub fn get_num_slots(&self) -> u8 {
        self.m_NumSlots
    }

    pub fn get_slots(&self) -> &Vec<GameSlot> {
        &self.m_Slots
    }

    pub fn get_random_seed(&self) -> u32 {
        self.m_RandomSeed
    }

    pub fn get_magic_number(&self) -> &Vec<u8> {
        &self.m_MagicNumber
    }

    pub fn set_file_name(&mut self, file_name: String) {
        self.m_FileName = file_name.clone();
        self.m_FileNameNoPath = file_name.split('/').last().unwrap_or("").to_string();
    }

    pub fn set_file_name_no_path(&mut self, file_name_no_path: String) {
        self.m_FileNameNoPath = file_name_no_path;
    }

    pub fn parse_save_game(&mut self) {
        self.m_MapPath.clear();
        self.m_GameName.clear();
        self.m_Slots.clear();
        self.m_RandomSeed = 0;
        self.m_MagicNumber.clear();

        if self.base.get_flags() != 0 {
            log_info("[SAVEGAME] invalid replay (flags mismatch)");
            self.base.m_Valid = false;
            return;
        }

        let mut cursor = Cursor::new(&self.base.m_Decompressed);
        let mut garbage_string = String::new();
        let mut garbage4 = [0u8; 4];
        let mut garbage2 = [0u8; 2];
        let mut garbage1 = [0u8; 1];
        let mut magic_number = [0u8; 4];

        if readstr(&mut cursor, &mut self.m_MapPath).is_err()
            || readstr(&mut cursor, &mut garbage_string).is_err()
            || readstr(&mut cursor, &mut self.m_GameName).is_err()
            || readstr(&mut cursor, &mut garbage_string).is_err()
            || readstr(&mut cursor, &mut garbage_string).is_err()
            || readb(&mut cursor, &mut garbage4).is_err()
            || readb(&mut cursor, &mut garbage4).is_err()
            || readb(&mut cursor, &mut garbage2).is_err()
            || readb(&mut cursor, &mut garbage1).is_err()
        {
            log_info("[SAVEGAME] failed to parse savegame header");
            self.base.m_Valid = false;
            return;
        }

        self.m_NumSlots = garbage1[0];

        if self.m_NumSlots > 12 {
            log_info("[SAVEGAME] invalid savegame (too many slots)");
            self.base.m_Valid = false;
            return;
        }

        for _ in 0..self.m_NumSlots {
            let mut slot_data = [0u8; 9];
            if readb(&mut cursor, &mut slot_data).is_err() {
                log_info("[SAVEGAME] failed to parse slot data");
                self.base.m_Valid = false;
                return;
            }
            self.m_Slots.push(GameSlot::new(
                slot_data[0], slot_data[1], slot_data[2], slot_data[3],
                slot_data[4], slot_data[5], slot_data[6], slot_data[7], slot_data[8],
            ));
        }

        if readb(&mut cursor, &mut garbage4).is_err() {
            log_info("[SAVEGAME] failed to read random seed");
            self.base.m_Valid = false;
            return;
        }
        self.m_RandomSeed = u32::from_le_bytes(garbage4);

        if readb(&mut cursor, &mut garbage1).is_err()
            || readb(&mut cursor, &mut garbage1).is_err()
            || readb(&mut cursor, &mut magic_number).is_err()
        {
            log_info("[SAVEGAME] failed to parse savegame footer");
            self.base.m_Valid = false;
            return;
        }

        self.m_MagicNumber = magic_number.to_vec();
        self.base.m_Valid = true;
    }
}