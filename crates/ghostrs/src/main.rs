use std::path::Path;

use ghostrs::{Config, Supervisor, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init("info")?;
    tracing::info!("ghostrs starting");

    let args: Vec<String> = std::env::args().collect();

    // `--host <game name>` creates a lobby at startup instead of waiting for a
    // root admin's `!pub`. Multiple `--host` flags can be passed to host multiple games.
    let mut host_on_start = Vec::new();
    let mut start_after = None;
    let mut fake_player = false;
    let mut positional = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--host" => {
                if let Some(h) = it.next() {
                    host_on_start.push(h.clone());
                }
            }
            // Fires the same GameCmd::Start that the `!start` command sends, so the
            // start path can be exercised without an admin whispering the bot.
            "--start-after" => start_after = it.next().and_then(|s| s.parse::<u64>().ok()),
            // Seats a fake player in the lobby at startup, the same way
            // `!fakeplayer` would, so a game can reach the playing phase
            // without a human client.
            "--fake-player" => fake_player = true,
            _ => positional.push(a.clone()),
        }
    }

    let args = positional;
    let cfg_path = if let Some(arg) = args.first().filter(|a| Path::new(a).exists()) {
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

    Supervisor::run(cfg, host_on_start, start_after, fake_player).await
}
