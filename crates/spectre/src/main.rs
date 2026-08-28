use std::path::Path;

use spectre::{Config, Supervisor, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    telemetry::init("info")?;
    tracing::info!("spectre starting");

    let args: Vec<String> = std::env::args().collect();

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
            "--start-after" => start_after = it.next().and_then(|s| s.parse::<u64>().ok()),
            "--fake-player" => fake_player = true,
            _ => positional.push(a.clone()),
        }
    }

    let args = positional;
    let cfg_path = if let Some(arg) = args.first().filter(|a| Path::new(a).exists()) {
        std::path::PathBuf::from(arg)
    } else if Path::new("spectre.toml").exists() {
        std::path::PathBuf::from("spectre.toml")
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
