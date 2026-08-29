#![expect(
    clippy::multiple_crate_versions,
    reason = "the exact teloxide 0.17 and rmcp 3.1 graphs contain documented incompatible transitive majors"
)]

//! Minimal production entrypoint for the gym MCP spike.

use std::{path::PathBuf, sync::Arc};

use gym_bot::{clock::SystemClock, mcp::run_mcp_server};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args_os();
    let program = arguments.next().unwrap_or_default();
    let Some(socket_path) = arguments.next().map(PathBuf::from) else {
        return Err(format!(
            "usage: {} <socket-path> <existing-gym-db>",
            PathBuf::from(program).display()
        )
        .into());
    };
    let Some(database_path) = arguments.next().map(PathBuf::from) else {
        return Err("missing existing gym database path".into());
    };
    if arguments.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?;
    runtime.block_on(run_mcp_server(
        socket_path,
        database_path,
        Arc::new(SystemClock),
    ))?;
    Ok(())
}
