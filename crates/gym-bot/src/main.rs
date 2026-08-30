#![expect(
    clippy::multiple_crate_versions,
    reason = "the exact teloxide 0.17 and rmcp 3.1 graphs contain documented incompatible transitive majors"
)]

//! Deployable gym service entrypoint.

use std::sync::Arc;

use gym_bot::{
    clock::{Clock, SystemClock},
    config::GymConfig,
    health::HealthImporter,
    health_server::run_health_server,
    mcp::run_mcp_server,
    receiver::HealthReceiver,
    runtime::{RuntimeService, run_telegram},
};

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
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new(&config.timezone)?);
    let command_service = Arc::new(RuntimeService::new(
        config.owner_id,
        &config.database_path,
        &config.processed_updates_path,
        Arc::clone(&clock),
    )?);
    let health_receiver = Arc::new(HealthReceiver::new(
        config.health_import_token,
        HealthImporter::new(&config.database_path),
    ));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(async move {
        tokio::select! {
            result = run_mcp_server(
                config.socket_path,
                &config.socket_group,
                config.database_path,
                clock,
            ) => result.map_err(|error| Box::new(error) as Box<dyn std::error::Error>),
            result = run_telegram(config.telegram_token, command_service) => {
                result.map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
            }
            result = run_health_server(config.health_bind_address, health_receiver) => {
                result.map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
            }
            result = shutdown_signal() => {
                result.map_err(|error| Box::new(error) as Box<dyn std::error::Error>)
            }
        }
    })?;
    Ok(())
}

async fn shutdown_signal() -> Result<(), std::io::Error> {
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    tokio::select! {
        result = tokio::signal::ctrl_c() => result,
        _ = terminate.recv() => Ok(()),
    }
}
