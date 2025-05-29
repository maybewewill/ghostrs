use std::env;

use tokio::time::{timeout, Duration};
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
mod packed;
mod savegame;

#[tokio::main]
async fn main() {
    unsafe {
        env::set_var("RUST_BACKTRACE", "1");
    }
    logger::log_info("[GHOSTRS] Starting GHOSTRS...");
    config::init("default.cfg");

    logger::log_info("[GHOSTRS] loaded config default.cfg...");

    logger::log_info("[GHOSTRS] Creating Ghost instance...");
    let mut ghost = ghost::Ghost::new().await;
    logger::log_info("[GHOSTRS] Ghost instance created");

    logger::log_info("[GHOSTRS] Initializing Ghost...");
    match timeout(Duration::from_secs(10), ghost.init()).await {
        Ok(()) => logger::log_info("[GHOSTRS] Ghost initialized"),
        Err(_) => {
            logger::log_error("[GHOSTRS] Ghost initialization timed out");
            return;
        }
    }
    loop {
        if ghost.update().await{
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
}