
#[test]
fn test_load_iccup_dota_507() {
    use std::path::Path;
    use ghost_engine::map::ParsedMap;

    let path = Path::new("../../maps/iCCup DotA 507.w3x");
    let common_j = std::fs::read("../../maps/common.j").ok();
    let blizzard_j = std::fs::read("../../maps/blizzard.j").ok();
    let res = ParsedMap::load_mpq(path, common_j.as_deref(), blizzard_j.as_deref());
    match res {
        Ok(m) => {
            println!("SUCCESS: parsed map!");
            println!("path: {}", m.info.path);
            println!("size: {}", m.info.size);
            println!("crc: 0x{:08X}", m.info.crc);
            println!("players: {}", m.info.num_players);
            println!("slots: {}", m.slots.len());
        }
        Err(e) => {
            panic!("FAILED to parse map: {:?}", e);
        }
    }
}
