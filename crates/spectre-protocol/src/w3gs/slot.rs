use bytes::{BufMut, BytesMut};

/// One entry of the W3GS slot table: 9 bytes on the wire.
/// Ported from src/gameslot.rs and src/engine/slot.rs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotInfo {
    pub pid: u8,
    /// 0..100, or 255 when unknown.
    pub download_status: u8,
    /// 0 = open, 1 = closed, 2 = occupied.
    pub slot_status: u8,
    /// 1 when the slot holds a computer player.
    pub computer: u8,
    pub team: u8,
    pub colour: u8,
    pub race: u8,
    /// 0 = easy, 1 = normal, 2 = insane.
    pub computer_type: u8,
    /// Percentage, normally 100.
    pub handicap: u8,
}

impl SlotInfo {
    pub const WIRE_LEN: usize = 9;

    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u8(self.pid);
        buf.put_u8(self.download_status);
        buf.put_u8(self.slot_status);
        buf.put_u8(self.computer);
        buf.put_u8(self.team);
        buf.put_u8(self.colour);
        buf.put_u8(self.race);
        buf.put_u8(self.computer_type);
        buf.put_u8(self.handicap);
    }
}
