use ghost_net::{ConnEvent, PlayerLink};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum GameCmd {
    NewConn { conn_id: u64, link: PlayerLink, external_ip: [u8; 4] },
    Conn(ConnEvent),
    Start { by: String },
    Chat(String),
    Unhost,
    Shutdown,
    CreateVirtualHost,
}

/// Cheap, cloneable handle to a game actor.
#[derive(Debug, Clone)]
pub struct GameHandle {
    tx: mpsc::Sender<GameCmd>,
}

impl GameHandle {
    pub fn new(tx: mpsc::Sender<GameCmd>) -> Self {
        Self { tx }
    }

    /// Fire-and-forget. A full queue means the actor is wedged; log and drop
    /// rather than block whoever is calling us.
    pub fn send(&self, cmd: GameCmd) {
        if let Err(e) = self.tx.try_send(cmd) {
            tracing::warn!(error = %e, "game command dropped");
        }
    }

    pub fn is_closed(&self) -> bool {
        self.tx.is_closed()
    }
}
