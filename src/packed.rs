use crate::crc32::*;
use crate::logger::log_info;
use flate2::Decompress;
use flate2::FlushDecompress;
use flate2::Status;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::{Cursor, Read, Result, Write};
use byteorder::{LittleEndian, ReadBytesExt};

pub struct Packed {
    pub m_CRC: CRC32,
    pub m_Valid: bool,
    pub m_Compressed: Vec<u8>, // Изменено с String на Vec<u8>
    pub m_Decompressed: Vec<u8>, // Изменено с String на Vec<u8>
    pub m_HeaderSize: u32,
    pub m_CompressedSize: u32,
    pub m_HeaderVersion: u32,
    pub m_DecompressedSize: u32,
    pub m_NumBlocks: u32,
    pub m_War3Identifier: u32,
    pub m_War3Version: u32,
    pub m_BuildNumber: u16,
    pub m_Flags: u16,
    pub m_ReplayLength: u32,
}

pub fn tzuncompress(source: &[u8], dest: &mut [u8]) -> Result<usize> {
    let mut decompressor = Decompress::new(false);
    let mut total_out = 0;

    let status = decompressor.decompress(source, dest, FlushDecompress::Sync)?;

    match status {
        Status::Ok | Status::StreamEnd => {
            total_out = decompressor.total_out() as usize;
            Ok(total_out)
        }
        _ => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Decompression failed")),
    }
}

impl Packed {
    pub fn new() -> Self {
        let mut m_CRC = CRC32::new();
        m_CRC.initialize();
        Packed {
            m_CRC,
            m_Valid: true,
            m_Compressed: Vec::new(),
            m_Decompressed: Vec::new(),
            m_HeaderSize: 0,
            m_CompressedSize: 0,
            m_HeaderVersion: 0,
            m_DecompressedSize: 0,
            m_NumBlocks: 0,
            m_War3Identifier: 0,
            m_War3Version: 0,
            m_BuildNumber: 0,
            m_Flags: 0,
            m_ReplayLength: 0,
        }
    }

    pub fn get_valid(&self) -> bool {
        self.m_Valid
    }
    pub fn get_header_size(&self) -> u32 {
        self.m_HeaderSize
    }
    pub fn get_compressed_size(&self) -> u32 {
        self.m_CompressedSize
    }
    pub fn get_header_version(&self) -> u32 {
        self.m_HeaderVersion
    }
    pub fn get_decompressed_size(&self) -> u32 {
        self.m_DecompressedSize
    }
    pub fn get_num_blocks(&self) -> u32 {
        self.m_NumBlocks
    }
    pub fn get_war3_identifier(&self) -> u32 {
        self.m_War3Identifier
    }
    pub fn get_war3_version(&self) -> u32 {
        self.m_War3Version
    }
    pub fn get_build_number(&self) -> u16 {
        self.m_BuildNumber
    }
    pub fn get_flags(&self) -> u16 {
        self.m_Flags
    }
    pub fn get_replay_length(&self) -> u32 {
        self.m_ReplayLength
    }

    pub fn set_war3_version(&mut self, version: u32) {
        self.m_War3Version = version;
    }
    pub fn set_build_number(&mut self, build: u16) {
        self.m_BuildNumber = build;
    }
    pub fn set_flags(&mut self, flags: u16) {
        self.m_Flags = flags;
    }
    pub fn set_replay_length(&mut self, length: u32) {
        self.m_ReplayLength = length;
    }

    pub fn load(&mut self, file_name: String, all_blocks: bool) {
        self.m_Valid = true;
        log_info(&format!("[Packed] loading data from file: {}", file_name));
        self.m_Compressed = match std::fs::read(&file_name) {
            Ok(data) => data,
            Err(e) => {
                log_info(&format!("[Packed] Error reading file {}: {}", file_name, e));
                self.m_Valid = false;
                return;
            }
        };
        self.decompress(all_blocks);
    }

    pub fn save(&self, _tft: bool, file_name: String) -> bool {
        if self.m_Valid {
            log_info(&format!("[Packed] saving data to file: {}", file_name));
            let mut file = match std::fs::File::create(&file_name) {
                Ok(file) => file,
                Err(e) => {
                    log_info(&format!("[Packed] Error creating file {}: {}", file_name, e));
                    return false;
                }
            };

            if let Err(e) = file.write_all(&self.m_Compressed) {
                log_info(&format!("[Packed] Error writing to file {}: {}", file_name, e));
                return false;
            }

            log_info(&format!("[Packed] Data saved successfully to {}", file_name));
            true
        } else {
            log_info("[Packed] Invalid data, cannot save.");
            false
        }
    }

    pub fn extract(&mut self, in_file_name: String, out_file_name: String) -> bool {
        self.m_Valid = true;
        log_info(&format!(
            "[Packed] extracting data from file: {} to file: {}",
            in_file_name, out_file_name
        ));
        self.m_Compressed = match std::fs::read(&in_file_name) {
            Ok(data) => data,
            Err(e) => {
                log_info(&format!("[Packed] Error reading file {}: {}", in_file_name, e));
                self.m_Valid = false;
                return false;
            }
        };
        self.decompress(true);

        if self.m_Valid {
            let mut file = match std::fs::File::create(&out_file_name) {
                Ok(file) => file,
                Err(e) => {
                    log_info(&format!("[Packed] Error creating file {}: {}", out_file_name, e));
                    self.m_Valid = false;
                    return false;
                }
            };

            if let Err(e) = file.write_all(&self.m_Decompressed) {
                log_info(&format!("[Packed] Error writing to file {}: {}", out_file_name, e));
                self.m_Valid = false;
                return false;
            }

            log_info(&format!("[Packed] Data extracted successfully to {}", out_file_name));
            true
        } else {
            log_info("[Packed] Invalid data, cannot extract.");
            false
        }
    }

    pub fn decompress(&mut self, all_blocks: bool) {
        log_info("[Packed] decompressing data");

        self.m_Decompressed.clear();
        let mut cursor = Cursor::new(&self.m_Compressed);

        // Read header
        let mut signature = vec![0u8; 28];
        if cursor.read_exact(&mut signature).is_err()
            || signature != b"Warcraft III recorded game\x1A".to_vec()
        {
            log_info("[Packed] not a valid packed file");
            self.m_Valid = false;
            return;
        }

        self.m_HeaderSize = match cursor.read_u32::<LittleEndian>() {
            Ok(size) => size,
            Err(_) => {
                log_info("[Packed] failed to read header size");
                self.m_Valid = false;
                return;
            }
        };

        self.m_CompressedSize = match cursor.read_u32::<LittleEndian>() {
            Ok(size) => size,
            Err(_) => {
                log_info("[Packed] failed to read compressed size");
                self.m_Valid = false;
                return;
            }
        };

        self.m_HeaderVersion = match cursor.read_u32::<LittleEndian>() {
            Ok(version) => version,
            Err(_) => {
                log_info("[Packed] failed to read header version");
                self.m_Valid = false;
                return;
            }
        };

        self.m_DecompressedSize = match cursor.read_u32::<LittleEndian>() {
            Ok(size) => size,
            Err(_) => {
                log_info("[Packed] failed to read decompressed size");
                self.m_Valid = false;
                return;
            }
        };

        self.m_NumBlocks = match cursor.read_u32::<LittleEndian>() {
            Ok(num) => num,
            Err(_) => {
                log_info("[Packed] failed to read number of blocks");
                self.m_Valid = false;
                return;
            }
        };

        if self.m_HeaderVersion == 0 {
            log_info("[Packed] header version is too old");
            self.m_Valid = false;
            return;
        }

        self.m_War3Identifier = match cursor.read_u32::<LittleEndian>() {
            Ok(id) => id,
            Err(_) => {
                log_info("[Packed] failed to read version identifier");
                self.m_Valid = false;
                return;
            }
        };

        self.m_War3Version = match cursor.read_u32::<LittleEndian>() {
            Ok(version) => version,
            Err(_) => {
                log_info("[Packed] failed to read version number");
                self.m_Valid = false;
                return;
            }
        };

        self.m_BuildNumber = match cursor.read_u16::<LittleEndian>() {
            Ok(build) => build,
            Err(_) => {
                log_info("[Packed] failed to read build number");
                self.m_Valid = false;
                return;
            }
        };

        self.m_Flags = match cursor.read_u16::<LittleEndian>() {
            Ok(flags) => flags,
            Err(_) => {
                log_info("[Packed] failed to read flags");
                self.m_Valid = false;
                return;
            }
        };

        self.m_ReplayLength = match cursor.read_u32::<LittleEndian>() {
            Ok(length) => length,
            Err(_) => {
                log_info("[Packed] failed to read replay length");
                self.m_Valid = false;
                return;
            }
        };

        // Skip CRC (4 bytes)
        if cursor.seek(SeekFrom::Current(4)).is_err() {
            log_info("[Packed] failed to skip CRC");
            self.m_Valid = false;
            return;
        }

        let num_blocks_to_read = if all_blocks {
            self.m_NumBlocks
        } else {
            1.min(self.m_NumBlocks)
        };
        log_info(&format!(
            "[Packed] reading {}/{} blocks",
            num_blocks_to_read, self.m_NumBlocks
        ));

        // Read blocks
        for i in 0..num_blocks_to_read {
            let block_compressed = match cursor.read_u16::<LittleEndian>() {
                Ok(size) => size,
                Err(_) => {
                    log_info(&format!("[Packed] failed to read block {} compressed size", i));
                    self.m_Valid = false;
                    return;
                }
            };

            let block_decompressed = match cursor.read_u16::<LittleEndian>() {
                Ok(size) => size,
                Err(_) => {
                    log_info(&format!("[Packed] failed to read block {} decompressed size", i));
                    self.m_Valid = false;
                    return;
                }
            };

            // Skip checksum (4 bytes)
            if cursor.seek(SeekFrom::Current(4)).is_err() {
                log_info(&format!("[Packed] failed to skip block {} checksum", i));
                self.m_Valid = false;
                return;
            }

            // Read compressed block data
            let mut compressed_data = vec![0u8; block_compressed as usize];
            if cursor.read_exact(&mut compressed_data).is_err() {
                log_info(&format!("[Packed] failed to read block {} data", i));
                self.m_Valid = false;
                return;
            }

            // Decompress block
            let mut decompressed_data = vec![0u8; block_decompressed as usize];
            match tzuncompress(&compressed_data, &mut decompressed_data) {
                Ok(decompressed_size) => {
                    if decompressed_size != block_decompressed as usize {
                        log_info(&format!(
                            "[Packed] block {} decompressed size mismatch: actual {}, expected {}",
                            i, decompressed_size, block_decompressed
                        ));
                        self.m_Valid = false;
                        return;
                    }
                    self.m_Decompressed.extend_from_slice(&decompressed_data);
                }
                Err(e) => {
                    log_info(&format!("[Packed] tzuncompress error for block {}: {}", i, e));
                    self.m_Valid = false;
                    return;
                }
            }
        }

        log_info(&format!(
            "[Packed] decompressed {} bytes",
            self.m_Decompressed.len()
        ));

        if (all_blocks || self.m_NumBlocks == 1) && self.m_Decompressed.len() > self.m_DecompressedSize as usize
        {
            log_info(&format!(
                "[Packed] discarding {} bytes",
                self.m_Decompressed.len() - self.m_DecompressedSize as usize
            ));
            self.m_Decompressed.truncate(self.m_DecompressedSize as usize);
        } else if self.m_Decompressed.len() < self.m_DecompressedSize as usize {
            log_info("[Packed] not enough decompressed data");
            self.m_Valid = false;
        }
    }
}