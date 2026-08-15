use std::fs;
use std::io;
use std::path::Path;
use std::sync::Arc;

use sha1::{Digest, Sha1};

use crate::state::MapInfo;

impl MapInfo {
    pub fn load_from_file(path: &Path) -> io::Result<Self> {
        let data = fs::read(path)?;
        let size = data.len() as u32;
        let crc = crc32fast::hash(&data);
        let mut hasher = Sha1::new();
        hasher.update(&data);
        let sha1_raw = hasher.finalize();
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(&sha1_raw);

        Ok(Self {
            path: path.to_string_lossy().to_string(),
            size,
            info: size,
            crc,
            sha1,
            num_players: 12,
            num_teams: 2,
            width: 128,
            height: 128,
            game_type: 1,
            flags: 0,
            data: Some(Arc::new(data)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_from_file_computes_crc_and_sha1() {
        let tmp = std::env::temp_dir().join("test_map.w3x");
        fs::write(&tmp, b"warcraft 3 map dummy content").unwrap();
        let info = MapInfo::load_from_file(&tmp).unwrap();
        assert_eq!(info.size, 28);
        assert_ne!(info.crc, 0);
        assert_ne!(info.sha1, [0; 20]);
        let _ = fs::remove_file(&tmp);
    }
}
