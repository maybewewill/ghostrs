use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::ZlibEncoder;

use crate::w3g::W3gWriter;

pub const CHUNK_SIZE: usize = 8192;
pub const GREETING: [u8; 4] = *b"DTV1";
const MAX_FRAME: usize = CHUNK_SIZE;
const EMPTY_TIMESLOT: [u8; 5] = [0x1F, 0x02, 0x00, 0x00, 0x00];

fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        table[i as usize] = c;
    }
    table
}

fn crc_push(mut reg: u32, data: &[u8], t: &[u32; 256]) -> u32 {
    for &b in data {
        reg = t[((reg ^ b as u32) & 0xFF) as usize] ^ (reg >> 8);
    }
    reg
}

pub fn crc32(data: &[u8]) -> u32 {
    crc_push(0xFFFF_FFFF, data, &crc_table()) ^ 0xFFFF_FFFF
}

#[derive(Debug, Clone)]
pub struct Chunk {
    pub compressed: Arc<Vec<u8>>,
    pub valid_bytes: u16,
    pub crc: u32,
}

impl Chunk {
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
    #[error("compressed chunk is {0} bytes, exceeds the {CHUNK_SIZE}-byte limit")]
    ChunkTooLarge(usize),
}

pub struct DotaTvStream {
    raw: Vec<u8>,
    raw_base: usize,
    frames: Vec<Chunk>,
    frame_times: Vec<Instant>,
    framed_len: usize,
    prologue: Vec<u8>,
    prologue_end: usize,
    war3_version: u32,
    build: u16,
    tft: bool,
    crc_reg: u32,
    crc_table: [u32; 256],
}

const RAW_TRIM_KEEP: usize = CHUNK_SIZE;

impl DotaTvStream {
    pub fn new(war3_version: u32, build: u16, tft: bool) -> Self {
        Self {
            raw: Vec::new(),
            raw_base: 0,
            frames: Vec::new(),
            frame_times: Vec::new(),
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

    pub fn mark_prologue_end(&mut self) {
        self.prologue_end = self.raw_base + self.raw.len();
        self.prologue.clear();
        self.prologue.extend_from_slice(&self.raw);
    }

    pub fn mark_prologue_end_at(&mut self, abs_offset: usize) {
        self.prologue_end = abs_offset;
        self.prologue.clear();
        let end = abs_offset.saturating_sub(self.raw_base).min(self.raw.len());
        self.prologue.extend_from_slice(&self.raw[..end]);
    }

    pub fn for_126a() -> Self {
        Self::new(26, 6059, true)
    }

    pub fn push_body(&mut self, bytes: &[u8]) -> Result<usize, DotaTvError> {
        self.raw.extend_from_slice(bytes);
        Ok(0)
    }

    pub fn flush(&mut self) -> Result<usize, DotaTvError> {
        let mut cut = 0;
        while self.framed_len < self.raw.len() {
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
            self.frame_times.push(Instant::now());
            self.crc_reg = crc_push(self.crc_reg, slice, &self.crc_table);
            self.framed_len = end;
            cut += 1;
        }

        if self.framed_len > RAW_TRIM_KEEP {
            let framed = self.framed_len;
            self.raw.drain(..framed);
            self.raw_base += framed;
            self.framed_len = 0;
        }
        Ok(cut)
    }

    pub fn pending_len(&self) -> usize {
        self.raw.len() - self.framed_len
    }

    pub fn retained_len(&self) -> usize {
        self.raw.len()
    }

    pub fn chunk_count(&self) -> usize {
        self.frames.len()
    }

    pub fn count_delayed(&self, delay: Duration) -> usize {
        if delay.is_zero() {
            return self.frames.len();
        }

        let Some(cutoff) = Instant::now().checked_sub(delay) else {
            return 0;
        };

        self.frame_times.partition_point(|t| *t <= cutoff)
    }

    pub fn status(&self, start_index: usize, delay: Duration) -> (bool, u64) {
        if delay.is_zero() || start_index >= self.frames.len() {
            return (true, 0);
        }
        if self.count_delayed(delay) > start_index {
            return (true, 0);
        }
        let age = Instant::now().saturating_duration_since(self.frame_times[start_index]);
        (false, delay.saturating_sub(age).as_secs())
    }

    pub fn chunk(&self, index: usize) -> Option<Chunk> {
        self.frames.get(index).cloned()
    }

    pub fn published_len(&self) -> usize {
        self.raw_base + self.framed_len
    }

    pub fn published_crc(&self) -> u32 {
        self.crc_reg ^ 0xFFFF_FFFF
    }

    pub fn bootstrap(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        debug_assert_eq!(
            self.prologue.len(),
            self.prologue_end,
            "prologue snapshot must cover body[0..prologue_end]; \
             mark_prologue_end must run before any trim could drop prologue bytes"
        );
        let prefix = self.prologue_end.min(self.published_len());
        let mut writer = W3gWriter::new(self.war3_version, self.build, self.tft);
        writer.set_replay_length(replay_length_ms);

        let aligned = prefix.div_ceil(CHUNK_SIZE) * CHUNK_SIZE;
        let mut padded = self.prologue[..prefix.min(self.prologue.len())].to_vec();
        while padded.len() + EMPTY_TIMESLOT.len() <= aligned {
            padded.extend_from_slice(&EMPTY_TIMESLOT);
        }
        padded.resize(aligned, 0);

        let file = writer
            .pack_chunk_aligned_declaring(&padded, aligned)
            .expect("padded is block-aligned by construction");

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

    pub fn bootstrap_full(&self, replay_length_ms: u32) -> (Vec<u8>, u32) {
        use flate2::read::ZlibDecoder;
        use std::io::Read as _;

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

        s.push_body(&[0x11; 100]).unwrap();
        assert_eq!(s.chunk_count(), 0, "push alone must not publish");
        assert_eq!(s.pending_len(), 100);
        assert_eq!(s.flush().unwrap(), 1);
        assert_eq!(s.chunk_count(), 1);
        assert_eq!(s.published_len(), 100);
        assert_eq!(s.pending_len(), 0);
    }

    #[test]
    fn count_delayed_withholds_frames_until_they_age_past_the_delay() {
        let mut s = DotaTvStream::for_126a();
        s.push_body(&[0x1F, 0x02, 0x00, 0x64, 0x00]).unwrap();
        s.flush().unwrap();
        assert_eq!(s.chunk_count(), 1);
        assert_eq!(s.count_delayed(Duration::ZERO), 1);
        assert_eq!(s.count_delayed(Duration::from_secs(60)), 0);

        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(s.count_delayed(Duration::from_millis(20)), 1);
    }

    #[test]
    fn count_delayed_boundary_is_monotonic_across_frames() {
        let mut s = DotaTvStream::for_126a();

        s.push_body(&[0x1F, 0x02, 0x00, 0x64, 0x00]).unwrap();
        s.flush().unwrap();
        std::thread::sleep(Duration::from_millis(40));

        s.push_body(&[0x1F, 0x02, 0x00, 0x64, 0x00]).unwrap();
        s.flush().unwrap();
        assert_eq!(s.chunk_count(), 2);
        assert_eq!(s.count_delayed(Duration::from_millis(20)), 1);

        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(s.count_delayed(Duration::from_millis(20)), 2);
    }

    #[test]
    fn a_small_flush_publishes_a_small_frame_with_no_filler() {
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
            assert_eq!(
                inflate(&c.compressed).len(),
                c.valid_bytes as usize,
                "frame {i}"
            );
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
        assert_eq!(
            rebuilt, body,
            "frames must cover the body with no gaps or filler"
        );
    }

    #[test]
    fn frames_never_cross_a_block_boundary() {
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

        for _ in 0..(CHUNK_SIZE * 2 + 500) / 5 {
            s.push_body(&[0x1F, 0x02, 0x00, 0x0A, 0x00]).unwrap();
            s.flush().unwrap();
        }

        let (file, resume) = s.bootstrap(4242);

        assert_eq!(resume, 0, "header-only bootstrap resumes at frame 0");
        let declared = read_u32(&file, 40) as usize;
        let blocks = read_u32(&file, 44) as usize;
        assert_eq!(blocks, 0, "bootstrap must carry no replay body");
        assert_eq!(declared, 0);
        assert_eq!(read_u32(&file, 52), 26, "war3 version");
        assert_eq!(u16::from_le_bytes([file[56], file[57]]), 6059, "build");
        assert_eq!(read_u32(&file, 60), 4242, "replay length ms");
    }

    #[test]
    fn a_long_match_streams_its_whole_history_without_a_hole() {
        let mut s = DotaTvStream::for_126a();
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
        let live: Vec<u8> = (0..CHUNK_SIZE).map(|i| (i % 97) as u8).collect();
        s.push_body(&live).unwrap();
        s.flush().unwrap();

        let mut resumed = Vec::new();
        for i in next_index as usize..s.chunk_count() {
            resumed.extend_from_slice(&inflate(&s.chunk(i).unwrap().compressed));
        }
        let expected = {
            let mut e = vec![0x66u8; CHUNK_SIZE * 2];
            e.extend_from_slice(&live);
            e
        };
        assert_eq!(
            resumed, expected,
            "resume must continue the byte stream exactly"
        );
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
        let mut s = DotaTvStream::for_126a();
        for block in 0..25 {
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
