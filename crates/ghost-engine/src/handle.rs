use ghost_net::{ConnEvent, PlayerLink};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum GameCmd {
    /// Same as the `!fakeplayer` chat command, reachable without Battle.net.
    ToggleFakePlayer,
    /// Attaches a live DotaTV stream to this game. Sent by the supervisor after
    /// it has bound the spectator port, so `GameConfig` stays unchanged.
    AttachDotaTv(std::sync::Arc<ghost_spectator::DotaTvShared>),
    NewConn {
        conn_id: u64,
        link: PlayerLink,
        external_ip: [u8; 4],
    },
    AdoptReconnect {
        conn_id: u64,
        pid: u8,
        reconnect_key: u32,
        last_packet: u32,
        link: PlayerLink,
        response: tokio::sync::oneshot::Sender<bool>,
    },
    Conn(ConnEvent),
    Start {
        by: String,
    },
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameEvent {
    LobbyStatus {
        host_counter: u32,
        slots_open: u32,
        slots_total: u32,
        human_players: u32,
    },
}
