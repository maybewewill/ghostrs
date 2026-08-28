

use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const VS_FFI_SIGNATURE: u32 = 0xFEEF_04BD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeFixedFileInfo {
    pub signature: u32,
    pub struct_version: u32,
    pub file_version_ms: u32,
    pub file_version_ls: u32,
    pub product_version_ms: u32,
    pub product_version_ls: u32,
    pub file_flags_mask: u32,
    pub file_flags: u32,
    pub file_os: u32,
    pub file_type: u32,
    pub file_subtype: u32,
    pub file_date_ms: u32,
    pub file_date_ls: u32,
}

impl PeFixedFileInfo {

    pub fn packed_product_version(&self) -> u32 {
        let hi_ms = (self.product_version_ms >> 16) & 0xFF;
        let lo_ms = self.product_version_ms & 0xFF;
        let hi_ls = (self.product_version_ls >> 16) & 0xFF;
        let lo_ls = self.product_version_ls & 0xFF;
        (hi_ms << 24) | (lo_ms << 16) | (hi_ls << 8) | lo_ls
    }
}

pub fn extract_pe_version(data: &[u8]) -> Option<u32> {
    if let Some(ffi) = extract_pe_fixed_file_info(data) {
        return Some(ffi.packed_product_version());
    }

    for w in data.windows(52) {
        if w[0..4] == [0xBD, 0x04, 0xEF, 0xFE] {
            let ms = u32::from_le_bytes(w[16..20].try_into().unwrap_or_default());
            let ls = u32::from_le_bytes(w[20..24].try_into().unwrap_or_default());
            let v = (((ms >> 16) & 0xFF) << 24)
                | ((ms & 0xFF) << 16)
                | (((ls >> 16) & 0xFF) << 8)
                | (ls & 0xFF);
            return Some(v);
        }
    }

    None
}

pub fn extract_pe_fixed_file_info(data: &[u8]) -> Option<PeFixedFileInfo> {
    if data.len() < 64 || &data[0..2] != b"MZ" {
        return None;
    }
    let pe_offset = u32::from_le_bytes(data.get(0x3C..0x40)?.try_into().ok()?) as usize;
    if data.len() < pe_offset + 24 || &data[pe_offset..pe_offset + 4] != b"PE\0\0" {
        return None;
    }

    let num_sections =
        u16::from_le_bytes(data.get(pe_offset + 6..pe_offset + 8)?.try_into().ok()?) as usize;
    let opt_hdr_size =
        u16::from_le_bytes(data.get(pe_offset + 20..pe_offset + 22)?.try_into().ok()?) as usize;

    let opt_hdr_offset = pe_offset + 24;
    let sections_offset = opt_hdr_offset + opt_hdr_size;

    let mut rsrc_virt_addr = None;
    let mut rsrc_raw_offset = None;
    let mut rsrc_raw_size = None;

    for i in 0..num_sections {
        let sec_offset = sections_offset + i * 40;
        if data.len() < sec_offset + 40 {
            break;
        }
        let name = &data[sec_offset..sec_offset + 8];
        if name.starts_with(b".rsrc") {
            let virt_addr = u32::from_le_bytes(
                data.get(sec_offset + 12..sec_offset + 16)?
                    .try_into()
                    .ok()?,
            );
            let raw_size = u32::from_le_bytes(
                data.get(sec_offset + 16..sec_offset + 20)?
                    .try_into()
                    .ok()?,
            );
            let raw_offset = u32::from_le_bytes(
                data.get(sec_offset + 20..sec_offset + 24)?
                    .try_into()
                    .ok()?,
            );
            rsrc_virt_addr = Some(virt_addr);
            rsrc_raw_offset = Some(raw_offset);
            rsrc_raw_size = Some(raw_size);
            break;
        }
    }

    if let (Some(rsrc_va), Some(rsrc_offset_u32), Some(rsrc_size_u32)) =
        (rsrc_virt_addr, rsrc_raw_offset, rsrc_raw_size)
    {
        let rsrc_offset = rsrc_offset_u32 as usize;
        let rsrc_size = rsrc_size_u32 as usize;
        if data.len() >= rsrc_offset + rsrc_size && rsrc_size >= 16 {
            let rsrc = &data[rsrc_offset..rsrc_offset + rsrc_size];
            if let (Some(named_bytes), Some(id_bytes)) = (rsrc.get(12..14), rsrc.get(14..16)) {
                let named_entries =
                    u16::from_le_bytes(named_bytes.try_into().unwrap_or_default()) as usize;
                let id_entries =
                    u16::from_le_bytes(id_bytes.try_into().unwrap_or_default()) as usize;
                let total_entries = named_entries + id_entries;

                for i in 0..total_entries {
                    let entry_off = 16 + i * 8;
                    if let (Some(id_slice), Some(data_slice)) = (
                        rsrc.get(entry_off..entry_off + 4),
                        rsrc.get(entry_off + 4..entry_off + 8),
                    ) {
                        let id_or_name =
                            u32::from_le_bytes(id_slice.try_into().unwrap_or_default());
                        let data_or_dir =
                            u32::from_le_bytes(data_slice.try_into().unwrap_or_default());

                        if id_or_name == 16 && (data_or_dir & 0x8000_0000) != 0 {
                            let l2_off = (data_or_dir & 0x7FFF_FFFF) as usize;
                            if l2_off + 16 <= rsrc.len() {
                                let l2_named = u16::from_le_bytes(
                                    rsrc.get(l2_off + 12..l2_off + 14)
                                        .and_then(|s| s.try_into().ok())
                                        .unwrap_or_default(),
                                ) as usize;
                                let l2_id = u16::from_le_bytes(
                                    rsrc.get(l2_off + 14..l2_off + 16)
                                        .and_then(|s| s.try_into().ok())
                                        .unwrap_or_default(),
                                ) as usize;
                                if l2_named + l2_id > 0 && l2_off + 24 <= rsrc.len() {
                                    let l2_entry = l2_off + 16;
                                    let l2_data_or_dir = u32::from_le_bytes(
                                        rsrc.get(l2_entry + 4..l2_entry + 8)
                                            .and_then(|s| s.try_into().ok())
                                            .unwrap_or_default(),
                                    );

                                    let data_entry_off = if (l2_data_or_dir & 0x8000_0000) != 0 {
                                        let l3_off = (l2_data_or_dir & 0x7FFF_FFFF) as usize;
                                        if l3_off + 24 <= rsrc.len() {
                                            let l3_entry = l3_off + 16;
                                            let d = u32::from_le_bytes(
                                                rsrc.get(l3_entry + 4..l3_entry + 8)
                                                    .and_then(|s| s.try_into().ok())
                                                    .unwrap_or_default(),
                                            );
                                            (d & 0x7FFF_FFFF) as usize
                                        } else {
                                            0
                                        }
                                    } else {
                                        (l2_data_or_dir & 0x7FFF_FFFF) as usize
                                    };

                                    if data_entry_off + 8 <= rsrc.len() {
                                        let data_rva = u32::from_le_bytes(
                                            rsrc.get(data_entry_off..data_entry_off + 4)
                                                .and_then(|s| s.try_into().ok())
                                                .unwrap_or_default(),
                                        );
                                        if let Some(rva_diff) = data_rva.checked_sub(rsrc_va) {
                                            let file_off = rsrc_offset + rva_diff as usize;
                                            if file_off + 52 <= data.len() {
                                                let slice = &data
                                                    [file_off..data.len().min(file_off + 1024)];
                                                for w in slice.windows(52) {
                                                    if w[0..4] == [0xBD, 0x04, 0xEF, 0xFE] {
                                                        return parse_fixed_file_info_struct(w);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

fn parse_fixed_file_info_struct(buf: &[u8]) -> Option<PeFixedFileInfo> {
    if buf.len() < 52 {
        return None;
    }
    let u32_at = |off: usize| -> u32 {
        u32::from_le_bytes([buf[off], buf[off + 1], buf[off + 2], buf[off + 3]])
    };

    Some(PeFixedFileInfo {
        signature: u32_at(0),
        struct_version: u32_at(4),
        file_version_ms: u32_at(8),
        file_version_ls: u32_at(12),
        product_version_ms: u32_at(16),
        product_version_ls: u32_at(20),
        file_flags_mask: u32_at(24),
        file_flags: u32_at(28),
        file_os: u32_at(32),
        file_type: u32_at(36),
        file_subtype: u32_at(40),
        file_date_ms: u32_at(44),
        file_date_ls: u32_at(48),
    })
}

pub fn extract_pe_version_from_file(path: &Path) -> Option<u32> {
    let mut f = File::open(path).ok()?;
    let mut buffer = Vec::new();
    f.by_ref()
        .take(64 * 1024 * 1024)
        .read_to_end(&mut buffer)
        .ok()?;
    extract_pe_version(&buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixed_file_info_version_packing() {
        let ffi = PeFixedFileInfo {
            signature: VS_FFI_SIGNATURE,
            struct_version: 0x00010000,
            file_version_ms: 0x0001001A,
            file_version_ls: 0x00000001,
            product_version_ms: 0x0001001A,
            product_version_ls: 0x00000001,
            file_flags_mask: 0x3F,
            file_flags: 0,
            file_os: 0x4,
            file_type: 0x1,
            file_subtype: 0,
            file_date_ms: 0,
            file_date_ls: 0,
        };

        assert_eq!(ffi.packed_product_version(), 0x011A0001);
    }
}
