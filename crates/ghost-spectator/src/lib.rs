#![forbid(unsafe_code)]

pub mod body;
pub mod relay;
pub mod w3g;

pub use body::ReplayBody;
pub use relay::{Relay, RelayCmd, RelayConfig, RelayHandle, spawn_relay};
pub use w3g::W3gWriter;

/// Packs and writes a replay on a blocking thread. zlib on a 4 MB body takes
/// tens of milliseconds — far more than one 100 ms tick can spare.
pub async fn save_replay(
    path: std::path::PathBuf,
    body: ReplayBody,
    war3_version: u32,
    build: u16,
    tft: bool,
) -> std::io::Result<()> {
    tokio::task::spawn_blocking(move || {
        let (bytes, len_ms) = body.finish();
        let mut w = W3gWriter::new(war3_version, build, tft);
        w.set_replay_length(len_ms);
        std::fs::write(path, w.pack(&bytes))
    })
    .await
    .map_err(|e| std::io::Error::other(e.to_string()))?
}

#[cfg(test)]
mod save_tests {
    use super::*;

    #[tokio::test]
    async fn saving_a_replay_does_not_run_on_the_caller_thread() {
        let dir = std::env::temp_dir().join("ghostrs-w3g-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live.w3g");

        let mut b = ReplayBody::new(1, "host");
        b.set_game("test game", &[0u8; 4], 0);
        b.set_start(vec![0u8; 9], 42, 0, 1);
        b.add_timeslot(100, &[0xAA]);

        save_replay(path.clone(), b, 26, 6059, true).await.unwrap();

        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b"Warcraft III recorded game\x1A\0"));
        assert_eq!(u32::from_le_bytes([data[32], data[33], data[34], data[35]]) as usize, data.len());
        assert_eq!(u32::from_le_bytes([data[60], data[61], data[62], data[63]]), 100);
    }
}
