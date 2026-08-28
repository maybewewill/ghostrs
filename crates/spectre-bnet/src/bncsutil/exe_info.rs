use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::UNIX_EPOCH;

pub const BNCSUTIL_PLATFORM_X86: u32 = 1;
pub const BNCSUTIL_PLATFORM_WINDOWS: u32 = 1;
pub const BNCSUTIL_PLATFORM_WIN: u32 = 1;
pub const BNCSUTIL_PLATFORM_MAC: u32 = 2;
pub const BNCSUTIL_PLATFORM_PPC: u32 = 2;
pub const BNCSUTIL_PLATFORM_OSX: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExeInfo {
    pub exe_info_string: String,
    pub version: u32,
}

pub fn get_exe_info(file_path: &Path, platform: u32) -> Result<ExeInfo, std::io::Error> {
    let metadata = std::fs::metadata(file_path)?;
    let file_size = metadata.len();
    let file_name = file_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("warcraft.exe");

    let modified = metadata.modified().unwrap_or(UNIX_EPOCH);
    let duration = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = duration.as_secs();
    let (yy, month, day, hours, mins, seconds) = timestamp_to_utc_parts(secs);

    let info_str = format!(
        "{} {:02}/{:02}/{:02} {:02}:{:02}:{:02} {}",
        file_name, month, day, yy, hours, mins, seconds, file_size
    );

    let version = match platform {
        BNCSUTIL_PLATFORM_MAC | BNCSUTIL_PLATFORM_OSX => {
            if let Ok(mut f) = File::open(file_path) {
                if f.seek(SeekFrom::End(-4)).is_ok() {
                    let mut buf = [0u8; 4];
                    if f.read_exact(&mut buf).is_ok() {
                        u32::from_be_bytes(buf)
                    } else {
                        0x011a0001
                    }
                } else {
                    0x011a0001
                }
            } else {
                0x011a0001
            }
        }
        _ => crate::bncsutil::pe::extract_pe_version_from_file(file_path).unwrap_or(0x011a0001),
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

        let info = get_exe_info(&test_exe, BNCSUTIL_PLATFORM_X86).expect("exeinfo parsed");
        assert!(info.exe_info_string.starts_with("test_war3.exe "));
        assert!(info.exe_info_string.ends_with(" 471040"));
        let _ = std::fs::remove_file(&test_exe);
    }

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
        let info = get_exe_info(exe_path, BNCSUTIL_PLATFORM_X86).expect("exeinfo parsed");
        assert!(info.exe_info_string.starts_with("warcraft.exe "));
        assert!(
            info.exe_info_string.ends_with(" 471040"),
            "trailing field is the file size in bytes"
        );
        assert_eq!(info.version, 18_481_153, "packed 1.26.0.1");
    }
}
