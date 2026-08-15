#![forbid(unsafe_code)]

pub mod actor;
pub mod handle;
pub mod lobby;
pub mod players;
pub mod slots;
pub mod state;
pub mod tick;

pub use actor::spawn_game;
pub use handle::{GameCmd, GameHandle};
pub use players::{NameMatch, Player, PlayerTable};
pub use slots::{SlotStatus, SlotTable};
pub use state::{GameConfig, GamePhase, GameState, MapInfo};
pub use tick::TickScheduler;
