
const CRC32_POLYNOMIAL: u32 = 0x04C11DB7;
#[derive(Debug,Clone)]
pub struct CRC32 {
    table: [u32; 256],
}

impl Default for CRC32 {
    fn default() -> Self {
        CRC32 {
            table: [0u32; 256],  // Or proper initialized table
        }
    }
}

impl CRC32 {
    pub fn new() -> Self {
        CRC32 {
            table: [0; 256],
        }
    }

    pub fn initialize(&mut self) {
        
        for i in 0..256 {
            self.table[i] = self.reflect(i as u32, 8) << 24;

            for j in 0..8 {
                if self.table[i] & (1 << 31) != 0 {
                    self.table[i] = (self.table[i] << 1) ^ CRC32_POLYNOMIAL;
                } else {
                    self.table[i] <<= 1;
                }
            }
            self.table[i] = self.reflect(self.table[i], 32);
        }
    }

    pub fn full_crc(&self, data: &[u8], length: u32) -> u32 {
        let mut crc = 0xFFFFFFFF;
        for i in 0..length {
            let byte = data[i as usize] as u32;
            crc = (crc >> 8) ^ self.table[((crc & 0xFF) ^ byte) as usize];
        }
        crc ^ 0xFFFFFFFF

    }

    pub fn partial_crc(&self, data: &[u8], length: u32) -> u32 {
        let mut crc = 0xFFFFFFFF;
        for i in 0..length {
            let byte =  data[i as usize] as u32;
            crc = (crc >> 8) ^ self.table[((crc & 0xFF) ^ byte) as usize];
        }
        crc ^ 0xFFFFFFFF
    }

    fn reflect(&self, mut data: u32, char: u8) -> u32 {
        let mut result = 0;
        for i in 1..=char {
            if (data & 1) != 0 {
                result |= 1 << (char - i);
            }
            data >>= 1;
        }
        result
    }


}