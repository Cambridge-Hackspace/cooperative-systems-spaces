//! Shared helpers for the structural checks in `checks/tests/`.
//!
//! There is no library API here beyond locating the repository root: each check
//! is a self-contained integration test, so that a failure names the claim that
//! broke rather than a helper three frames down.

use std::path::{Path, PathBuf};

/// The repository root, derived from this crate's own manifest directory.
///
/// Resolved from `CARGO_MANIFEST_DIR` rather than the current directory,
/// because cargo runs integration tests with an unspecified cwd and a check
/// that silently read no files would pass vacuously.
pub fn repo_root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("checks/ always has a parent")
        .to_path_buf();
    assert!(
        root.join("Cargo.toml").is_file(),
        "repo root {} has no Cargo.toml; the layout moved and these checks are \
         reading the wrong tree",
        root.display()
    );
    root
}

/// Read a repo-relative file, failing loudly if it is missing.
///
/// A check that treats a missing file as "nothing to assert" is a check that
/// passes when the thing it guards is deleted.
pub fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}
