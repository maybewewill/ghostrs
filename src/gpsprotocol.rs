#[allow(unused)]
use crate::util::*;

type ByteArray = Vec<u8>;


pub const GPS_HEADER_CONSTANT: u8 = 248;
pub const REJECTGPS_INVALID: u8 = 1;
pub const REJECTGPS_NOTFOUND: u8 = 2;

#[allow(non_camel_case_types)]
#[repr(u8)]
pub enum Protocol {
    GPS_INIT = 0x01,
    GPS_RECONNECT = 0x02,
    GPS_ACK = 0x03,
    GPS_REJECT = 0x04,
}
#[derive(Debug, Default, Clone)]
pub struct CGPSProtocol {}

#[allow(non_snake_case)]
#[allow(unused)]
impl CGPSProtocol {
    pub fn new() -> Self {
        CGPSProtocol {}
    }

    pub fn SEND_GPSC_INIT(&self, version: u32) -> ByteArray {
        let mut packet: ByteArray = Vec::new();
        packet.push(GPS_HEADER_CONSTANT);
        packet.push(Protocol::GPS_INIT as u8);
        packet.push(0x00);
        packet.push(0x00);
        append_byte_array_from_u32(&mut packet, version, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_GPSC_RECONNECT(&self, pid: u8, reconnect_key: u32, last_packet: u32) -> ByteArray {
        let mut packet: ByteArray = Vec::new();
        packet.push(GPS_HEADER_CONSTANT);
        packet.push(Protocol::GPS_RECONNECT as u8);
        packet.push(0x00);
        packet.push(0x00);
        packet.push(pid);
        append_byte_array_from_u32(&mut packet, reconnect_key, false);
        append_byte_array_from_u32(&mut packet, last_packet, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_GPSC_ACK(&self, last_packet: u32) -> ByteArray {
        let mut packet: ByteArray = Vec::new();
        packet.push(GPS_HEADER_CONSTANT);
        packet.push(Protocol::GPS_ACK as u8);
        packet.push(0x00);
        packet.push(0x00);
        append_byte_array_from_u32(&mut packet, last_packet, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn SEND_GPSC_REJECT(&self, reason: u32) -> ByteArray {
        let mut packet: ByteArray = Vec::new();
        packet.push(GPS_HEADER_CONSTANT);
        packet.push(Protocol::GPS_REJECT as u8);
        packet.push(0x00);
        packet.push(0x00);
        append_byte_array_from_u32(&mut packet, reason, false);
        self.AssignLength(&mut packet);
        packet
    }

    pub fn AssignLength(&self, content: &mut ByteArray) -> bool {
        let mut length_bytes: ByteArray = Vec::new();
        if content.len() >= 4 && content.len() <= 65535 {
            length_bytes = create_byte_array_from_u16(content.len() as u16, false);
            content[2] = length_bytes[0];
            content[3] = length_bytes[1];
            return true;
        }
        false
    }

    pub fn ValidateLength(&self, content: &ByteArray) -> bool {
        let mut length: u16;
        let mut length_bytes: ByteArray = Vec::new();
        if content.len() >= 4 && content.len() <= 65535 {
            length_bytes.push(content[2]);
            length_bytes.push(content[3]);
            length = byte_array_to_u16(&length_bytes, false, 0);
            if length == content.len() as u16 {
                return true;
            }
        }
        false
    }
}