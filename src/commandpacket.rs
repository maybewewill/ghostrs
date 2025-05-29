use crate::util::*;
#[derive(Clone)]
#[derive(Debug)]
pub struct CommandPacket {
    m_PacketType: u8,
    m_ID: i32,
    m_Data: ByteArray
}


impl CommandPacket {
    pub fn new( packet_type: u8, id: i32, data: ByteArray) -> Self {
        CommandPacket {
            m_PacketType: packet_type,
            m_ID: id,
            m_Data: data
        }
    }

    pub fn get_packet_type(&self) -> u8 {
        self.m_PacketType
    }

    pub fn get_id(&self) -> i32 {
        self.m_ID
    }

    pub fn get_data(&self) -> &ByteArray {
        &self.m_Data
    }
}
