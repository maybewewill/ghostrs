use std::fs;
use std::path::Path;
use std::sync::Arc;

use crate::block_table::{
    FILE_COMPRESS, FILE_IMPLODE, FILE_PATCH_FILE, MpqBlockTable,
};
use crate::compression::{decompress_multi, decompress_pkware};
use crate::crypt::{self, HASH_FILE_KEY};
use crate::error::MpqError;
use crate::hash_table::MpqHashTable;
use crate::header::MpqHeader;

#[derive(Debug, Clone)]
pub struct Archive {
    data: Arc<[u8]>,
    header: MpqHeader,
    hash_table: MpqHashTable,
    block_table: MpqBlockTable,
}

#[derive(Debug, Clone)]
pub struct MpqFile {
    filename: String,
    block_index: u32,
    size: usize,
}

impl MpqFile {
    #[inline]
    #[must_use]
    pub fn size(&self) -> u32 {
        self.size as u32
    }

    #[inline]
    #[must_use]
    pub fn name(&self) -> &str {
        &self.filename
    }

    pub fn read(&self, archive: &mut Archive, buf: &mut [u8]) -> Result<usize, MpqError> {
        archive.read_file_by_block(self.block_index, &self.filename, buf)
    }

    pub fn read_ref(&self, archive: &Archive, buf: &mut [u8]) -> Result<usize, MpqError> {
        archive.read_file_by_block(self.block_index, &self.filename, buf)
    }
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, MpqError> {
        let bytes = fs::read(path)?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, MpqError> {
        let data: Arc<[u8]> = bytes.into();
        let header = MpqHeader::locate(&data)?;

        let hash_table_end = header.hash_table_offset + header.hash_table_count * 16;
        if hash_table_end > data.len() {
            return Err(MpqError::CorruptedHashTable);
        }
        let hash_table = MpqHashTable::parse_and_decrypt(
            &data[header.hash_table_offset..hash_table_end],
            header.hash_table_count,
        )?;

        let block_table_end = header.block_table_offset + header.block_table_count * 16;
        if block_table_end > data.len() {
            return Err(MpqError::CorruptedBlockTable);
        }
        let block_table = MpqBlockTable::parse_and_decrypt(
            &data[header.block_table_offset..block_table_end],
            header.block_table_count,
        )?;

        Ok(Self {
            data,
            header,
            hash_table,
            block_table,
        })
    }

    #[must_use]
    pub fn has_file(&self, filename: &str) -> bool {
        self.hash_table.find_block_index(filename).is_some()
    }

    #[must_use]
    pub fn file_size(&self, filename: &str) -> Option<u32> {
        let block_idx = self.hash_table.find_block_index(filename)?;
        let block = self.block_table.get(block_idx as usize)?;
        Some(block.file_size as u32)
    }

    pub fn open_file(&mut self, filename: &str) -> Result<MpqFile, MpqError> {
        let block_index = self
            .hash_table
            .find_block_index(filename)
            .ok_or_else(|| MpqError::FileNotFound(filename.to_string()))?;

        let block = self
            .block_table
            .get(block_index as usize)
            .ok_or_else(|| MpqError::FileNotFound(filename.to_string()))?;

        Ok(MpqFile {
            filename: filename.to_string(),
            block_index,
            size: block.file_size,
        })
    }

    pub fn read_file(&self, filename: &str) -> Result<Vec<u8>, MpqError> {
        let block_index = self
            .hash_table
            .find_block_index(filename)
            .ok_or_else(|| MpqError::FileNotFound(filename.to_string()))?;

        let block = self
            .block_table
            .get(block_index as usize)
            .ok_or_else(|| MpqError::FileNotFound(filename.to_string()))?;

        let mut out = vec![0u8; block.file_size];
        let bytes_read = self.read_file_by_block(block_index, filename, &mut out)?;
        out.truncate(bytes_read);
        Ok(out)
    }

    pub(crate) fn read_file_by_block(
        &self,
        block_index: u32,
        filename: &str,
        out: &mut [u8],
    ) -> Result<usize, MpqError> {
        let block = self
            .block_table
            .get(block_index as usize)
            .ok_or_else(|| MpqError::FileNotFound(filename.to_string()))?;

        if block.file_size == 0 {
            return Ok(0);
        }

        if block.flags & FILE_PATCH_FILE != 0 {
            return Err(MpqError::UnsupportedCompression(0xFF));
        }

        let abs_file_pos = self.header.archive_offset + block.file_pos;
        if abs_file_pos + block.compressed_size > self.data.len() {
            return Err(MpqError::UnexpectedEof {
                expected: abs_file_pos + block.compressed_size,
                got: self.data.len(),
            });
        }

        let block_slice = &self.data[abs_file_pos..abs_file_pos + block.compressed_size];

        let mut file_key = 0u32;
        if block.is_encrypted() {
            let basename = filename
                .rsplit_once(['\\', '/'])
                .map_or(filename, |(_, base)| base);
            file_key = crypt::hash_string(basename, HASH_FILE_KEY);
            if block.has_fixed_key() {
                file_key = (file_key.wrapping_add(block.file_pos as u32)) ^ (block.file_size as u32);
            }
        }

        if block.is_single_unit() {
            return self.read_single_unit(block, block_slice, file_key, out);
        }

        self.read_sector_file(block, block_slice, file_key, out)
    }

    fn read_single_unit(
        &self,
        block: &crate::block_table::MpqBlockEntry,
        raw_data: &[u8],
        file_key: u32,
        out: &mut [u8],
    ) -> Result<usize, MpqError> {
        let mut in_buf = raw_data.to_vec();
        if block.is_encrypted() {
            crypt::decrypt_bytes(&mut in_buf, file_key);
        }

        if block.is_compressed() && out.len() > in_buf.len() {
            decompress_multi(&in_buf, out)
        } else if block.is_imploded() {
            decompress_pkware(&in_buf, out)
        } else {
            let to_copy = block.file_size.min(out.len()).min(in_buf.len());
            out[..to_copy].copy_from_slice(&in_buf[..to_copy]);
            Ok(to_copy)
        }
    }

    fn read_sector_file(
        &self,
        block: &crate::block_table::MpqBlockEntry,
        raw_data: &[u8],
        file_key: u32,
        out: &mut [u8],
    ) -> Result<usize, MpqError> {
        let sector_size = self.header.sector_size;
        if sector_size == 0 {
            return Err(MpqError::InvalidSectorTable);
        }

        let num_sectors = block.file_size.div_ceil(sector_size);

        if block.flags & (FILE_COMPRESS | FILE_IMPLODE) == 0 {
            let to_copy = block.file_size.min(out.len()).min(raw_data.len());
            out[..to_copy].copy_from_slice(&raw_data[..to_copy]);
            return Ok(to_copy);
        }

        let sector_table_size = (num_sectors + 1) * 4;
        if raw_data.len() < sector_table_size {
            return Err(MpqError::InvalidSectorTable);
        }

        let mut sector_table_bytes = raw_data[..sector_table_size].to_vec();
        if block.is_encrypted() {
            crypt::decrypt_bytes(&mut sector_table_bytes, file_key.wrapping_sub(1));
        }

        let mut sector_offsets = Vec::with_capacity(num_sectors + 1);
        for chunk in sector_table_bytes.chunks_exact(4) {
            sector_offsets.push(u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) as usize);
        }

        let mut total_written = 0usize;
        for i in 0..num_sectors {
            let start = sector_offsets[i];
            let end = sector_offsets[i + 1];
            if start > end || end > raw_data.len() {
                return Err(MpqError::InvalidSectorTable);
            }

            let mut sector_data = raw_data[start..end].to_vec();
            if block.is_encrypted() {
                crypt::decrypt_bytes(&mut sector_data, file_key.wrapping_add(i as u32));
            }

            let sector_unpacked_size = if i == num_sectors - 1 {
                block.file_size - i * sector_size
            } else {
                sector_size
            };

            let out_end = (total_written + sector_unpacked_size).min(out.len());
            let target_slice = &mut out[total_written..out_end];

            let written = if sector_data.len() == sector_unpacked_size {
                let to_copy = sector_data.len().min(target_slice.len());
                target_slice[..to_copy].copy_from_slice(&sector_data[..to_copy]);
                to_copy
            } else if block.is_compressed() {
                decompress_multi(&sector_data, target_slice)?
            } else if block.is_imploded() {
                decompress_pkware(&sector_data, target_slice)?
            } else {
                let to_copy = sector_data.len().min(target_slice.len());
                target_slice[..to_copy].copy_from_slice(&sector_data[..to_copy]);
                to_copy
            };

            total_written += written;
        }

        Ok(total_written)
    }
}
