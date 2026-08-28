//! `.w3g` replay container. Byte-for-byte equivalent to GHost++ `CPacked::Compress`
//! (ref/ghostpp/ghost/packed.cpp:275-394).
use std::io::Write;

use flate2::Compression;
use flate2::write::ZlibEncoder;

const HEADER_SIZE: u32 = 68;
const BLOCK_SIZE: usize = 8192;
/// Header word at 0x3A (the high u16 of the dword at 0x38, whose low u16 is the
/// build number). This is the replay FLAGS word: 0x8000 = multiplayer replay,
/// 0x0000 = single-player. Game.dll refuses a multiplayer-shaped replay body
/// (multiple player records + GameStartRecord) that is flagged single-player and
/// bounces straight back to the main menu with no loading screen. A real ICCup
/// DotA replay (build 6059) carries 0xC000 here and plays; a bootstrap written
/// with flags=0 does not. (An earlier theory held that the client validated the
/// whole 0x38 dword as "build <= 6059" and required flags=0 - that is false: the
/// working 0xC000 replay yields dword 0xC00017AB and loads fine.) Match that
/// real replay exactly: 0x8000 alone made the client accept the file but then
/// crash (0xC0000005) mid-load; the full 0xC000 (the extra 0x4000 bit a genuine
/// ICCup DotA replay carries) is what loads and plays.
const FLAGS_MULTIPLAYER: u16 = 0xC000;
// Offset 0x28 is NOT a version id - it is the size of the decompressed body.
// Game.dll's loader (Game.dll+0x535050) reads it as `declared`, sums the block
// headers, and refuses the file with error 16 unless
//     sum(compressed) - last_compressed  <=  declared  <=  sum(uncompressed)
// A vanilla 1.26a LastReplay.w3g happens to carry 6029 there because that is
// its own body length, which is what made the value look like a constant.

pub struct W3gWriter {
    war3_version: u32,
    build: u16,
    tft: bool,
    replay_length_ms: u32,
}

impl W3gWriter {
    pub fn new(war3_version: u32, build: u16, tft: bool) -> Self {
        Self {
            war3_version,
            build,
            tft,
            replay_length_ms: 0,
        }
    }

    pub fn set_replay_length(&mut self, ms: u32) {
        self.replay_length_ms = ms;
    }

    /// Packs an already-built replay body into a complete `.w3g` file.
    pub fn pack(&self, decompressed: &[u8]) -> Vec<u8> {
        // Every block must inflate to exactly BLOCK_SIZE, so the tail is padded.
        // GHost++ pads unconditionally, appending a whole redundant block when
        // the body is already aligned; kept as-is for byte-for-byte parity.
        let mut padded = decompressed.to_vec();
        let pad = BLOCK_SIZE - (padded.len() % BLOCK_SIZE);
        padded.resize(padded.len() + pad, 0);

        self.emit(&padded, decompressed.len())
    }

    /// Packs a body that is already a whole number of blocks, skipping the
    /// unconditional tail pad `pack` inherits from GHost++.
    ///
    /// Returns `None` for a ragged body. Live-streamed replays need this: the
    /// header records the *unpadded* length, so a padded tail leaves the engine
    /// with materialized bytes past the declared end. For a file that is only
    /// ever played to completion that is harmless, but DotaTV appends more
    /// blocks afterwards and they would land behind the padding, which the
    /// replay parser would read as records first. See
    /// `docs/REPLAY_STREAM_SPEC.md`.
    pub fn pack_chunk_aligned(&self, decompressed: &[u8]) -> Option<Vec<u8>> {
        self.pack_chunk_aligned_declaring(decompressed, decompressed.len())
    }

    /// Same as [`W3gWriter::pack_chunk_aligned`] but declares a body length shorter
    /// than the materialized blocks.
    ///
    /// Live streaming zero-fills the tail of the last block to reach the block size the
    /// engine requires. That filler is not records, so the header must declare only the
    /// real length, otherwise the replay parser reads filler as record id 0x00.
    pub fn pack_chunk_aligned_declaring(
        &self,
        decompressed: &[u8],
        declared_len: usize,
    ) -> Option<Vec<u8>> {
        if !decompressed.len().is_multiple_of(BLOCK_SIZE) {
            return None;
        }
        if declared_len > decompressed.len() {
            return None;
        }
        Some(self.emit(decompressed, declared_len))
    }

    /// `padded` must be a whole number of blocks; `declared_len` is the
    /// unpadded length written to the header.
    fn emit(&self, padded: &[u8], declared_len: usize) -> Vec<u8> {
        let mut blocks: Vec<Vec<u8>> = Vec::with_capacity(padded.len() / BLOCK_SIZE);
        for chunk in padded.chunks_exact(BLOCK_SIZE) {
            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(chunk)
                .expect("zlib encode into Vec cannot fail");
            blocks.push(enc.finish().expect("zlib finish into Vec cannot fail"));
        }

        let compressed_total: usize = blocks.iter().map(|b| b.len()).sum();
        let file_size = HEADER_SIZE as usize + compressed_total + blocks.len() * 8;

        let mut header = Vec::with_capacity(HEADER_SIZE as usize);
        header.extend_from_slice(b"Warcraft III recorded game\x1A\0");
        header.extend_from_slice(&HEADER_SIZE.to_le_bytes());
        header.extend_from_slice(&(file_size as u32).to_le_bytes());
        header.extend_from_slice(&1u32.to_le_bytes()); // header version
        header.extend_from_slice(&(declared_len as u32).to_le_bytes());
        header.extend_from_slice(&(blocks.len() as u32).to_le_bytes());
        // "W3XP"/"WAR3" stored reversed on the wire (packed.cpp:326-336).
        header.extend_from_slice(if self.tft { b"PX3W" } else { b"3RAW" });
        header.extend_from_slice(&self.war3_version.to_le_bytes());
        header.extend_from_slice(&self.build.to_le_bytes());
        header.extend_from_slice(&FLAGS_MULTIPLAYER.to_le_bytes());
        header.extend_from_slice(&self.replay_length_ms.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes()); // CRC placeholder
        // Header is always HEADER_SIZE bytes: 28 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 + 4 = 68.

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
            let crc1 = {
                let c = crc32fast::hash(&bh);
                c ^ (c >> 16)
            };
            let crc2 = {
                let c = crc32fast::hash(block);
                c ^ (c >> 16)
            };
            let block_crc = (crc1 & 0xFFFF) | (crc2 << 16);
            bh[4..8].copy_from_slice(&block_crc.to_le_bytes());

            out.extend_from_slice(&bh);
            out.extend_from_slice(block);
        }
        // File size is HEADER_SIZE + sum of all block compressed sizes + 8 bytes per block.
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::ZlibDecoder;
    use std::io::Read;

    fn read_u32(d: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
    }

    #[test]
    fn the_header_carries_the_flags_length_and_a_self_consistent_crc() {
        let mut w = W3gWriter::new(26, 6059, true);
        w.set_replay_length(123_456);
        let out = w.pack(&[0xABu8; 100]);

        assert_eq!(&out[..28], b"Warcraft III recorded game\x1A\0");
        assert_eq!(read_u32(&out, 28), 68, "header size");
        assert_eq!(read_u32(&out, 36), 1, "header version");
        assert_eq!(&out[48..52], b"PX3W", "W3XP, little-endian on the wire");
        assert_eq!(read_u32(&out, 52), 26, "war3 version");
        assert_eq!(u16::from_le_bytes([out[56], out[57]]), 6059, "build");
        assert_eq!(
            u16::from_le_bytes([out[58], out[59]]),
            0xC000,
            "header flags must match a real ICCup replay (0xC000) or the client bounces/crashes"
        );
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
        assert_eq!(
            read_u32(&out, 40),
            body.len() as u32,
            "0x28 carries the unpadded body length, which is what the client's \
             loader bounds-checks against the block table"
        );

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
        assert_eq!(
            read_u32(&out, 32) as usize,
            out.len(),
            "field 32 is the whole file size"
        );
    }

    #[test]
    fn the_block_crc_folds_the_header_and_data_checksums() {
        let w = W3gWriter::new(26, 6059, true);
        let out = w.pack(&[1u8; 10]);
        let c_len = u16::from_le_bytes([out[68], out[69]]) as usize;

        let mut bh = out[68..76].to_vec();
        bh[4..8].copy_from_slice(&0u32.to_le_bytes());
        let crc1 = {
            let c = crc32fast::hash(&bh);
            c ^ (c >> 16)
        };
        let crc2 = {
            let c = crc32fast::hash(&out[76..76 + c_len]);
            c ^ (c >> 16)
        };
        let expected = (crc1 & 0xFFFF) | (crc2 << 16);

        assert_eq!(
            u32::from_le_bytes([out[72], out[73], out[74], out[75]]),
            expected
        );
    }

    #[test]
    fn the_decompressed_size_field_enables_trim_and_round_trip() {
        // Test with non-block-aligned input to ensure padding logic is correct.
        let body = vec![0x42u8; 8192 * 2 + 7];
        let w = W3gWriter::new(26, 6059, true);
        let out = w.pack(&body);

        let block_count = read_u32(&out, 44) as usize;

        // The header no longer stores the body length (that dword is the
        // game version id), so the round trip asserts the prefix instead:
        // every original byte must survive in order, followed by padding.
        let mut decompressed = Vec::new();
        let mut pos = 68;
        for _ in 0..block_count {
            let c_len = u16::from_le_bytes([out[pos], out[pos + 1]]) as usize;
            let comp = &out[pos + 8..pos + 8 + c_len];
            ZlibDecoder::new(comp)
                .read_to_end(&mut decompressed)
                .expect("block must be valid zlib");
            pos += 8 + c_len;
        }

        assert!(
            decompressed.len() >= body.len(),
            "padded stream must cover the whole body"
        );
        assert!(
            decompressed[..body.len()] == body[..],
            "decompressed stream must start with the original body"
        );
    }
}
