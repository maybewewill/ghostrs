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

    // `--host <game name>` creates a lobby at startup instead of waiting for a
    // root admin's `!pub`. The advert is queued and goes out on the next refresh
    // tick once battle.net login completes.
    let mut host_on_start = None;
    let mut start_after = None;
    let mut positional = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--host" => host_on_start = it.next().cloned(),
            // Fires the same GameCmd::Start that the `!start` command sends, so the
            // start path can be exercised without an admin whispering the bot.
            "--start-after" => start_after = it.next().and_then(|s| s.parse::<u64>().ok()),
            _ => positional.push(a.clone()),
        }
    }

    let args = positional;
    let cfg_path = if let Some(arg) = args.first() {
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

    Supervisor::run(cfg, host_on_start, start_after).await
}
