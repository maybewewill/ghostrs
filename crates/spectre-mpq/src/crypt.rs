const TABLE_SIZE: usize = 0x500;

pub const HASH_TABLE_OFFSET: u32 = 0;
pub const HASH_NAME_A: u32 = 0x100;
pub const HASH_NAME_B: u32 = 0x200;
pub const HASH_FILE_KEY: u32 = 0x300;

const fn build_storm_buffer() -> [u32; TABLE_SIZE] {
    let mut table = [0u32; TABLE_SIZE];
    let mut seed = 0x0010_0001u32;
    let mut i = 0;
    while i < 0x100 {
        let mut index = i;
        let mut step = 0;
        while step < 5 {
            seed = (seed.wrapping_mul(125).wrapping_add(3)) % 0x2A_AAAB;
            let temp1 = (seed & 0xFFFF) << 16;
            seed = (seed.wrapping_mul(125).wrapping_add(3)) % 0x2A_AAAB;
            let temp2 = seed & 0xFFFF;
            table[index] = temp1 | temp2;
            index += 0x100;
            step += 1;
        }
        i += 1;
    }
    table
}

pub static STORM_BUFFER: [u32; TABLE_SIZE] = build_storm_buffer();

#[must_use]
pub fn hash_string(s: &str, hash_type: u32) -> u32 {
    let mut seed1 = 0x7FED_7FEDu32;
    let mut seed2 = 0xEEEE_EEEEu32;

    for b in s.bytes() {
        let ch = if b == b'/' {
            b'\\'
        } else {
            b.to_ascii_uppercase()
        };
        let ch_u32 = ch as u32;
        let val = STORM_BUFFER[(hash_type as usize) + (ch_u32 as usize)];
        seed1 = val ^ (seed1.wrapping_add(seed2));
        seed2 = ch_u32
            .wrapping_add(seed1)
            .wrapping_add(seed2)
            .wrapping_add(seed2 << 5)
            .wrapping_add(3);
    }

    seed1
}

#[allow(dead_code)]
pub fn decrypt_u32(data: &mut [u32], mut key: u32) {
    let mut seed = 0xEEEE_EEEEu32;
    for item in data.iter_mut() {
        seed = seed.wrapping_add(STORM_BUFFER[0x400 + ((key & 0xFF) as usize)]);
        let ch = *item ^ (key.wrapping_add(seed));
        key = (!key << 21).wrapping_add(0x1111_1111) | (key >> 11);
        seed = ch
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
        *item = ch;
    }
}

pub fn decrypt_bytes(data: &mut [u8], mut key: u32) {
    let mut seed = 0xEEEE_EEEEu32;
    let u32_count = data.len() / 4;

    for i in 0..u32_count {
        seed = seed.wrapping_add(STORM_BUFFER[0x400 + ((key & 0xFF) as usize)]);
        let raw = u32::from_le_bytes([
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ]);
        let ch = raw ^ (key.wrapping_add(seed));
        key = (!key << 21).wrapping_add(0x1111_1111) | (key >> 11);
        seed = ch
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);

        let bytes = ch.to_le_bytes();
        data[i * 4..i * 4 + 4].copy_from_slice(&bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storm_hash_constants_match_spec() {
        assert_eq!(hash_string("(hash table)", HASH_FILE_KEY), 0xC3AF_3770);
        assert_eq!(hash_string("(block table)", HASH_FILE_KEY), 0xEC83_B3A3);
    }

    #[test]
    fn storm_hash_is_case_and_slash_insensitive() {
        assert_eq!(
            hash_string("scripts/war3map.j", HASH_NAME_A),
            hash_string("SCRIPTS\\WAR3MAP.J", HASH_NAME_A)
        );
    }
}
