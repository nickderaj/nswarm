//! Production binary argument and startup-failure coverage.

use std::process::Command;

#[test]
fn binary_rejects_missing_and_extra_arguments() {
    let program = env!("CARGO_BIN_EXE_gym-bot");
    for (arguments, expected) in [
        (Vec::<&str>::new(), "usage:"),
        (vec!["socket.sock"], "missing existing gym database path"),
        (
            vec!["socket.sock", "gym.db", "unexpected"],
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
    let missing_parent_socket = directory.path().join("missing/mcp.sock");
    let output = Command::new(env!("CARGO_BIN_EXE_gym-bot"))
        .arg(missing_parent_socket)
        .arg(directory.path().join("gym.db"))
        .output()
        .expect("run gym MCP binary");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Io("));
}
