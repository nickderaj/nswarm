//! Production binary argument and startup-failure coverage.

mod common;

use std::process::Command;

#[test]
fn binary_rejects_missing_and_extra_arguments() {
    let program = env!("CARGO_BIN_EXE_gym-bot");
    for (arguments, expected) in [
        (Vec::<&str>::new(), "usage:"),
        (vec!["socket.sock"], "missing existing gym database path"),
        (vec!["socket.sock", "gym.db"], "missing IANA time-zone name"),
        (
            vec!["socket.sock", "gym.db", "Europe/London", "unexpected"],
            "unexpected extra argument",
        ),
    ] {
        let output = Command::new(program)
            .args(arguments)
            .output()
            .expect("run gym MCP binary");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected),
            "stderr was {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn binary_builds_runtime_and_reports_socket_bind_failure() {
    let directory = tempfile::tempdir().expect("tempdir");
    let database = common::copy_fixture(&directory, "gym.db");
    let missing_parent_socket = directory.path().join("missing/mcp.sock");
    let output = Command::new(env!("CARGO_BIN_EXE_gym-bot"))
        .arg(missing_parent_socket)
        .arg(database)
        .arg("Europe/London")
        .output()
        .expect("run gym MCP binary");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("gym MCP socket error:"),
        "stderr was {stderr:?}"
    );
    assert!(
        !stderr.contains("Io("),
        "stderr used Debug formatting: {stderr:?}"
    );
}
