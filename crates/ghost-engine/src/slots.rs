use ghost_protocol::w3gs::SlotInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SlotStatus {
    Open = 0,
    Closed = 1,
    Occupied = 2,
}

/// The authoritative slot table. Indices are slot ids (SIDs); the wire format
/// is produced verbatim from this vector.
#[derive(Debug, Clone)]
pub struct SlotTable {
    slots: Vec<SlotInfo>,
}

impl SlotTable {
    pub fn new(num_slots: usize) -> Self {
        let slots = (0..num_slots)
            .map(|i| SlotInfo {
                pid: 0,
                download_status: 255,
                slot_status: SlotStatus::Open as u8,
                computer: 0,
                team: (i / 6) as u8,
                colour: i as u8,
                race: 0x20, // random
                computer_type: 1,
                handicap: 100,
            })
            .collect();
        Self { slots }
    }

    pub fn from_slots(slots: Vec<SlotInfo>) -> Self {
        Self { slots }
    }

    pub fn as_wire(&self) -> &[SlotInfo] {
        &self.slots
    }

    fn get_mut(&mut self, sid: u8) -> Option<&mut SlotInfo> {
        self.slots.get_mut(sid as usize)
    }

    pub fn open(&mut self, sid: u8) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                s.slot_status = SlotStatus::Open as u8;
                s.pid = 0;
                true
            }
            None => false,
        }
    }

    pub fn close(&mut self, sid: u8) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                s.slot_status = SlotStatus::Closed as u8;
                s.pid = 0;
                true
            }
            None => false,
        }
    }

    pub fn swap(&mut self, a: u8, b: u8) -> bool {
        let (a, b) = (a as usize, b as usize);
        if a >= self.slots.len() || b >= self.slots.len() {
            return false;
        }
        self.slots.swap(a, b);
        true
    }

    pub fn occupy_slot(&mut self, sid: u8, pid: u8) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                s.slot_status = SlotStatus::Occupied as u8;
                s.pid = pid;
                s.computer = 0;
                true
            }
            None => false,
        }
    }

    pub fn occupy(&mut self, sid: u8, pid: u8, team: u8, colour: u8) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                s.slot_status = SlotStatus::Occupied as u8;
                s.pid = pid;
                s.team = team;
                s.colour = colour;
                s.computer = 0;
                true
            }
            None => false,
        }
    }

    pub fn replace(&mut self, sid: u8, info: SlotInfo) -> bool {
        match self.get_mut(sid) {
            Some(s) => {
                *s = info;
                true
            }
            None => false,
        }
    }

    /// Frees whichever slot holds `pid`, returning its SID.
    pub fn release(&mut self, pid: u8) -> Option<u8> {
        let sid = self.sid_of_pid(pid)?;
        self.open(sid);
        Some(sid)
    }

    pub fn sid_of_pid(&self, pid: u8) -> Option<u8> {
        self.slots.iter().position(|s| {
            s.slot_status == SlotStatus::Occupied as u8 && s.pid == pid
        }).map(|i| i as u8)
    }

    pub fn first_open(&self) -> Option<u8> {
        self.slots.iter().position(|s| s.slot_status == SlotStatus::Open as u8).map(|i| i as u8)
    }

    pub fn count_open(&self) -> u32 {
        self.slots.iter().filter(|s| s.slot_status == SlotStatus::Open as u8).count() as u32
    }

    pub fn count_occupied(&self) -> u32 {
        self.slots.iter().filter(|s| s.slot_status == SlotStatus::Occupied as u8).count() as u32
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_table_is_all_open() {
        let t = SlotTable::new(12);
        assert_eq!(t.as_wire().len(), 12);
        assert_eq!(t.count_open(), 12);
        assert_eq!(t.count_occupied(), 0);
        assert_eq!(t.first_open(), Some(0));
    }

    #[test]
    fn occupy_and_release_track_pids() {
        let mut t = SlotTable::new(4);
        assert!(t.occupy(1, 7, 0, 3));
        assert_eq!(t.sid_of_pid(7), Some(1));
        assert_eq!(t.count_occupied(), 1);
        assert_eq!(t.count_open(), 3);
        assert_eq!(t.first_open(), Some(0));

        assert_eq!(t.release(7), Some(1));
        assert_eq!(t.sid_of_pid(7), None);
        assert_eq!(t.count_open(), 4);
    }

    #[test]
    fn closed_slots_are_neither_open_nor_occupied() {
        let mut t = SlotTable::new(3);
        assert!(t.close(0));
        assert_eq!(t.count_open(), 2);
        assert_eq!(t.count_occupied(), 0);
        assert_eq!(t.first_open(), Some(1));
        assert!(t.open(0));
        assert_eq!(t.first_open(), Some(0));
    }

    #[test]
    fn swap_moves_occupants() {
        let mut t = SlotTable::new(4);
        t.occupy(0, 5, 0, 1);
        assert!(t.swap(0, 3));
        assert_eq!(t.sid_of_pid(5), Some(3));
    }

    #[test]
    fn out_of_range_operations_are_rejected_not_panicking() {
        let mut t = SlotTable::new(2);
        assert!(!t.close(9));
        assert!(!t.open(9));
        assert!(!t.swap(0, 9));
        assert!(!t.occupy(9, 1, 0, 0));
    }
}
