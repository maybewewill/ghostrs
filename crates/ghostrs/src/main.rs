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

    let args: Vec<String> = std::env::args().collect();
    let cfg_path = if let Some(arg) = args.get(1) {
        std::path::PathBuf::from(arg)
    } else if Path::new("ghost.toml").exists() {
        std::path::PathBuf::from("ghost.toml")
    } else {
        std::path::PathBuf::from("default.cfg")
    };

    let cfg = if cfg_path.exists() {
        tracing::info!("loading configuration from {}", cfg_path.display());
        Config::load(&cfg_path)?
    } else {
        tracing::warn!("no config file found, using default configuration");
        Config::from_toml("")?
    };

    Supervisor::run(cfg).await
}
