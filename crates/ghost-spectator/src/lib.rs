#![forbid(unsafe_code)]

pub mod body;
pub mod relay;
pub mod w3g;

pub use body::ReplayBody;
pub use relay::{Relay, RelayCmd, RelayConfig, RelayHandle, spawn_relay};
pub use w3g::W3gWriter;

/// Packs and writes a replay on a blocking thread. zlib on a 4 MB body takes
/// tens of milliseconds — far more than one 100 ms tick can spare. Callers
/// get that guarantee from `tokio::task::spawn_blocking` moving `body.finish()`,
/// `W3gWriter::pack()`, and `std::fs::write()` onto the blocking thread pool;
/// it is not re-verified by every test in this module (see
/// `an_awaiting_caller_can_still_make_progress_while_a_replay_saves` below for
/// the one test that does observe it).
pub async fn save_replay(
    path: std::path::PathBuf,
    body: ReplayBody,
    war3_version: u32,
    build: u16,
    tft: bool,
) -> std::io::Result<()> {
    tokio::task::spawn_blocking(move || {
        let (bytes, len_ms) = body
            .finish()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
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
    async fn saving_a_replay_writes_a_valid_w3g_file() {
        let dir = std::env::temp_dir().join("ghostrs-w3g-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("live.w3g");

        let mut b = ReplayBody::new(1, "host");
        b.set_game("test game", &[0u8; 4], 0);
        b.set_start(vec![0u8; 9], 42, 0, 1).unwrap();
        b.add_timeslot(100, &[0xAA]);

        save_replay(path.clone(), b, 26, 6059, true).await.unwrap();

        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b"Warcraft III recorded game\x1A\0"));
        assert_eq!(
            u32::from_le_bytes([data[32], data[33], data[34], data[35]]) as usize,
            data.len()
        );
        assert_eq!(
            u32::from_le_bytes([data[60], data[61], data[62], data[63]]),
            100
        );
    }

    /// Proves `save_replay` does not block the runtime it's awaited on, rather
    /// than just asserting file contents. `#[tokio::test]` defaults to the
    /// single-threaded `current_thread` flavor, so if `save_replay` ran its
    /// zlib pass and disk write inline on the caller's task instead of via
    /// `spawn_blocking`, that single OS thread would be pinned for the whole
    /// call and the concurrently spawned `ticker` task below could not be
    /// polled even once — it would only get CPU time after `save_replay`
    /// resolves, and `ticks` would still read 0 at that point. Because
    /// `spawn_blocking` moves the blocking work to a separate thread pool,
    /// the `current_thread` runtime is free to poll other ready tasks while
    /// awaiting the `JoinHandle`, so `ticker` gets scheduled and increments
    /// `ticks` before `save_replay` completes. The body is large (~4 MB)
    /// specifically to give the ticker a wide window to be scheduled at
    /// least once, making the assertion effectively deterministic rather
    /// than a real-world race.
    #[tokio::test]
    async fn an_awaiting_caller_can_still_make_progress_while_a_replay_saves() {
        let dir = std::env::temp_dir().join("ghostrs-w3g-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("offthread-{}.w3g", std::process::id()));

        let mut b = ReplayBody::new(1, "host");
        b.set_start(vec![0u8; 9], 1, 0, 1).unwrap();
        // ~4 MB of timeslot data so pack()'s zlib pass takes real wall-clock
        // time, giving the concurrently spawned ticker task room to run.
        let chunk = vec![0xAAu8; 4096];
        for _ in 0..1000 {
            b.add_timeslot(100, &chunk);
        }

        let ticks = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
        let ticker_ticks = ticks.clone();
        let ticker = tokio::spawn(async move {
            loop {
                ticker_ticks.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                tokio::task::yield_now().await;
            }
        });

        save_replay(path.clone(), b, 26, 6059, true).await.unwrap();
        ticker.abort();
        std::fs::remove_file(&path).ok();

        assert!(
            ticks.load(std::sync::atomic::Ordering::Relaxed) > 0,
            "the ticker task made no progress while save_replay was pending; \
             this would happen if save_replay blocked the current_thread \
             runtime instead of handing zlib/disk I/O to spawn_blocking"
        );
    }
}
