//! `.w3g` replay container. Byte-for-byte equivalent to GHost++ `CPacked::Compress`
//! (ref/ghostpp/ghost/packed.cpp:275-394).
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

const HEADER_SIZE: u32 = 68;
const BLOCK_SIZE: usize = 8192;
/// GHost++ hardcodes this; the client validates it (replay.cpp:234).
const FLAGS_MULTIPLAYER: u16 = 32768;

pub struct W3gWriter {
    war3_version: u32,
    build: u16,
    tft: bool,
    replay_length_ms: u32,
}

impl W3gWriter {
    pub fn new(war3_version: u32, build: u16, tft: bool) -> Self {
        Self { war3_version, build, tft, replay_length_ms: 0 }
    }

    pub fn set_replay_length(&mut self, ms: u32) {
        self.replay_length_ms = ms;
    }

    /// Packs an already-built replay body into a complete `.w3g` file.
    pub fn pack(&self, decompressed: &[u8]) -> Vec<u8> {
        // Every block must inflate to exactly BLOCK_SIZE, so the tail is padded.
        let mut padded = decompressed.to_vec();
        let pad = BLOCK_SIZE - (padded.len() % BLOCK_SIZE);
        padded.resize(padded.len() + pad, 0);

        let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(padded.len() / BLOCK_SIZE);
        for chunk in padded.chunks_exact(BLOCK_SIZE) {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(chunk).expect("zlib encode into Vec cannot fail");
            blocks.push(enc.finish().expect("zlib finish into Vec cannot fail"));
        }

        let compressed_total: usize = blocks.iter().map(|b| b.len()).sum();
        let file_size = HEADER_SIZE as usize + compressed_total + blocks.len() * 8;

        let mut header = Vec::with_capacity(HEADER_SIZE as usize);
        header.extend_from_slice(b"Warcraft III recorded game\x1A\0");
        header.extend_from_slice(&HEADER_SIZE.to_le_bytes());
        header.extend_from_slice(&(file_size as u32).to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes()); // header version
        header.extend_from_slice(&(padded.len() as u32).to_le_bytes());
        header.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        // "W3XP"/"WAR3" stored reversed on the wire (packed.cpp:326-336).
        header.extend_from_slice(if self.tft { b"PX3W" } else { b"3RAW" });
        header.extend_from_slice(&self.war3_version.to_le_bytes());
        header.extend_from_slice(&self.build.to_le_bytes());
        header.extend_from_slice(&FLAGS_MULTIPLAYER.to_le_bytes());
        header.extend_from_slice(&self.replay_length_ms.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
        debug_assert_eq!(header.len(), HEADER_SIZE as usize);

        let crc = crc32fast::hash(&header);
        header[64..68].copy_from_slice(&crc.to_le_bytes());

        let mut out = Vec::with_capacity(file_size);
        out.extend_from_slice(&header);
        for block in &blocks {
            let mut bh = Vec::with_capacity(8);
            bh.extend_from_slice(&(block.len() as u16).to_le_bytes());
            bh.extend_from_slice(&(BLOCK_SIZE as u16).to_le_bytes());
            bh.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder

            // Folded 16+16 checksum, packed.cpp:377-382.
            let crc1 = { let c = crc32fast::hash(&bh); c ^ (c >> 16) };
            let crc2 = { let c = crc32fast::hash(block); c ^ (c >> 16) };
            let block_crc = (crc1 & 0xFFFF) | (crc2 << 16);
            bh[4..8].copy_from_slice(&block_crc.to_le_bytes());

            out.extend_from_slice(&bh);
            out.extend_from_slice(block);
        }
        debug_assert_eq!(out.len(), file_size);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use flate2::read::ZlibDecoder;

    fn read_u32(d: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
    }

    #[test]
    fn the_header_carries_the_flags_length_and_a_self_consistent_crc() {
        let mut w = W3gWriter::new(26, 6059, true);
        w.set_replay_length(123_456);
        let out = w.pack(&vec![0xABu8; 100]);

        assert_eq!(&out[..28], b"Warcraft III recorded game\x1A\0");
        assert_eq!(read_u32(&out, 28), 68, "header size");
        assert_eq!(read_u32(&out, 36), 1, "header version");
        assert_eq!(&out[48..52], b"PX3W", "W3XP, little-endian on the wire");
        assert_eq!(read_u32(&out, 52), 26, "war3 version");
        assert_eq!(u16::from_le_bytes([out[56], out[57]]), 6059, "build");
        assert_eq!(u16::from_le_bytes([out[58], out[59]]), 32768, "flags must be 32768");
        assert_eq!(read_u32(&out, 60), 123_456, "replay length ms");

        // The stored CRC must equal CRC32 of the header with its CRC field zeroed.
        let stored = read_u32(&out, 64);
        let mut probe = out[..68].to_vec();
        probe[64..68].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(stored, crc32fast::hash(&probe), "header CRC mismatch");
        assert_ne!(stored, 0);
    }

    #[test]
    fn every_block_decompresses_to_exactly_8192_bytes() {
        let w = W3gWriter::new(26, 6059, true);
        // 3 blocks worth, with a deliberately ragged tail.
        let body = vec![0x5Au8; 8192 * 2 + 7];
        let out = w.pack(&body);

        let n_blocks = read_u32(&out, 44) as usize;
        assert_eq!(n_blocks, 3);
        assert_eq!(read_u32(&out, 40) as usize, 8192 * 3, "decompressed size must be padded");

        let mut pos = 68;
        for i in 0..n_blocks {
            let c_len = u16::from_le_bytes([out[pos], out[pos + 1]]) as usize;
            let u_len = u16::from_le_bytes([out[pos + 2], out[pos + 3]]) as usize;
            assert_eq!(u_len, 8192, "block {i} uncompressed size");
            let comp = &out[pos + 8..pos + 8 + c_len];
            let mut dec = Vec::new();
            ZlibDecoder::new(comp)
                .read_to_end(&mut dec)
                .expect("block must be valid zlib");
            assert_eq!(dec.len(), 8192, "block {i} must inflate to 8192 bytes");
            pos += 8 + c_len;
        }
        assert_eq!(pos, out.len(), "compressed size accounting");
        assert_eq!(read_u32(&out, 32) as usize, out.len(), "field 32 is the whole file size");
    }

    #[test]
    fn the_block_crc_folds_the_header_and_data_checksums() {
        let w = W3gWriter::new(26, 6059, true);
        let out = w.pack(&vec![1u8; 10]);
        let c_len = u16::from_le_bytes([out[68], out[69]]) as usize;

        let mut bh = out[68..76].to_vec();
        bh[4..8].copy_from_slice(&0u32.to_le_bytes());
        let crc1 = { let c = crc32fast::hash(&bh); c ^ (c >> 16) };
        let crc2 = { let c = crc32fast::hash(&out[76..76 + c_len]); c ^ (c >> 16) };
        let expected = (crc1 & 0xFFFF) | (crc2 << 16);

        assert_eq!(u32::from_le_bytes([out[72], out[73], out[74], out[75]]), expected);
    }
}
