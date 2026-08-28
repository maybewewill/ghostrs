#![forbid(unsafe_code)]

pub mod actions;
pub mod actor;
pub mod chat;
pub mod gproxy;
pub mod handle;
pub mod hcl;
pub mod lagcheck;
pub mod lobby;
pub mod map;
pub mod mapxfer;
pub mod players;
pub mod slots;
pub mod state;
pub mod stats_dota;
pub mod tick;

pub use actions::MAX_ACTION_PAYLOAD;
pub use actor::spawn_game;
pub use chat::{ChatCommand, parse_command};
pub use gproxy::GProxyBuffer;
pub use handle::{GameCmd, GameEvent, GameHandle};
pub use hcl::Hcl;
pub use map::{MapOverride, ParsedMap, xor_rotate_left};
pub use mapxfer::{Download, MAP_CHUNK, MAX_PARTS_PER_TICK};
pub use players::{NameMatch, Player, PlayerTable};
pub use slots::{SlotStatus, SlotTable};
pub use state::{GameConfig, GamePhase, GameState, MapInfo};
pub use stats_dota::{DotAPlayerStats, StatsDotA};
pub use tick::TickScheduler;
