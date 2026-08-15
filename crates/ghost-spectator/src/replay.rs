use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;
use flate2::Compression;
use flate2::write::ZlibEncoder;

pub struct ReplayWriter {
    file: File,
    buffer: Vec<u8>,
    uncompressed_size: u32,
    compressed_size: u32,
    num_blocks: u32,
}

impl ReplayWriter {
    pub fn create(path: &Path, _game_name: &str) -> io::Result<Self> {
        let mut file = File::create(path)?;
        // Write 68-byte placeholder header
        let mut header = [0u8; 68];
        let intro = b"Warcraft III recorded game\x1A\0";
        header[..28].copy_from_slice(intro);
        header[28..32].copy_from_slice(&0x44u32.to_le_bytes()); // header size = 68
        header[36..40].copy_from_slice(&1u32.to_le_bytes());    // header version = 1
        header[48..52].copy_from_slice(b"PX3W");                 // "W3XP"
        header[52..56].copy_from_slice(&26u32.to_le_bytes());   // version 26
        header[56..58].copy_from_slice(&6059u16.to_le_bytes()); // build 6059
        file.write_all(&header)?;
        Ok(Self {
            file,
            buffer: Vec::with_capacity(8192),
            uncompressed_size: 0,
            compressed_size: 0,
            num_blocks: 0,
        })
    }

    pub fn push_block(&mut self, block: &[u8]) -> io::Result<()> {
        self.buffer.extend_from_slice(block);
        while self.buffer.len() >= 8192 {
            let chunk: Vec<u8> = self.buffer.drain(..8192).collect();
            self.flush_chunk(&chunk)?;
        }
        Ok(())
    }

    fn flush_chunk(&mut self, uncompressed: &[u8]) -> io::Result<()> {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(uncompressed)?;
        let compressed = encoder.finish()?;

        let u_len = uncompressed.len() as u16;
        let c_len = compressed.len() as u16;
        let mut block_header = [0u8; 8];
        block_header[0..2].copy_from_slice(&c_len.to_le_bytes());
        block_header[2..4].copy_from_slice(&u_len.to_le_bytes());
        let crc = crc32fast::hash(&compressed);
        block_header[4..8].copy_from_slice(&crc.to_le_bytes());

        self.file.write_all(&block_header)?;
        self.file.write_all(&compressed)?;

        self.uncompressed_size += uncompressed.len() as u32;
        self.compressed_size += (8 + compressed.len()) as u32;
        self.num_blocks += 1;
        Ok(())
    }

    pub fn finish(mut self) -> io::Result<()> {
        if !self.buffer.is_empty() {
            let remaining = std::mem::take(&mut self.buffer);
            self.flush_chunk(&remaining)?;
        }

        self.file.seek(SeekFrom::Start(32))?;
        self.file.write_all(&self.compressed_size.to_le_bytes())?;
        self.file.seek(SeekFrom::Start(40))?;
        self.file.write_all(&self.uncompressed_size.to_le_bytes())?;
        self.file.write_all(&self.num_blocks.to_le_bytes())?;
        self.file.flush()?;
        Ok(())
    }
}
