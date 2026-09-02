use crate::crypt::{self, HASH_FILE_KEY};
use crate::error::MpqError;

pub const BLOCK_ENTRY_SIZE: usize = 16;

pub const FILE_IMPLODE: u32 = 0x0000_0100; // Explode (PKWARE DCL)
pub const FILE_COMPRESS: u32 = 0x0000_0200; // Multi-compression
pub const FILE_ENCRYPTED: u32 = 0x0001_0000;
pub const FILE_FIX_KEY: u32 = 0x0002_0000;
#[allow(dead_code)]
pub const FILE_PATCH_FILE: u32 = 0x0010_0000;
pub const FILE_SINGLE_UNIT: u32 = 0x0100_0000;
#[allow(dead_code)]
pub const FILE_SECTOR_CRC: u32 = 0x0400_0000;

#[derive(Debug, Clone, Copy)]
pub struct MpqBlockEntry {
    pub file_pos: usize,
    pub compressed_size: usize,
    pub file_size: usize,
    pub flags: u32,
}

impl MpqBlockEntry {
    #[inline]
    #[must_use]
    pub fn is_encrypted(&self) -> bool {
        self.flags & FILE_ENCRYPTED != 0
    }

    #[inline]
    #[must_use]
    pub fn has_fixed_key(&self) -> bool {
        self.flags & FILE_FIX_KEY != 0
    }

    #[inline]
    #[must_use]
    pub fn is_compressed(&self) -> bool {
        self.flags & FILE_COMPRESS != 0
    }

    #[inline]
    #[must_use]
    pub fn is_imploded(&self) -> bool {
        self.flags & FILE_IMPLODE != 0
    }

    #[inline]
    #[must_use]
    pub fn is_single_unit(&self) -> bool {
        self.flags & FILE_SINGLE_UNIT != 0
    }
}

#[derive(Debug, Clone)]
pub struct MpqBlockTable {
    entries: Vec<MpqBlockEntry>,
}

impl MpqBlockTable {
    pub fn parse_and_decrypt(data: &[u8], count: usize) -> Result<Self, MpqError> {
        let expected_bytes = count * BLOCK_ENTRY_SIZE;
        if data.len() < expected_bytes {
            return Err(MpqError::CorruptedBlockTable);
        }

        let mut decrypted = data[..expected_bytes].to_vec();
        let key = crypt::hash_string("(block table)", HASH_FILE_KEY);
        crypt::decrypt_bytes(&mut decrypted, key);

        let mut entries = Vec::with_capacity(count);
        for chunk in decrypted.chunks_exact(BLOCK_ENTRY_SIZE) {
            let file_pos = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize;
            let compressed_size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]) as usize;
            let file_size = u32::from_le_bytes([chunk[8], chunk[9], chunk[10], chunk[11]]) as usize;
            let flags = u32::from_le_bytes([chunk[12], chunk[13], chunk[14], chunk[15]]);

            entries.push(MpqBlockEntry {
                file_pos,
                compressed_size,
                file_size,
                flags,
            });
        }

        Ok(Self { entries })
    }

    #[inline]
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&MpqBlockEntry> {
        self.entries.get(index)
    }

    #[inline]
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
