use crate::crypt::{self, HASH_FILE_KEY, HASH_NAME_A, HASH_NAME_B, HASH_TABLE_OFFSET};
use crate::error::MpqError;

pub const HASH_ENTRY_EMPTY: u32 = 0xFFFF_FFFF;
pub const HASH_ENTRY_DELETED: u32 = 0xFFFF_FFFE;
pub const HASH_ENTRY_SIZE: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct MpqHashEntry {
    pub hash_a: u32,
    pub hash_b: u32,
    pub locale: u16,
    pub platform: u16,
    pub block_index: u32,
}

#[derive(Debug, Clone)]
pub struct MpqHashTable {
    entries: Vec<MpqHashEntry>,
}

impl MpqHashTable {
    pub fn parse_and_decrypt(data: &[u8], count: usize) -> Result<Self, MpqError> {
        let expected_bytes = count * HASH_ENTRY_SIZE;
        if data.len() < expected_bytes {
            return Err(MpqError::CorruptedHashTable);
        }

        let mut decrypted = data[..expected_bytes].to_vec();
        let key = crypt::hash_string("(hash table)", HASH_FILE_KEY);
        crypt::decrypt_bytes(&mut decrypted, key);

        let mut entries = Vec::with_capacity(count);
        for chunk in decrypted.chunks_exact(HASH_ENTRY_SIZE) {
            let hash_a = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let hash_b = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let locale = u16::from_le_bytes([chunk[8], chunk[9]]);
            let platform = u16::from_le_bytes([chunk[10], chunk[11]]);
            let block_index = u32::from_le_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]);

            entries.push(MpqHashEntry {
                hash_a,
                hash_b,
                locale,
                platform,
                block_index,
            });
        }

        Ok(Self { entries })
    }

    /// Finds block index for a filename using circular linear probing (with wrap-around).
    #[must_use]
    pub fn find_block_index(&self, filename: &str) -> Option<u32> {
        let count = self.entries.len();
        if count == 0 {
            return None;
        }

        let mask = count.saturating_sub(1);
        let start_index = (crypt::hash_string(filename, HASH_TABLE_OFFSET) as usize) & mask;
        let hash_a = crypt::hash_string(filename, HASH_NAME_A);
        let hash_b = crypt::hash_string(filename, HASH_NAME_B);

        // Circular probing: inspect each slot modulo count
        for i in 0..count {
            let idx = (start_index + i) % count;
            let entry = &self.entries[idx];

            if entry.block_index == HASH_ENTRY_EMPTY {
                return None;
            }

            if entry.block_index != HASH_ENTRY_DELETED
                && entry.hash_a == hash_a
                && entry.hash_b == hash_b
            {
                return Some(entry.block_index);
            }
        }

        None
    }
}
