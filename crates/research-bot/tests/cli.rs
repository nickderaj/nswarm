//! Fail-closed process contract for the architecture-gated research runtime.

use std::process::Command;

#[test]
fn architecture_gated_runtime_fails_closed() {
    let output = Command::new(env!("CARGO_BIN_EXE_research-bot"))
        .output()
        .expect("research runtime boundary launches");
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
        "research-bot runtime is gated by unresolved D23/D24 decisions\n"
    );
    assert!(output.stdout.is_empty());
}
