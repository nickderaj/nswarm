//! Production binary configuration and startup-failure coverage.

mod common;

use std::process::Command;

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
