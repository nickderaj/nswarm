//! Production binary configuration and startup-failure coverage.

mod common;

use std::process::Command;
use std::time::Duration;
use std::{fs::Permissions, os::unix::fs::PermissionsExt};

#[test]
fn binary_rejects_arguments_and_missing_settings_without_secret_output() {
    let program = env!("CARGO_BIN_EXE_gym-bot");
    let extra = Command::new(program)
        .arg("unexpected")
        .output()
        .expect("run binary");
    assert!(!extra.status.success());
    assert!(String::from_utf8_lossy(&extra.stderr).contains("unexpected extra argument"));

    let missing = Command::new(program)
        .env_clear()
        .output()
        .expect("run empty environment");
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("GYM_BOT_TOKEN"));
}

#[test]
fn binary_validates_disposable_database_before_binding_socket() {
    let directory = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(directory.path(), Permissions::from_mode(0o750))
        .expect("group-readable runtime directory");
    let database = common::copy_fixture(&directory, "gym.db");
    let token = "synthetic-secret-never-log";
    let output = Command::new(env!("CARGO_BIN_EXE_gym-bot"))
        .env_clear()
        .env("GYM_BOT_TOKEN", token)
        .env("OWNER_TELEGRAM_ID", "1001")
        .env("GYM_DATA_DIR", directory.path())
        .env("TIMEZONE", "Europe/London")
        .output()
        .expect("run configured binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("gym MCP socket error:"),
        "stderr was {stderr:?}"
    );
    assert!(!stderr.contains(token));
    assert!(database.exists());
}

#[test]
fn explicit_config_map_rejects_missing_database_without_secret_leak() {
    let values = std::collections::HashMap::from([
        ("GYM_BOT_TOKEN".to_owned(), "synthetic-secret".to_owned()),
        ("OWNER_TELEGRAM_ID".to_owned(), "1001".to_owned()),
        (
            "GYM_DATA_DIR".to_owned(),
            "/definitely/missing/gym".to_owned(),
        ),
    ]);
    let error = gym_bot::config::GymConfig::from_map(&values).expect_err("missing database");
    assert!(error.to_string().contains("configured gym database"));
    assert!(!error.to_string().contains("synthetic-secret"));
}

#[test]
fn configured_binary_reaches_bound_runtime_and_stays_alive() {
    let directory = tempfile::tempdir().expect("tempdir");
    std::fs::set_permissions(directory.path(), Permissions::from_mode(0o750))
        .expect("group-readable runtime directory");
    common::copy_fixture(&directory, "gym.db");
    let mut child = Command::new(env!("CARGO_BIN_EXE_gym-bot"))
        .env_clear()
        .env("GYM_BOT_TOKEN", "synthetic")
        .env("OWNER_TELEGRAM_ID", "1001")
        .env("GYM_DATA_DIR", directory.path())
        .env("TIMEZONE", "UTC")
        .env("GYM_MCP_SOCKET", directory.path().join("mcp.sock"))
        .spawn()
        .expect("start configured service");
    std::thread::sleep(Duration::from_millis(100));
    assert!(child.try_wait().expect("status").is_none());
    child.kill().expect("stop service");
    child.wait().expect("reap service");
}
