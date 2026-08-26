//! Every file in `contracts/` must be read by at least two crates.
//!
//! A golden vector file exists to be an oracle that two independent
//! implementations are both measured against. One read by a single crate is
//! not an oracle — it is that crate's fixture wearing a costume, and it agrees
//! with whatever that crate does. One read by nobody is worse: it looks like
//! coverage in a directory listing and asserts nothing at all.

use css_checks::repo_root;
use walkdir::WalkDir;

/// Crates that could plausibly read a contract file.
const CRATES: &[&str] = &["server", "edge", "cli", "css_lib", "checks"];

fn readers_of(file_name: &str) -> Vec<String> {
    let root = repo_root();
    let mut out = Vec::new();
    for krate in CRATES {
        let mut found = false;
        for entry in WalkDir::new(root.join(krate))
            .into_iter()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        {
            let src = std::fs::read_to_string(entry.path()).unwrap_or_default();
            if src.contains(file_name) {
                found = true;
                break;
            }
        }
        if found {
            out.push((*krate).to_string());
        }
    }
    out
}

#[test]
fn every_contract_file_has_at_least_two_readers() {
    let dir = repo_root().join("contracts");
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .expect("contracts/ must exist")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".json"))
        .collect();
    files.sort();

    assert!(
        !files.is_empty(),
        "contracts/ is empty; this check would pass vacuously"
    );

    let mut problems = Vec::new();
    for file in &files {
        let readers = readers_of(file);
        if readers.len() < 2 {
            problems.push(format!(
                "{file} is read by {readers:?} — a vector file with fewer than two \
                 readers is not an oracle, it is one crate's fixture"
            ));
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
