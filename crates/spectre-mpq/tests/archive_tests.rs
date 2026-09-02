use std::path::Path;
use spectre_mpq::{Archive, MpqError};

#[test]
fn test_open_real_iccup_dota_507_w3x() {
    let map_path = Path::new("../../maps/iCCup DotA 507.w3x");
    if !map_path.exists() {
        eprintln!("Map file {:?} not found, skipping real map test", map_path);
        return;
    }

    let mut archive = Archive::open(map_path).expect("Failed to open real 507 map");

    assert!(archive.has_file("war3map.w3i"), "war3map.w3i must exist");
    assert!(archive.has_file("war3map.j") || archive.has_file("scripts\\war3map.j"), "Script must exist");

    let w3i_data = archive.read_file("war3map.w3i").expect("Failed to read war3map.w3i");
    assert!(!w3i_data.is_empty(), "war3map.w3i data must not be empty");

    // Test MpqFile compatibility
    let file = archive.open_file("war3map.w3i").expect("open_file must succeed");
    assert_eq!(file.size() as usize, w3i_data.len());
    let mut buf = vec![0u8; file.size() as usize];
    let bytes_read = file.read(&mut archive, &mut buf).expect("read into buf must succeed");
    assert_eq!(bytes_read, w3i_data.len());
    assert_eq!(buf, w3i_data);
}

#[test]
fn test_from_bytes_matches_open_from_path() {
    let map_path = Path::new("../../maps/DotA v6.83sf.w3x");
    if !map_path.exists() {
        return;
    }

    let file_bytes = std::fs::read(map_path).expect("read file bytes");
    let archive_from_bytes = Archive::from_bytes(file_bytes).expect("from_bytes must succeed");
    let archive_from_path = Archive::open(map_path).expect("open must succeed");

    assert_eq!(
        archive_from_bytes.has_file("war3map.w3i"),
        archive_from_path.has_file("war3map.w3i")
    );

    let bytes_from_mem = archive_from_bytes.read_file("war3map.w3i").unwrap();
    let bytes_from_disk = archive_from_path.read_file("war3map.w3i").unwrap();
    assert_eq!(bytes_from_mem, bytes_from_disk);
}

#[test]
fn test_non_existent_archive_and_file() {
    let invalid_data = vec![0u8; 1024];
    let res = Archive::from_bytes(invalid_data);
    assert!(matches!(res, Err(MpqError::HeaderNotFound)));

    let map_path = Path::new("../../maps/iCCup DotA 507.w3x");
    if map_path.exists() {
        let archive = Archive::open(map_path).unwrap();
        assert!(!archive.has_file("non_existent_file_xyz.txt"));
        let err = archive.read_file("non_existent_file_xyz.txt");
        assert!(matches!(err, Err(MpqError::FileNotFound(_))));
    }
}
