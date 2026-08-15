use std::path::Path;

mod config;
mod supervisor;
mod telemetry;

use config::Config;
use supervisor::Supervisor;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init("info")?;
    tracing::info!("ghostrs starting");

    let cfg_path = Path::new("default.cfg");
    let cfg = if cfg_path.exists() {
        tracing::info!("loading configuration from {}", cfg_path.display());
        Config::load(cfg_path)?
    } else {
        tracing::warn!("default.cfg not found, using default configuration");
        Config::parse("")?
    };

    Supervisor::run(cfg).await
}
