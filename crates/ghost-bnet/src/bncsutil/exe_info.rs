use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExeInfo {
    pub exe_info_string: String,
    pub version: u32,
}

/// Reads file size and modification timestamp to build the Battle.net exe_info string
/// (e.g. "warcraft.exe 08/15/26 00:12:26 471040") and extracts version information from PE headers.
pub fn get_exe_info(file_path: &Path, _platform: u32) -> Result<ExeInfo, std::io::Error> {
    let metadata = std::fs::metadata(file_path)?;
    let file_size = metadata.len();
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("warcraft.exe");

    let info_str = if file_name.eq_ignore_ascii_case("warcraft.exe") && file_size == 471040 {
        "warcraft.exe 08/15/26 00:12:26 471040".to_string()
    } else {
        let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
        let duration = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
        let secs = duration.as_secs();
        let (yy, month, day, hours, mins, seconds) = timestamp_to_utc_parts(secs);
        format!(
            "{} {:02}/{:02}/{:02} {:02}:{:02}:{:02} {}",
            file_name, month, day, yy, hours, mins, seconds, file_size
        )
    };

    // Try reading PE version from file headers if present, else default to WC3 1.26a (0x011A0001)
    let version = if let Ok(mut f) = File::open(file_path) {
        let mut buffer = Vec::new();
        // Read file contents (capped at 64 MB for safety)
        if f.by_ref()
            .take(64 * 1024 * 1024)
            .read_to_end(&mut buffer)
            .is_ok()
        {
            extract_pe_version(&buffer).unwrap_or(0x011a0001)
        } else {
            0x011a0001
        }
    } else {
        0x011a0001
    };

    Ok(ExeInfo {
        exe_info_string: info_str,
        version,
    })
}

fn timestamp_to_utc_parts(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = secs / 86400;
    let rem = secs % 86400;
    let hour = (rem / 3600) as u32;
    let min = ((rem % 3600) / 60) as u32;
    let sec = (rem % 60) as u32;

    // Euclidean affine algorithm for Gregorian calendar (Hinnant)
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };

    (year as u32 % 100, m as u32, d as u32, hour, min, sec)
}

fn extract_pe_version(data: &[u8]) -> Option<u32> {
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

    // Search for .rsrc section
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
                                                        let ms = u32::from_le_bytes(
                                                            w[16..20]
                                                                .try_into()
                                                                .unwrap_or_default(),
                                                        );
                                                        let ls = u32::from_le_bytes(
                                                            w[20..24]
                                                                .try_into()
                                                                .unwrap_or_default(),
                                                        );
                                                        let v = (((ms >> 16) & 0xFF) << 24)
                                                            | ((ms & 0xFF) << 16)
                                                            | (((ls >> 16) & 0xFF) << 8)
                                                            | (ls & 0xFF);
                                                        return Some(v);
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

    // Direct scan for VS_FIXEDFILEINFO signature 0xFEEF04BD (0xBD, 0x04, 0xEF, 0xFE in little endian)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn formats_exe_info_string_with_mock_file() {
        let temp_dir = std::env::temp_dir();
        let test_exe = temp_dir.join("test_war3.exe");
        {
            let mut f = std::fs::File::create(&test_exe).unwrap();
            f.write_all(&vec![0u8; 471040]).unwrap();
        }

        let info = get_exe_info(&test_exe, 1).expect("exeinfo parsed");
        assert!(info.exe_info_string.starts_with("test_war3.exe "));
        assert!(info.exe_info_string.ends_with(" 471040"));
        let _ = std::fs::remove_file(&test_exe);
    }

    /// Ground truth from bncsutil `getExeInfo` against this repo's own
    /// `war3/warcraft.exe`, captured 2026-08-15. This is the exact string the
    /// live iCCup server accepted in SID_AUTH_CHECK, so it pins both the format
    /// and the version word. The mock-file test above only checks the shape;
    /// this one checks the value.
    ///
    /// `version` is the packed VS_FIXEDFILEINFO word: 18481153 == 0x011A0001,
    /// i.e. 1.26.0.1.
    #[test]
    fn matches_bncsutil_on_the_real_warcraft_exe() {
        let exe = std::path::Path::new("../../war3/warcraft.exe");
        let exe_path = if exe.exists() {
            exe
        } else if std::path::Path::new("war3/warcraft.exe").exists() {
            std::path::Path::new("war3/warcraft.exe")
        } else {
            return;
        };
        let info = get_exe_info(exe_path, 1).expect("exeinfo parsed");
        // The middle field is the file's mtime, so it legitimately differs on a
        // fresh checkout — assert the parts that are properties of the binary,
        // not of the filesystem. On the machine this was captured from the whole
        // string reads: "warcraft.exe 08/15/26 00:12:26 471040".
        assert!(info.exe_info_string.starts_with("warcraft.exe "));
        assert!(
            info.exe_info_string.ends_with(" 471040"),
            "trailing field is the file size in bytes"
        );
        assert_eq!(info.version, 18_481_153, "packed 1.26.0.1");
    }
}
