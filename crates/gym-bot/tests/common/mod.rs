use std::path::{Path, PathBuf};

pub fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gym/v0-gym-v5.sqlite3")
}

pub fn copy_fixture(directory: &tempfile::TempDir, name: &str) -> PathBuf {
    let destination = directory.path().join(name);
    std::fs::copy(fixture_path(), &destination).expect("copy sanitized fixture");
    destination
}
