use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt};

/// Installs the global tracing subscriber. `default_level` is used when
/// RUST_LOG is unset, e.g. "info" or "ghost_engine=debug,info".
pub fn init(default_level: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_level));
    fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(true)
        .try_init()
        .map_err(|e| anyhow::anyhow!("failed to install tracing subscriber: {e}"))
}
