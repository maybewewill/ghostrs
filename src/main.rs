use util::file_read_full_bytes;



mod ghost;
mod gpsprotocol;
mod socket;
mod logger;
mod crc32;
mod sha1;
mod bnetprotocol;
mod bnet;
mod commandpacket;
mod bncsutil;
mod util;
mod bncsutilinterface;
mod config;
mod gameslot;
mod map;
mod gameprotocol;
mod gameplayer;
mod game_base;
mod game;
mod lang;

#[tokio::main]
async fn main() {
    logger::log_info("[GHOSTRS] Starting GHOSTRS...");
    config::init("default.cfg");

    logger::log_info("[GHOSTRS] loaded config default.cfg...");
   

    let mut ghost = ghost::Ghost::new().await;
    ghost.init().await;

   // println!("{:?}", file_read_full_bytes("maps/iCCup DotA 454.w3x").unwrap().len());
    loop {
        if ghost.update().await {
            logger::log_info("[GHOSTRS] Exiting GHOSTRS...");
            break;
        }
    }
    
}
