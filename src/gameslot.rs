pub const SLOTSTATUS_OPEN: u8 = 0;
pub const SLOTSTATUS_CLOSED: u8 = 1;
pub const SLOTSTATUS_OCCUPIED: u8 = 2;

pub const SLOTRACE_HUMAN: u8 = 1;
pub const SLOTRACE_ORC: u8 = 2;
pub const SLOTRACE_NIGHTELF: u8 = 4;
pub const SLOTRACE_UNDEAD: u8 = 8;
pub const SLOTRACE_RANDOM: u8 = 32;
pub const SLOTRACE_SELECTABLE: u8 = 64;

pub const SLOTCOMP_EASY: u8 = 0;
pub const SLOTCOMP_NORMAL: u8 = 1;
pub const SLOTCOMP_HARD: u8 = 2;

#[derive(Debug, Clone)]
pub struct GameSlot {
    pid: u8,
    download_status: u8,
    slot_status: u8,
    computer: u8,
    team: u8,
    colour: u8,
    race: u8,
    computer_type: u8,
    handicap: u8,
}

impl GameSlot {
    // Конструктор из среза байт (аналог BYTEARRAY &n)
    pub fn from_bytes(n: &[u8]) -> Self {
        let pid = if n.len() >= 1 { n[0] } else { 0 };
        let download_status = if n.len() >= 2 { n[1] } else { 255 };
        let slot_status = if n.len() >= 3 { n[2] } else { SLOTSTATUS_OPEN };
        let computer = if n.len() >= 4 { n[3] } else { 0 };
        let team = if n.len() >= 5 { n[4] } else { 0 };
        let colour = if n.len() >= 6 { n[5] } else { 1 };
        let race = if n.len() >= 7 { n[6] } else { SLOTRACE_RANDOM };
        let computer_type = if n.len() >= 8 { n[7] } else { SLOTCOMP_NORMAL };
        let handicap = if n.len() >= 9 { n[8] } else { 100 };

        Self {
            pid,
            download_status,
            slot_status,
            computer,
            team,
            colour,
            race,
            computer_type,
            handicap,
        }
    }

    // Конструктор с параметрами
    pub fn new(
        pid: u8,
        download_status: u8,
        slot_status: u8,
        computer: u8,
        team: u8,
        colour: u8,
        race: u8,
        computer_type: u8,
        handicap: u8,
    ) -> Self {
        Self {
            pid,
            download_status,
            slot_status,
            computer,
            team,
            colour,
            race,
            computer_type,
            handicap,
        }
    }

    pub fn new_from_byte_array(n: &[u8]) -> Self {
        Self::from_bytes(n)
    }

    // Геттеры
    pub fn pid(&self) -> u8 { self.pid }
    pub fn download_status(&self) -> u8 { self.download_status }
    pub fn slot_status(&self) -> u8 { self.slot_status }
    pub fn computer(&self) -> u8 { self.computer }
    pub fn team(&self) -> u8 { self.team }
    pub fn colour(&self) -> u8 { self.colour }
    pub fn race(&self) -> u8 { self.race }
    pub fn computer_type(&self) -> u8 { self.computer_type }
    pub fn handicap(&self) -> u8 { self.handicap }

    // Сеттеры
    pub fn set_pid(&mut self, val: u8) { self.pid = val; }
    pub fn set_download_status(&mut self, val: u8) { self.download_status = val; }
    pub fn set_slot_status(&mut self, val: u8) { self.slot_status = val; }
    pub fn set_computer(&mut self, val: u8) { self.computer = val; }
    pub fn set_team(&mut self, val: u8) { self.team = val; }
    pub fn set_colour(&mut self, val: u8) { self.colour = val; }
    pub fn set_race(&mut self, val: u8) { self.race = val; }
    pub fn set_computer_type(&mut self, val: u8) { self.computer_type = val; }
    pub fn set_handicap(&mut self, val: u8) { self.handicap = val; }

    // Аналог GetByteArray — получить данные как Vec<u8>
    pub fn to_bytes(&self) -> Vec<u8> {
        vec![
            self.pid,
            self.download_status,
            self.slot_status,
            self.computer,
            self.team,
            self.colour,
            self.race,
            self.computer_type,
            self.handicap,
        ]
    }
}
