use crate::error::MpqError;

pub const ID_MPQ_ARCHIVE: [u8; 4] = [b'M', b'P', b'Q', 0x1A];
pub const ID_MPQ_USER_DATA: [u8; 4] = [b'M', b'P', b'Q', 0x1B];

pub const HEADER_SIZE_V1: usize = 32;

#[derive(Debug, Clone, Copy)]
pub struct MpqHeader {
    pub archive_offset: usize,
    pub header_size: u32,
    pub archive_size: u32,
    pub format_version: u16,
    pub sector_size_shift: u16,
    pub sector_size: usize,
    pub hash_table_offset: usize,
    pub block_table_offset: usize,
    pub hash_table_count: usize,
    pub block_table_count: usize,
}

impl MpqHeader {
    pub fn locate(data: &[u8]) -> Result<Self, MpqError> {
        let len = data.len();
        if len < HEADER_SIZE_V1 {
            return Err(MpqError::HeaderNotFound);
        }

        let mut offset = 0;
        while offset + HEADER_SIZE_V1 <= len {
            if data[offset..offset + 4] == ID_MPQ_ARCHIVE {
                return Self::parse_at(data, offset, offset);
            }
            if data[offset..offset + 4] == ID_MPQ_USER_DATA && offset + 16 <= len {
                let _user_data_size = u32::from_le_bytes([
                    data[offset + 4],
                    data[offset + 5],
                    data[offset + 6],
                    data[offset + 7],
                ]);
                let header_offset = u32::from_le_bytes([
                    data[offset + 8],
                    data[offset + 9],
                    data[offset + 10],
                    data[offset + 11],
                ]);
                let target_offset = offset + (header_offset as usize);
                if target_offset + HEADER_SIZE_V1 <= len
                    && data[target_offset..target_offset + 4] == ID_MPQ_ARCHIVE
                {
                    return Self::parse_at(data, offset, target_offset);
                }
            }

            offset += 512;
        }

        // If not found on 512-byte boundaries, search byte-by-byte as fallback
        for i in 0..len.saturating_sub(HEADER_SIZE_V1) {
            if data[i..i + 4] == ID_MPQ_ARCHIVE {
                return Self::parse_at(data, i, i);
            }
        }

        Err(MpqError::HeaderNotFound)
    }

    fn parse_at(data: &[u8], archive_offset: usize, header_offset: usize) -> Result<Self, MpqError> {
        if header_offset + HEADER_SIZE_V1 > data.len() {
            return Err(MpqError::CorruptedHeader("Header exceeds file boundary"));
        }

        let slice = &data[header_offset..header_offset + HEADER_SIZE_V1];
        let header_size = u32::from_le_bytes([slice[4], slice[5], slice[6], slice[7]]);
        let archive_size = u32::from_le_bytes([slice[8], slice[9], slice[10], slice[11]]);
        let format_version = u16::from_le_bytes([slice[12], slice[13]]);
        let sector_size_shift = u16::from_le_bytes([slice[14], slice[15]]);
        let hash_table_offset_rel = u32::from_le_bytes([slice[16], slice[17], slice[18], slice[19]]) as usize;
        let block_table_offset_rel = u32::from_le_bytes([slice[20], slice[21], slice[22], slice[23]]) as usize;
        let hash_table_count = u32::from_le_bytes([slice[24], slice[25], slice[26], slice[27]]) as usize;
        let block_table_count = u32::from_le_bytes([slice[28], slice[29], slice[30], slice[31]]) as usize;

        let hash_table_offset = archive_offset + hash_table_offset_rel;
        let block_table_offset = archive_offset + block_table_offset_rel;
        let sector_size = 512usize << sector_size_shift;

        Ok(MpqHeader {
            archive_offset,
            header_size,
            archive_size,
            format_version,
            sector_size_shift,
            sector_size,
            hash_table_offset,
            block_table_offset,
            hash_table_count,
            block_table_count,
        })
    }
}
