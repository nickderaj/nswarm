//! Fail-closed process contract for the non-runnable step-1 scaffold.

use std::process::Command;

#[test]
fn step_one_scaffold_fails_closed() {
    let output = Command::new(env!("CARGO_BIN_EXE_research-bot"))
        .output()
        .expect("research scaffold launches");
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
        "research-bot is a non-runnable step-1 policy scaffold\n"
    );
    assert!(output.stdout.is_empty());
}
