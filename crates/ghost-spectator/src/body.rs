//! The decompressed replay body. Mirrors GHost++ `CReplay::BuildReplay`
//! (ref/ghostpp/ghost/replay.cpp:135-212) and `CReplay::AddTimeSlot`/`AddChatMessage`.

const REPLAY_FIRSTSTARTBLOCK: u8 = 0x1A;
const REPLAY_SECONDSTARTBLOCK: u8 = 0x1B;
const REPLAY_THIRDSTARTBLOCK: u8 = 0x1C;
const REPLAY_TIMESLOTBLOCK: u8 = 0x1E;
const REPLAY_CHATMESSAGE: u8 = 0x20;
const REPLAY_LEAVEGAME: u8 = 0x17;
/// GHost++ hardcodes this language id (replay.cpp:143).
const LANGUAGE_ID: u32 = 0x0012_F8B0;

/// Errors from building a [`ReplayBody`]. Both variants exist to make an
/// invalid `.w3g` body impossible to produce silently: a body finished
/// without slot data, or slot data that doesn't decode to a whole number of
/// 9-byte slot records, would otherwise corrupt every field that follows the
/// `GameStartRecord` in the byte stream without any error at build time.
#[derive(Debug, PartialEq, Eq)]
pub enum ReplayBodyError {
    /// `finish()` was called without a prior successful `set_start()`.
    StartNeverSet,
    /// `set_start()` was given a `slots` buffer whose length is not a
    /// multiple of 9 (the wire size of one W3GS_SLOTINFO slot record).
    InvalidSlotsLength(usize),
}

impl std::fmt::Display for ReplayBodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReplayBodyError::StartNeverSet => {
                write!(f, "ReplayBody::finish called before set_start")
            }
            ReplayBodyError::InvalidSlotsLength(len) => {
                write!(f, "slots buffer length {len} is not a multiple of 9")
            }
        }
    }
}

impl std::error::Error for ReplayBodyError {}

pub struct ReplayBody {
    host_pid: u8,
    host_name: String,
    players: Vec<(u8, String)>,
    slots: Vec<u8>,
    random_seed: u32,
    select_mode: u8,
    start_spots: u8,
    num_slots: usize,
    start_set: bool,
    game_name: String,
    stat_string: Vec<u8>,
    map_game_type: u32,
    blocks: Vec<u8>,
    replay_length_ms: u32,
}

fn put_cstr(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

impl ReplayBody {
    pub fn new(host_pid: u8, host_name: &str) -> Self {
        Self {
            host_pid,
            host_name: host_name.to_string(),
            players: Vec::new(),
            slots: Vec::new(),
            random_seed: 0,
            select_mode: 0,
            start_spots: 0,
            num_slots: 0,
            start_set: false,
            game_name: String::new(),
            stat_string: Vec::new(),
            map_game_type: 0,
            blocks: Vec::new(),
            replay_length_ms: 0,
        }
    }

    pub fn set_game(&mut self, game_name: &str, stat_string: &[u8], map_game_type: u32) {
        self.game_name = game_name.to_string();
        self.stat_string = stat_string.to_vec();
        self.map_game_type = map_game_type;
    }

    pub fn add_player(&mut self, pid: u8, name: &str) {
        if pid != self.host_pid {
            self.players.push((pid, name.to_string()));
        }
    }

    /// `slots` is the raw 9-bytes-per-slot wire form used by W3GS_SLOTINFO.
    /// Rejects a `slots` buffer that isn't a whole number of 9-byte records:
    /// `finish()` writes the GameStartRecord `Size` field as `7 + num_slots *
    /// 9` but writes the *entire* `slots` buffer, so a stray remainder would
    /// silently undercount `Size` relative to what's actually on the wire and
    /// desync every field after it.
    pub fn set_start(
        &mut self,
        slots: Vec<u8>,
        random_seed: u32,
        select_mode: u8,
        start_spots: u8,
    ) -> Result<(), ReplayBodyError> {
        if slots.len() % 9 != 0 {
            return Err(ReplayBodyError::InvalidSlotsLength(slots.len()));
        }
        self.num_slots = slots.len() / 9;
        self.slots = slots;
        self.random_seed = random_seed;
        self.select_mode = select_mode;
        self.start_spots = start_spots;
        self.start_set = true;
        Ok(())
    }

    /// One 100 ms action packet. `actions` is the payload of W3GS_INCOMING_ACTION
    /// *after* the send-interval field, i.e. the CRC and action blocks.
    ///
    /// Length field matches `CReplay::AddTimeSlot` (replay.cpp:87-110): the C++
    /// block is `[RecordID][u16 len placeholder][u16 timeIncrement][actions...]`
    /// and the length written back is `Block.size() - 3`, i.e. it counts the
    /// time increment plus the action bytes but not the RecordID or itself.
    pub fn add_timeslot(&mut self, time_increment: u16, actions: &[u8]) {
        self.replay_length_ms += time_increment as u32;
        self.blocks.push(REPLAY_TIMESLOTBLOCK);
        let len = 2 + actions.len();
        self.blocks.extend_from_slice(&(len as u16).to_le_bytes());
        self.blocks.extend_from_slice(&time_increment.to_le_bytes());
        self.blocks.extend_from_slice(actions);
    }

    /// Length field matches `CReplay::AddChatMessage` (replay.cpp:112-128): the
    /// C++ block is `[RecordID][PID][u16 len placeholder][flags][u32 chatMode][message + NUL]`
    /// and the length written back is `Block.size() - 4`, i.e. flags + chatMode
    /// + message bytes including the string's own null terminator (GHost++'s
    /// `UTIL_AppendByteArrayFast` appends one by default), but not the RecordID,
    /// PID, or the length field itself.
    pub fn add_chat(&mut self, pid: u8, flag: u8, extra: u32, message: &str) {
        self.blocks.push(REPLAY_CHATMESSAGE);
        self.blocks.push(pid);
        let len = 1 + 4 + message.len() + 1;
        self.blocks.extend_from_slice(&(len as u16).to_le_bytes());
        self.blocks.push(flag);
        self.blocks.extend_from_slice(&extra.to_le_bytes());
        put_cstr(&mut self.blocks, message);
    }

    pub fn add_leaver(&mut self, pid: u8, reason: u32, result: u32) {
        self.blocks.push(REPLAY_LEAVEGAME);
        self.blocks.extend_from_slice(&reason.to_le_bytes());
        self.blocks.push(pid);
        self.blocks.extend_from_slice(&result.to_le_bytes());
        self.blocks.extend_from_slice(&1u32.to_le_bytes());
    }

    /// Returns the decompressed body and the total replay length in ms.
    ///
    /// Errors rather than emitting a syntactically well-formed but corrupt
    /// `GameStartRecord` if `set_start()` was never called successfully —
    /// see [`ReplayBodyError::StartNeverSet`].
    pub fn finish(self) -> Result<(Vec<u8>, u32), ReplayBodyError> {
        if !self.start_set {
            return Err(ReplayBodyError::StartNeverSet);
        }
        let mut r = Vec::with_capacity(512 + self.blocks.len());
        r.extend_from_slice(&[16, 1, 0, 0]); // unknown (4.0)
        r.push(0); // host RecordID
        r.push(self.host_pid);
        put_cstr(&mut r, &self.host_name);
        r.push(1); // host AdditionalSize
        r.push(0); // host AdditionalData
        put_cstr(&mut r, &self.game_name);
        r.push(0); // null (4.0)
        r.extend_from_slice(&self.stat_string);
        // replay.cpp:157 appends the stat string via `UTIL_AppendByteArrayFast`
        // with its default `terminator = true`, so a NUL follows the stat
        // string itself (in addition to the "null (4.0)" byte above, which is
        // a separate field). `UTIL_EncodeStatString` never emits a zero byte,
        // so this terminator is what lets a reader find the field's end.
        r.push(0); // stat string terminator
        r.extend_from_slice(&(self.num_slots as u32).to_le_bytes());
        r.extend_from_slice(&self.map_game_type.to_le_bytes());
        r.extend_from_slice(&LANGUAGE_ID.to_le_bytes());

        for (pid, name) in &self.players {
            r.push(22); // player RecordID
            r.push(*pid);
            put_cstr(&mut r, name);
            r.push(1);
            r.push(0);
            r.extend_from_slice(&0u32.to_le_bytes());
        }

        r.push(25); // GameStartRecord
        r.extend_from_slice(&((7 + self.num_slots * 9) as u16).to_le_bytes());
        r.push(self.num_slots as u8);
        r.extend_from_slice(&self.slots);
        r.extend_from_slice(&self.random_seed.to_le_bytes());
        r.push(self.select_mode);
        r.push(self.start_spots);

        r.push(REPLAY_FIRSTSTARTBLOCK);
        r.extend_from_slice(&1u32.to_le_bytes());
        r.push(REPLAY_SECONDSTARTBLOCK);
        r.extend_from_slice(&1u32.to_le_bytes());
        r.push(REPLAY_THIRDSTARTBLOCK);
        r.extend_from_slice(&1u32.to_le_bytes());

        r.extend_from_slice(&self.blocks);
        let len = self.replay_length_ms;
        Ok((r, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_body_opens_with_the_host_record_and_the_three_start_blocks() {
        let mut b = ReplayBody::new(1, "iCCup");
        b.add_player(2, "alice");
        b.set_start(vec![0u8; 9 * 2], 0xDEADBEEF, 0, 2).unwrap();
        let (body, len_ms) = b.finish().unwrap();

        assert_eq!(&body[0..4], &[16, 1, 0, 0], "unknown 4.0");
        assert_eq!(body[4], 0, "host RecordID");
        assert_eq!(body[5], 1, "host PID");
        assert_eq!(&body[6..12], b"iCCup\0", "host name, null terminated");
        assert_eq!(len_ms, 0);

        // RecordID 25 introduces the GameStartRecord, then 0x1A/0x1B/0x1C.
        let start = body.windows(1).position(|w| w[0] == 25).expect("GameStartRecord");
        assert_eq!(body[start + 1..start + 3], (7u16 + 2 * 9).to_le_bytes());
        let tail = &body[body.len() - 15..];
        assert_eq!(tail, &[0x1A, 1, 0, 0, 0, 0x1B, 1, 0, 0, 0, 0x1C, 1, 0, 0, 0]);
    }

    #[test]
    fn timeslots_accumulate_the_replay_length() {
        let mut b = ReplayBody::new(1, "h");
        b.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        b.add_timeslot(100, &[0xAA]);
        b.add_timeslot(150, &[0xBB]);
        let (_body, len_ms) = b.finish().unwrap();
        assert_eq!(len_ms, 250, "replay length is the sum of time increments");
    }

    #[test]
    fn a_timeslot_block_is_length_prefixed_after_its_first_four_bytes() {
        let mut b = ReplayBody::new(1, "h");
        b.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        b.add_timeslot(100, &[0xAA, 0xBB]);
        let (body, _) = b.finish().unwrap();

        // Locate the 0x1E block: [0x1E][u16 len][u16 time][actions...]
        let at = body.windows(5)
            .position(|w| w[0] == 0x1E && u16::from_le_bytes([w[3], w[4]]) == 100)
            .expect("timeslot block");
        let len = u16::from_le_bytes([body[at + 1], body[at + 2]]) as usize;
        assert_eq!(len, 2 + 2, "length counts time increment plus actions");
    }

    #[test]
    fn finishing_without_set_start_is_an_error_not_silent_corruption() {
        let b = ReplayBody::new(1, "h");
        let err = b.finish().unwrap_err();
        assert_eq!(err, ReplayBodyError::StartNeverSet);
    }

    #[test]
    fn set_start_rejects_a_slots_buffer_that_is_not_a_multiple_of_nine_bytes() {
        let mut b = ReplayBody::new(1, "h");
        let err = b.set_start(vec![0u8; 10], 1, 0, 1).unwrap_err();
        assert_eq!(err, ReplayBodyError::InvalidSlotsLength(10));
    }
}
