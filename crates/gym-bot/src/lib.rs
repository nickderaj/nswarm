#![expect(
    clippy::multiple_crate_versions,
    reason = "the exact teloxide 0.17 and rmcp 3.1 graphs contain documented incompatible transitive majors"
)]

//! Minimal gym vertical slice for nswarm rebuild Step 2.
//!
//! Telegram types are confined to [`telegram`]. Command handling, `SQLite`
//! persistence, MCP queries, and parity operate on transport-neutral data.

pub mod clock;
pub mod command;
pub mod database;
pub mod mcp;
pub mod parity;
pub mod telegram;
