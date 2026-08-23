//! DotaTV live replay stream.
//!
//! Viewers do not join the game. They are booted straight into replay playback
//! with `war3.exe -loadfile <bootstrap.w3g>` and then fed the rest of the match
//! as compressed `.w3g` data blocks, which `dotatv_client.dll` appends directly
//! into the replay stream Game.dll holds in memory.
//!
//! The protocol constraints all come from `game.dll` 1.26a and are documented in
//! `docs/REPLAY_STREAM_SPEC.md`. The two that shape this module:
//!
//! * A block is only accepted if it inflates to **exactly** [`CHUNK_SIZE`], and
//!   its compressed form must not exceed [`CHUNK_SIZE`] either.
//! * The bootstrap body must be a whole number of chunks. `W3gWriter::pack`
//!   zero-pads a ragged tail while recording the unpadded length, which would
//!   leave the engine parsing padding zeros as records before it reached the
//!   first live block.

use std::io::Write;
use std::sync::Arc;

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::w3g::W3gWriter;

/// Size of a `.w3g` data block. Only the bootstrap *file* is built out of blocks:
/// Game.dll's loader requires every block in the file to inflate to exactly this.
/// The live wire frames are not blocks and are not tied to this size.
pub const CHUNK_SIZE: usize = 8192;

/// Greeting the server sends before any frame.
pub const GREETING: [u8; 4] = *b"DTV1";

/// Largest live frame put on the wire.
///
/// Frames are cut so they never cross a [`CHUNK_SIZE`] boundary of the body, which is
/// what keeps every bootstrap split point (always a block boundary) also a frame
/// boundary, so a resuming viewer never needs a partial frame.
const MAX_FRAME: usize = CHUNK_SIZE;
/// Empty TimeSlot record: id 0x1F, zero action bytes, zero ms increment.
/// A no-op tick the 1.26a parser accepts; used as bootstrap padding so the
/// file carries valid post-start-block data without any real actions.
const EMPTY_TIMESLOT: [u8; 5] = [0x1F, 0x02, 0x00, 0x00, 0x00];

/// Wire CRC32 (IEEE, reflected, poly 0xEDB88320) over the decompressed payload
/// bytes. The injected client recomputes it after inflating and drops the
/// connection if it disagrees — a corrupted or truncated stream must never
/// reach Game.dll's replay parser, where a torn record means a desync or crash.
fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    table
}

/// Pushes `data` through an un-finalized CRC register.
fn crc_push(mut reg: u32, data: &[u8], t: &[u32; 256]) -> u32 {
    for &b in data {
        reg = t[((reg ^ b as u32) & 0xFF) as usize] ^ (reg >> 8);
    }
    reg
}

/// CRC32 of `data` (finalized).
pub fn crc32(data: &[u8]) -> u32 {
    crc_push(0xFFFF_FFFF, data, &crc_table()) ^ 0xFFFF_FFFF
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub compressed: Arc<Vec<u8>>,
    /// Decompressed size of this frame. Every byte is a real record: frames never
    /// carry filler, so the client appends the whole payload.
    pub valid_bytes: u16,
    /// CRC32 over the decompressed payload (`valid_bytes` bytes).
    pub crc: u32,
}

impl Chunk {
    /// Wire frame: `u16 compressedSize, u16 validBytes, u32 crc32, u8 data[]`, little endian.
    /// The CRC covers the decompressed payload; the client verifies it after
    /// inflating and disconnects on mismatch instead of feeding Game.dll a
    /// torn record stream.
    pub fn frame(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + self.compressed.len());
        out.extend_from_slice(&(self.compressed.len() as u16).to_le_bytes());
        out.extend_from_slice(&self.valid_bytes.to_le_bytes());
        out.extend_from_slice(&self.crc.to_le_bytes());
        out.extend_from_slice(&self.compressed);
        out
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DotaTvError {
    /// zlib expanded a chunk past what the append guard accepts. Only reachable
    /// with incompressible data, which action streams are not, but a rejected
    /// block would tear a hole in the stream so it is surfaced rather than
    /// silently dropped.
    #[error("compressed chunk is {0} bytes, exceeds the {CHUNK_SIZE}-byte limit")]
    ChunkTooLarge(usize),
}

/// Append-only decompressed body plus the compressed frames cut from it.
///
/// `raw` is a rolling window over the record stream: framed bytes are trimmed
/// from its front as soon as they are cut into [`Chunk`]s, because the frames
/// alone carry everything a viewer needs (the bootstrap is built from the
/// separately retained prologue copy). This keeps resident memory proportional
/// to the compressed history plus one unframed tail, not the whole decompressed
/// match.
///
/// Frames cover `raw[..framed_len]` with no gaps and no filler, and every frame
/// boundary that matters (a [`CHUNK_SIZE`] multiple, in absolute body
/// coordinates) is also a bootstrap split point, so a viewer that loads a
/// bootstrap covering N bytes resumes exactly at the frame starting at N.
pub struct DotaTvStream {
    /// Rolling window over the body. `raw[0]` sits at absolute offset
    /// [`Self::raw_base`](Self::raw_base); framed bytes are drained from the
    /// front by [`Self::flush`].
    raw: Vec<u8>,
    /// Absolute body offset of `raw[0]`.
    raw_base: usize,
    frames: Vec<Chunk>,
    /// Bytes of `raw` already cut into frames.
    framed_len: usize,
    /// Copy of the body prefix that belongs to the prologue (player records,
    /// game name, stat string, slots, start blocks), taken at
    /// [`Self::mark_prologue_end`]. The bootstrap is built exclusively from
    /// this copy, which is what lets `raw` be trimmed freely afterwards.
    /// Action timeslots crash the 1.26a parser behind the loading screen.
    prologue: Vec<u8>,
    /// Absolute body offset just past the last prologue byte.
    prologue_end: usize,
    war3_version: u32,
    build: u16,
    tft: bool,
    /// Un-finalized CRC32 register over every published body byte
    /// (`raw_base + framed_len` bytes). The heartbeat marker carries its
    /// finalized value so a viewer can verify the stream end to end.
    crc_reg: u32,
    crc_table: [u32; 256],
}

/// Framed bytes retained ahead of the unframed tail before a flush trims them.
/// Only affects memory; framing itself uses absolute coordinates.
const RAW_TRIM_KEEP: usize = CHUNK_SIZE;

impl DotaTvStream {
    pub fn new(war3_version: u32, build: u16, tft: bool) -> Self {
        Self {
            raw: Vec::new(),
            raw_base: 0,
            frames: Vec::new(),
            framed_len: 0,
            prologue: Vec::new(),
            prologue_end: 0,
            war3_version,
            build,
            tft,
            crc_reg: 0xFFFF_FFFF,
            crc_table: crc_table(),
        }
    }

    /// Marks the current end of `raw` as the prologue boundary and snapshots it.
    /// Called once, right after the prologue bytes are pushed and before any
    /// timeslot data. The snapshot is what makes later trimming safe.
    pub fn mark_prologue_end(&mut self) {
        self.prologue_end = self.raw_base + self.raw.len();
        self.prologue.clear();
        self.prologue.extend_from_slice(&self.raw);
    }

    /// Marks an absolute body offset as the prologue boundary (raw-replay mode:
    /// the whole body is pushed at once, so the boundary sits mid-buffer).
    pub fn mark_prologue_end_at(&mut self, abs_offset: usize) {
        self.prologue_end = abs_offset;
        self.prologue.clear();
        let end = abs_offset.saturating_sub(self.raw_base).min(self.raw.len());
        self.prologue.extend_from_slice(&self.raw[..end]);
    }

    /// Stream for Warcraft III 1.26a. The build number is the one carried by the
    /// `.w3g` format and checked at `0x6F5A42EA`, which is 6059 — not the 6401
    /// in the `game.dll` file version.
    pub fn for_126a() -> Self {
        Self::new(26, 6059, true)
    }

    /// Appends decompressed body bytes. Nothing is published until [`Self::flush`].
    pub fn push_body(&mut self, bytes: &[u8]) -> Result<usize, DotaTvError> {
        self.raw.extend_from_slice(bytes);
        Ok(0)
    }

    /// Publishes everything buffered so far as one or more frames.
    ///
    /// Frames carry only real records — no filler — so the viewer's engine never
    /// executes synthetic turns. That is what keeps playback smooth and keeps the match
    /// clock driven purely by the host's own TimeSlot records.
    pub fn flush(&mut self) -> Result<usize, DotaTvError> {
        let mut cut = 0;
        while self.framed_len < self.raw.len() {
            // Frame boundaries are tracked in absolute body coordinates so the
            // block-alignment guarantee survives front-trimming.
            let start_abs = self.raw_base + self.framed_len;
            let block_end = (start_abs / CHUNK_SIZE + 1) * CHUNK_SIZE;
            let end_abs = (self.raw_base + self.raw.len())
                .min(block_end)
                .min(start_abs + MAX_FRAME);
            let end = end_abs - self.raw_base;
            let slice = &self.raw[self.framed_len..end];

            let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
            enc.write_all(slice)
                .expect("zlib encode into Vec cannot fail");
            let compressed = enc.finish().expect("zlib finish into Vec cannot fail");

            if compressed.len() > CHUNK_SIZE {
                return Err(DotaTvError::ChunkTooLarge(compressed.len()));
            }

            self.frames.push(Chunk {
                compressed: Arc::new(compressed),
                valid_bytes: slice.len() as u16,
                crc: crc32(slice),
            });
            self.crc_reg = crc_push(self.crc_reg, slice, &self.crc_table);
            self.framed_len = end;
            cut += 1;
        }

        // Drop framed bytes from the window. The frames own that data now, and
        // the prologue snapshot owns the bootstrap prefix, so nothing is lost.
        // The tail (< CHUNK_SIZE after a full drain) stays for the next flush.
        if self.framed_len > RAW_TRIM_KEEP {
            let framed = self.framed_len;
            self.raw.drain(..framed);
            self.raw_base += framed;
            self.framed_len = 0;
        }
        Ok(cut)
    }

    /// Buffered body bytes not yet published as frames.
    pub fn pending_len(&self) -> usize {
        self.raw.len() - self.framed_len
    }

    /// Body bytes still held in memory outside the compressed frames: the
    /// unframed tail plus whatever has not crossed the trim threshold yet.
    pub fn retained_len(&self) -> usize {
        self.raw.len()
    }

    pub fn chunk_count(&self) -> usize {
        self.frames.len()
    }

    pub fn chunk(&self, index: usize) -> Option<Chunk> {
        self.frames.get(index).cloned()
    }

    /// Body bytes covered by published frames.
    pub fn published_len(&self) -> usize {
        self.raw_base + self.framed_len
    }

    /// Finalized CRC32 over every published body byte. Matches what a viewer
    /// that received every frame from 0 computes incrementally.
    pub fn published_crc(&self) -> u32 {
        self.crc_reg ^ 0xFFFF_FFFF
    }

    /// Builds a `.w3g` carrying the prologue plus empty-timeslot padding, and
    /// returns the frame index a viewer loading it must resume from.
    ///
    /// War3 needs the prologue to know which map to load — without it the engine
    /// cannot set up the game at all. It also needs at least one TimeSlot record
    /// after the start blocks: a replay that ends right on the third start block
    /// is rejected with NETERROR_CANTLOADREPLAYDATA. But real DotA 507 action
    /// data crashes the 1.26a parser behind the loading screen, so the padding
    /// uses empty timeslots (record 0x1F, zero actions, zero increment) — valid
    /// no-op ticks that carry no actions for the parser to choke on.
    ///
    /// The declared body length covers the whole padded block, matching the
    /// bootstrap layout that is known to load. Every real timeslot arrives over
    /// the live chunk stream; the injected client resumes at the first frame
    /// past the prologue boundary.
    pub fn bootstrap(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        // Built from the prologue snapshot, not `raw`: by the time a late
        // viewer asks, the prologue bytes have long been trimmed from the
        // rolling window.
        debug_assert_eq!(
            self.prologue.len(),
            self.prologue_end,
            "prologue snapshot must cover body[0..prologue_end]; \
             mark_prologue_end must run before any trim could drop prologue bytes"
        );
        let prefix = self.prologue_end.min(self.published_len());

        let mut writer = W3gWriter::new(self.war3_version, self.build, self.tft);
        writer.set_replay_length(replay_length_ms);

        // The prologue is typically 200-500 bytes — less than one CHUNK_SIZE.
        // The .w3g format requires whole blocks, so fill the rest of the block
        // with empty timeslots and declare the full length.
        let aligned = prefix.div_ceil(CHUNK_SIZE) * CHUNK_SIZE;
        let mut padded = self.prologue[..prefix.min(self.prologue.len())].to_vec();
        while padded.len() + EMPTY_TIMESLOT.len() <= aligned {
            padded.extend_from_slice(&EMPTY_TIMESLOT);
        }
        padded.resize(aligned, 0);

        let file = writer
            .pack_chunk_aligned_declaring(&padded, aligned)
            .expect("padded is block-aligned by construction");

        // The resume index is the first frame that starts at or after `prefix`.
        // Frames never cross a block boundary, so such a frame always exists.
        let mut covered = 0usize;
        let mut resume = self.frames.len();
        for (i, f) in self.frames.iter().enumerate() {
            if covered >= prefix {
                resume = i;
                break;
            }
            covered += f.valid_bytes as usize;
        }

        (file, resume as u32)
    }

    /// Like [`Self::bootstrap`] but embeds the ENTIRE recorded body so far, not
    /// just the prologue, and resumes at the live edge.
    ///
    /// A viewer loading this seeks to match-time BEHIND its loading screen: the
    /// injected client drains the whole engine buffer before revealing the 3D
    /// world, so a spectator who joins at minute 15 sees the world appear
    /// already at 15:00 instead of watching a visible fast-forward.
    ///
    /// WARNING: real action timeslots are known to crash the 1.26a parser behind
    /// the loading screen (see [`Self::bootstrap`] and `prologue`). This path is
    /// only safe once the injected client arms the DotA map engine before it
    /// drains, so it is gated behind `MODE_BOOTSTRAP_FULL` and the plain
    /// prologue-only `bootstrap` stays the default.
    pub fn bootstrap_full(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        use flate2::read::ZlibDecoder;
        use std::io::Read as _;

        // Frames cover body[0..] with no gaps or filler, so inflating each in
        // order reconstructs the full published body verbatim.
        let mut body = Vec::new();
        for f in &self.frames {
            let mut raw = Vec::new();
            ZlibDecoder::new(&f.compressed[..])
                .read_to_end(&mut raw)
                .expect("frame is valid zlib produced by flush");
            body.extend_from_slice(&raw);
        }

        let mut writer = W3gWriter::new(self.war3_version, self.build, self.tft);
        writer.set_replay_length(replay_length_ms);

        // Pad the final partial block with empty timeslots so the declared body
        // is block-aligned, exactly like `bootstrap`.
        let aligned = body.len().div_ceil(CHUNK_SIZE) * CHUNK_SIZE;
        let mut padded = body;
        while padded.len() + EMPTY_TIMESLOT.len() <= aligned {
            padded.extend_from_slice(&EMPTY_TIMESLOT);
        }
        padded.resize(aligned, 0);

        let file = writer
            .pack_chunk_aligned_declaring(&padded, aligned)
            .expect("padded is block-aligned by construction");

        (file, self.frames.len() as u32)
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

    fn inflate(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .expect("valid zlib");
        out
    }

    #[test]
    fn nothing_is_published_until_flush() {
        let mut s = DotaTvStream::for_126a();

        s.push_body(&vec![0x11; 100]).unwrap();
        assert_eq!(s.chunk_count(), 0, "push alone must not publish");
        assert_eq!(s.pending_len(), 100);

        assert_eq!(s.flush().unwrap(), 1);
        assert_eq!(s.chunk_count(), 1);
        assert_eq!(s.published_len(), 100);
        assert_eq!(s.pending_len(), 0);
    }

    #[test]
    fn a_small_flush_publishes_a_small_frame_with_no_filler() {
        // This is the property that keeps playback smooth: a quiet tick is a handful of
        // bytes and must go out as a handful of bytes, not padded to a block.
        let mut s = DotaTvStream::for_126a();
        s.push_body(&[0x1F, 0x02, 0x00, 0x64, 0x00]).unwrap();
        s.flush().unwrap();

        let f = s.chunk(0).unwrap();
        assert_eq!(f.valid_bytes, 5);
        assert_eq!(inflate(&f.compressed), vec![0x1F, 0x02, 0x00, 0x64, 0x00]);
    }

    #[test]
    fn every_frame_stays_within_the_compressed_size_guard() {
        let mut s = DotaTvStream::for_126a();
        let body: Vec<u8> = (0..CHUNK_SIZE * 3).map(|i| (i % 251) as u8).collect();
        s.push_body(&body).unwrap();
        s.flush().unwrap();

        for i in 0..s.chunk_count() {
            let c = s.chunk(i).unwrap();
            assert!(
                c.compressed.len() <= CHUNK_SIZE,
                "frame {i} compressed to {} bytes, guard caps at {CHUNK_SIZE}",
                c.compressed.len()
            );
            assert_eq!(inflate(&c.compressed).len(), c.valid_bytes as usize, "frame {i}");
        }
    }

    #[test]
    fn frames_reassemble_into_the_original_body() {
        let mut s = DotaTvStream::for_126a();
        let body: Vec<u8> = (0..CHUNK_SIZE * 2 + 777).map(|i| (i % 199) as u8).collect();
        s.push_body(&body).unwrap();
        s.flush().unwrap();

        let mut rebuilt = Vec::new();
        for i in 0..s.chunk_count() {
            rebuilt.extend_from_slice(&inflate(&s.chunk(i).unwrap().compressed));
        }
        assert_eq!(rebuilt, body, "frames must cover the body with no gaps or filler");
    }

    #[test]
    fn frames_never_cross_a_block_boundary() {
        // Bootstraps split on block boundaries, so a resuming viewer must always find a
        // frame that starts exactly there.
        let mut s = DotaTvStream::for_126a();
        s.push_body(&vec![0x66; CHUNK_SIZE * 2 + 500]).unwrap();
        s.flush().unwrap();

        let mut offset = 0usize;
        for i in 0..s.chunk_count() {
            let len = s.chunk(i).unwrap().valid_bytes as usize;
            assert_eq!(
                offset / CHUNK_SIZE,
                (offset + len - 1) / CHUNK_SIZE,
                "frame {i} spans a block boundary"
            );
            offset += len;
        }
        assert_eq!(offset, CHUNK_SIZE * 2 + 500);
    }

    #[test]
    fn bootstrap_is_header_only_and_resumes_at_zero() {
        let mut s = DotaTvStream::for_126a();
        // Two whole blocks plus a tail, published in small pieces the way a live match
        // produces them.
        for _ in 0..(CHUNK_SIZE * 2 + 500) / 5 {
            s.push_body(&[0x1F, 0x02, 0x00, 0x0A, 0x00]).unwrap();
            s.flush().unwrap();
        }

        let (file, resume) = s.bootstrap(4242);

        // The 1.26a parser crashes on any non-trivial replay body behind the
        // loading screen, so the bootstrap must be a bare header: zero blocks,
        // zero declared body bytes.
        assert_eq!(resume, 0, "header-only bootstrap resumes at frame 0");
        let declared = read_u32(&file, 40) as usize;
        let blocks = read_u32(&file, 44) as usize;
        assert_eq!(blocks, 0, "bootstrap must carry no replay body");
        assert_eq!(declared, 0);
        assert_eq!(read_u32(&file, 52), 26, "war3 version");
        assert_eq!(u16::from_le_bytes([file[56], file[57]]), 6059, "build");
        assert_eq!(read_u32(&file, 60), 4242, "replay length ms");
    }

    /// The whole history must still reach the viewer — through the stream. The
    /// bootstrap carries nothing, so every published byte must come out of frame
    /// 0 onwards with no gap and no overlap.
    #[test]
    fn a_long_match_streams_its_whole_history_without_a_hole() {
        let mut s = DotaTvStream::for_126a();
        // 20 whole blocks plus a ragged tail: five real minutes of DotA is this order.
        let total = CHUNK_SIZE * 20 + 777;
        for i in 0..total / 5 {
            let t = (i % 251) as u8;
            s.push_body(&[0x1F, 0x02, 0x00, t, 0x00]).unwrap();
        }
        s.flush().unwrap();

        let (file, resume) = s.bootstrap(9999);
        assert_eq!(resume, 0, "streaming starts at the first frame");
        assert_eq!(read_u32(&file, 44) as usize, 0, "no body in the file");

        let streamed: usize = (0..s.chunk_count())
            .map(|i| s.chunk(i).unwrap().valid_bytes as usize)
            .sum();
        assert_eq!(
            streamed,
            s.published_len(),
            "frames must cover the whole body with no hole"
        );
    }

    #[test]
    fn a_viewer_resuming_at_the_bootstrap_index_continues_the_same_byte_stream() {
        let mut s = DotaTvStream::for_126a();
        s.push_body(&vec![0x66; CHUNK_SIZE * 2]).unwrap();
        s.flush().unwrap();
        let (_, next_index) = s.bootstrap(0);

        // Live data arriving after the viewer took its bootstrap.
        let live: Vec<u8> = (0..CHUNK_SIZE).map(|i| (i % 97) as u8).collect();
        s.push_body(&live).unwrap();
        s.flush().unwrap();

        // Resume is 0, so the viewer receives everything: the backlog first,
        // then exactly the new bytes.
        let mut resumed = Vec::new();
        for i in next_index as usize..s.chunk_count() {
            resumed.extend_from_slice(&inflate(&s.chunk(i).unwrap().compressed));
        }
        let expected = {
            let mut e = vec![0x66u8; CHUNK_SIZE * 2];
            e.extend_from_slice(&live);
            e
        };
        assert_eq!(resumed, expected, "resume must continue the byte stream exactly");
    }

    #[test]
    fn frames_carry_the_sizes_the_client_validates() {
        let mut s = DotaTvStream::for_126a();
        s.push_body(&vec![0x77; 4096]).unwrap();
        s.flush().unwrap();
        let chunk = s.chunk(0).unwrap();

        let frame = chunk.frame();
        let comp_size = u16::from_le_bytes([frame[0], frame[1]]) as usize;
        let valid = u16::from_le_bytes([frame[2], frame[3]]) as usize;
        let crc = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);

        assert_eq!(comp_size, chunk.compressed.len());
        assert_eq!(valid, 4096);
        assert_eq!(crc, chunk.crc);
        assert_eq!(frame.len(), 8 + comp_size);
        assert_eq!(&frame[8..], chunk.compressed.as_slice());
    }

    #[test]
    fn flush_keeps_the_raw_window_bounded() {
        // A multi-block match must not accumulate the whole decompressed body:
        // framed bytes are trimmed from the rolling window as frames own them.
        let mut s = DotaTvStream::for_126a();
        let mut rebuilt = Vec::new();
        for block in 0..40 {
            let body: Vec<u8> = (0..CHUNK_SIZE)
                .map(|i| ((i + block * 7) % 251) as u8)
                .collect();
            s.push_body(&body).unwrap();
            s.flush().unwrap();
            rebuilt.extend_from_slice(&body);

            assert!(
                s.retained_len() <= 2 * CHUNK_SIZE,
                "window ballooned to {} bytes after block {block}",
                s.retained_len()
            );
        }

        assert_eq!(s.published_len(), rebuilt.len());
        assert_eq!(s.pending_len(), 0);
        for i in 0..s.chunk_count() {
            let c = s.chunk(i).unwrap();
            assert_eq!(inflate(&c.compressed).len(), c.valid_bytes as usize);
        }
        let mut streamed = Vec::new();
        for i in 0..s.chunk_count() {
            streamed.extend_from_slice(&inflate(&s.chunk(i).unwrap().compressed));
        }
        assert_eq!(streamed, rebuilt, "frames must still reassemble exactly");
    }

    #[test]
    fn bootstrap_is_stable_across_raw_trimming() {
        // The prologue snapshot must survive front-trimming: a bootstrap taken
        // after hours of streamed data is byte-identical to one taken early.
        let prologue: Vec<u8> = (0..500).map(|i| (i % 13) as u8).collect();

        let mut early = DotaTvStream::for_126a();
        early.push_body(&prologue).unwrap();
        early.flush().unwrap();
        early.mark_prologue_end();
        let (want_file, want_resume) = early.bootstrap(777);

        let mut late = DotaTvStream::for_126a();
        late.push_body(&prologue).unwrap();
        late.flush().unwrap();
        late.mark_prologue_end();
        // Hours of live data, flushed in ticks so trimming runs repeatedly.
        for block in 0..40 {
            let body: Vec<u8> = (0..CHUNK_SIZE).map(|i| ((i ^ block) % 199) as u8).collect();
            late.push_body(&body).unwrap();
            late.flush().unwrap();
        }
        assert!(late.retained_len() <= 2 * CHUNK_SIZE, "trim must have run");

        let (got_file, got_resume) = late.bootstrap(777);
        assert_eq!(got_resume, want_resume);
        assert_eq!(got_file, want_file, "bootstrap changed after trimming");
    }

    #[test]
    fn frames_never_cross_a_block_boundary_even_after_front_trimming() {
        // Trimming shifts the window, but block alignment is tracked in
        // absolute body coordinates, so the guarantee must hold for the whole
        // stream, not just the untrimmed prefix.
        let mut s = DotaTvStream::for_126a();
        for block in 0..25 {
            // Ragged pieces so frame cuts land at non-trivial offsets.
            s.push_body(&vec![0x51; CHUNK_SIZE / 3 + block]).unwrap();
            s.flush().unwrap();
        }

        let mut offset = 0usize;
        for i in 0..s.chunk_count() {
            let len = s.chunk(i).unwrap().valid_bytes as usize;
            assert_eq!(
                offset / CHUNK_SIZE,
                (offset + len - 1) / CHUNK_SIZE,
                "frame {i} spans a block boundary"
            );
            offset += len;
        }
        let expected: usize = (0..25).map(|b| CHUNK_SIZE / 3 + b).sum();
        assert_eq!(offset, expected);
    }
}
