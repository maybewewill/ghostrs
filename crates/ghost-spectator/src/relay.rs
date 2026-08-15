use std::collections::VecDeque;
use std::time::Duration;
use tokio::time::Instant;

use bytes::Bytes;
use ghost_net::PlayerLink;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
pub struct RelayConfig {
    pub port: u16,
    pub delay: Duration,
    pub max_viewers: usize,
    pub game_name: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RelayError {
    #[error("viewer capacity reached")]
    Full,
}

#[derive(Debug)]
pub enum RelayCmd {
    GameBlock(Bytes),
    PlayerInfo { pid: u8, name: String },
    GameOver,
    Shutdown,
    DebugGetReleasedCount(oneshot::Sender<usize>),
}

#[derive(Debug, Clone)]
pub struct RelayHandle {
    tx: mpsc::Sender<RelayCmd>,
}

impl RelayHandle {
    pub fn new(tx: mpsc::Sender<RelayCmd>) -> Self {
        Self { tx }
    }

    pub fn push(&self, block: Bytes) {
        let _ = self.tx.try_send(RelayCmd::GameBlock(block));
    }

    pub async fn debug_released_count(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(RelayCmd::DebugGetReleasedCount(tx)).await;
        rx.await.unwrap_or(0)
    }
}

pub struct Relay {
    pub cfg: RelayConfig,
    pub viewers: Vec<(u64, PlayerLink)>,
    pub delayed_blocks: VecDeque<(Instant, Bytes)>,
    pub released_count: usize,
}

impl Relay {
    pub fn new(cfg: RelayConfig) -> Self {
        Self {
            cfg,
            viewers: Vec::new(),
            delayed_blocks: VecDeque::new(),
            released_count: 0,
        }
    }

    pub fn add_viewer(&mut self, conn_id: u64, link: PlayerLink) -> Result<(), RelayError> {
        if self.viewers.len() >= self.cfg.max_viewers {
            return Err(RelayError::Full);
        }
        self.viewers.push((conn_id, link));
        Ok(())
    }

    pub fn release_due_blocks(&mut self) {
        let now = Instant::now();
        while let Some(&(release_at, _)) = self.delayed_blocks.front() {
            if release_at <= now {
                let (_, block) = self.delayed_blocks.pop_front().unwrap();
                self.broadcast(&block);
                self.released_count += 1;
            } else {
                break;
            }
        }
    }

    pub fn broadcast(&mut self, bytes: &Bytes) {
        self.viewers.retain(|(_, link)| {
            link.try_send(bytes.clone()).is_ok()
        });
    }
}

pub fn spawn_relay(cfg: RelayConfig) -> (RelayHandle, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(1024);
    let join = tokio::spawn(async move {
        run_relay(cfg, rx).await;
    });
    (RelayHandle::new(tx), join)
}

async fn run_relay(cfg: RelayConfig, mut rx: mpsc::Receiver<RelayCmd>) {
    let mut relay = Relay::new(cfg);
    let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            cmd = rx.recv() => {
                match cmd {
                    Some(RelayCmd::Shutdown) | None => break,
                    Some(RelayCmd::GameBlock(block)) => {
                        let release_at = Instant::now() + relay.cfg.delay;
                        relay.delayed_blocks.push_back((release_at, block));
                        relay.release_due_blocks();
                    }
                    Some(RelayCmd::PlayerInfo { pid: _, name: _ }) => {}
                    Some(RelayCmd::GameOver) => {
                        // Flush any remaining blocks
                        while let Some((_, block)) = relay.delayed_blocks.pop_front() {
                            relay.broadcast(&block);
                            relay.released_count += 1;
                        }
                    }
                    Some(RelayCmd::DebugGetReleasedCount(resp)) => {
                        relay.release_due_blocks();
                        let _ = resp.send(relay.released_count);
                    }
                }
            }

            _ = tick_interval.tick() => {
                relay.release_due_blocks();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::ReplayWriter;

    fn test_link() -> PlayerLink {
        let (tx, _rx) = mpsc::channel(64);
        PlayerLink::for_test(tx)
    }

    #[tokio::test(start_paused = true)]
    async fn blocks_are_released_only_after_the_configured_delay() {
        let (handle, _join) = spawn_relay(RelayConfig {
            port: 0,
            delay: Duration::from_secs(120),
            max_viewers: 8,
            game_name: "t".into(),
        });
        handle.push(Bytes::from_static(&[1, 2, 3]));
        tokio::task::yield_now().await;

        tokio::time::advance(Duration::from_secs(60)).await;
        assert_eq!(handle.debug_released_count().await, 0);

        tokio::time::advance(Duration::from_secs(61)).await;
        assert_eq!(handle.debug_released_count().await, 1);
    }

    #[tokio::test]
    async fn viewers_beyond_the_limit_are_refused() {
        let cfg = RelayConfig {
            port: 0,
            delay: Duration::ZERO,
            max_viewers: 2,
            game_name: "t".into(),
        };
        let mut relay = Relay::new(cfg);
        assert!(relay.add_viewer(1, test_link()).is_ok());
        assert!(relay.add_viewer(2, test_link()).is_ok());
        assert!(relay.add_viewer(3, test_link()).is_err());
    }

    #[test]
    fn replay_header_is_written_and_the_block_count_updated() {
        let dir = std::env::temp_dir().join("ghostrs-replay-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.w3g");
        let mut w = ReplayWriter::create(&path, "test").unwrap();
        w.push_block(&[0xF7, 0x0C, 0x04, 0x00]).unwrap();
        w.finish().unwrap();

        let data = std::fs::read(&path).unwrap();
        assert!(data.starts_with(b"Warcraft III recorded game\x1A\0"));
        assert!(data.len() > 68);
    }
}
