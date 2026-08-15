mod telemetry;

fn main() -> anyhow::Result<()> {
    telemetry::init("info")?;
    tracing::info!("ghostrs starting");
    Ok(())
}
