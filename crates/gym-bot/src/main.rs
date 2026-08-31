#![expect(
    clippy::multiple_crate_versions,
    reason = "the exact teloxide 0.17 and rmcp 3.1 graphs contain documented incompatible transitive majors"
)]

//! Deployable gym service entrypoint.

use std::sync::Arc;

use gym_bot::{clock::SystemClock, config::GymConfig, mcp::run_mcp_server_for_group};

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args_os().nth(1).is_some() {
        return Err("unexpected extra argument".into());
    }
    let config = GymConfig::from_env()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(run_mcp_server_for_group(
        config.socket_path,
        &config.socket_group,
        config.database_path,
        Arc::new(SystemClock::new(&config.timezone)?),
    ))?;
    Ok(())
}
