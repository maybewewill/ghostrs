const REPLAY_LEAVEGAME: u8 = 0x17;
const REPLAY_FIRSTSTARTBLOCK: u8 = 0x1A;
const REPLAY_SECONDSTARTBLOCK: u8 = 0x1B;
const REPLAY_THIRDSTARTBLOCK: u8 = 0x1C;
const REPLAY_TIMESLOT2: u8 = 0x1E;
const REPLAY_TIMESLOT: u8 = 0x1F;
const REPLAY_CHATMESSAGE: u8 = 0x20;
const LANGUAGE_ID: u32 = 0x0012_F8B0;
const REPLAY_GAME_TYPE: u32 = 0x0000_0001;

#[derive(Debug, PartialEq, Eq)]
pub enum ReplayBodyError {
    StartNeverSet,
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
    loading_blocks: Vec<Vec<u8>>,
    blocks: Vec<u8>,
    replay_length_ms: u32,
    published: usize,
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
            loading_blocks: Vec::new(),
            blocks: Vec::new(),
            replay_length_ms: 0,
            published: 0,
        }
    }

    pub fn set_host(&mut self, host_pid: u8, host_name: &str) {
        self.host_pid = host_pid;
        self.host_name = host_name.to_string();
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

    pub fn set_start(
        &mut self,
        slots: Vec<u8>,
        random_seed: u32,
        select_mode: u8,
        start_spots: u8,
    ) -> Result<(), ReplayBodyError> {
        if !slots.len().is_multiple_of(9) {
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

    pub fn add_timeslot(&mut self, time_increment: u16, actions: &[u8]) {
        self.replay_length_ms += time_increment as u32;
        self.blocks.push(REPLAY_TIMESLOT);
        let len = 2 + actions.len();
        self.blocks.extend_from_slice(&(len as u16).to_le_bytes());
        self.blocks.extend_from_slice(&time_increment.to_le_bytes());
        self.blocks.extend_from_slice(actions);
    }

    pub fn add_timeslot2(&mut self, actions: &[u8]) {
        self.blocks.push(REPLAY_TIMESLOT2);
        let len = 2 + actions.len();
        self.blocks.extend_from_slice(&(len as u16).to_le_bytes());
        self.blocks.extend_from_slice(&0u16.to_le_bytes());
        self.blocks.extend_from_slice(actions);
    }

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

    pub fn add_server_chat(&mut self, pid: u8, message: &str) {
        self.add_chat(pid, 0x20, 0, message);
    }

    pub fn add_leaver_loading(&mut self, pid: u8, reason: u32, result: u32) {
        let mut block = Vec::with_capacity(14);
        block.push(REPLAY_LEAVEGAME);
        block.extend_from_slice(&reason.to_le_bytes());
        block.push(pid);
        block.extend_from_slice(&result.to_le_bytes());
        block.extend_from_slice(&1u32.to_le_bytes());
        self.loading_blocks.push(block);
    }

    pub fn finish(self) -> Result<(Vec<u8>, u32), ReplayBodyError> {
        let mut r = self.prologue()?;
        r.extend_from_slice(&self.blocks);
        Ok((r, self.replay_length_ms))
    }

    pub fn prologue(&self) -> Result<Vec<u8>, ReplayBodyError> {
        if !self.start_set {
            return Err(ReplayBodyError::StartNeverSet);
        }
        let mut r = Vec::with_capacity(512 + self.blocks.len());

        r.extend_from_slice(&0x0000_0110u32.to_le_bytes());
        r.push(0);
        r.push(self.host_pid);
        put_cstr(&mut r, &self.host_name);
        r.push(1);
        r.push(0);
        put_cstr(&mut r, &self.game_name);
        r.push(0);
        r.extend_from_slice(&self.stat_string);
        r.push(0);
        r.extend_from_slice(&(self.num_slots as u32).to_le_bytes());

        r.extend_from_slice(&REPLAY_GAME_TYPE.to_le_bytes());
        r.extend_from_slice(&LANGUAGE_ID.to_le_bytes());

        for (pid, name) in &self.players {
            r.push(22);
            r.push(*pid);
            put_cstr(&mut r, name);
            r.push(1);
            r.push(0);
            r.extend_from_slice(&0u32.to_le_bytes());
        }

        r.push(25);
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

        for lb in &self.loading_blocks {
            r.extend_from_slice(lb);
        }

        r.push(REPLAY_THIRDSTARTBLOCK);
        r.extend_from_slice(&1u32.to_le_bytes());

        Ok(r)
    }

    pub fn drain_new_blocks(&mut self) -> Vec<u8> {
        let fresh = self.blocks[self.published..].to_vec();
        self.published = self.blocks.len();
        fresh
    }

    pub fn replay_length_ms(&self) -> u32 {
        self.replay_length_ms
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

        let start = body
            .windows(1)
            .position(|w| w[0] == 25)
            .expect("GameStartRecord");
        assert_eq!(body[start + 1..start + 3], (7u16 + 2 * 9).to_le_bytes());
        let tail = &body[body.len() - 15..];
        assert_eq!(
            tail,
            &[0x1A, 1, 0, 0, 0, 0x1B, 1, 0, 0, 0, 0x1C, 1, 0, 0, 0]
        );
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

        let at = body
            .windows(5)
            .position(|w| w[0] == 0x1F && u16::from_le_bytes([w[3], w[4]]) == 100)
            .expect("timeslot block");
        let len = u16::from_le_bytes([body[at + 1], body[at + 2]]) as usize;
        assert_eq!(len, 2 + 2, "length counts time increment plus actions");
    }

    #[test]
    fn a_timeslot2_block_uses_record_0x1e() {
        let mut b = ReplayBody::new(1, "h");
        b.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        b.add_timeslot2(&[0xCC, 0xDD]);
        let (body, _) = b.finish().unwrap();

        let at = body
            .windows(5)
            .position(|w| w[0] == 0x1E && u16::from_le_bytes([w[3], w[4]]) == 0)
            .expect("timeslot2 block");
        let len = u16::from_le_bytes([body[at + 1], body[at + 2]]) as usize;
        assert_eq!(len, 2 + 2, "length counts time increment 0 plus actions");
    }

    #[test]
    fn loading_leavers_are_placed_between_start_blocks_two_and_three() {
        let mut b = ReplayBody::new(1, "h");
        b.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        b.add_leaver_loading(2, 13, 0);
        let (body, _) = b.finish().unwrap();

        let block2_idx = body
            .windows(5)
            .position(|w| w == [0x1B, 1, 0, 0, 0])
            .expect("block 0x1B");
        let block3_idx = body
            .windows(5)
            .position(|w| w == [0x1C, 1, 0, 0, 0])
            .expect("block 0x1C");

        assert!(block3_idx > block2_idx + 5);
        let leaver_block = &body[block2_idx + 5..block3_idx];
        assert_eq!(leaver_block[0], 0x17);
        assert_eq!(leaver_block[5], 2);
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
