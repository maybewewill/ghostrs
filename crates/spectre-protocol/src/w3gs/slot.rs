use bytes::{BufMut, BytesMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotInfo {
    pub pid: u8,

    pub download_status: u8,

    pub slot_status: u8,

    pub computer: u8,
    pub team: u8,
    pub colour: u8,
    pub race: u8,

    pub computer_type: u8,

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
