//! The pages refresh actually fetches, and its deadline actually bites (#94).
//!
//! `checks/tests/pages_git_never_blocks_the_runtime.rs` is the cheap half of
//! this: it reads the source and proves the blocking constructs are gone. What
//! it cannot prove is that what replaced them works. A `tokio::process::Command`
//! assembled slightly wrong, a timeout that fires on every call, or a
//! `kill_on_drop` that reaps the child before it is read all pass a text scan
//! and fail in production -- so this drives the real code against a real
//! repository.
//!
//! Local clones on purpose: `git clone /some/path dest` exercises the same code
//! path as a remote URL, and a test that needed the network would be a test
//! that failed for reasons having nothing to do with this file.
//!
//! What this does NOT prove: that the runtime stays responsive while a refresh
//! is in flight. Demonstrating that needs a slow child process, a loaded
//! runtime and a timing assertion, which is a flaky test for a property the
//! source check already settles structurally.

use std::fs;
use std::path::Path;
use std::process::Command;

use css_server::config::PagesConfig;
use css_server::pages::{PageType, PagesService};

/// A git repository with some nested markdown in it, built in a temp dir.
fn fixture_repo(root: &Path) {
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git is required to run this test");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };

    fs::create_dir_all(root.join("TOOLS/LASERS")).unwrap();
    fs::write(root.join("INDEX.md"), "# Index\n").unwrap();
    fs::write(root.join("TOOLS/LATHE.md"), "# Lathe\n").unwrap();
    fs::write(root.join("TOOLS/LASERS/MUSE.md"), "# Muse\n").unwrap();

    git(&["init", "--initial-branch=main"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "user.name", "Test"]);
    git(&["add", "."]);
    git(&["commit", "-m", "fixture"]);
}

fn config_for(repo: &Path, timeout_secs: u64) -> PagesConfig {
    PagesConfig {
        wiki_repo: Some(repo.to_string_lossy().to_string()),
        wiki_readme: false,
        git_timeout_secs: timeout_secs,
        ..Default::default()
    }
}

#[tokio::test]
async fn a_refresh_clones_and_builds_the_pages() {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    fs::create_dir_all(&origin).unwrap();
    fixture_repo(&origin);

    let checkout = tmp.path().join("checkout");
    let gate = tokio::sync::Mutex::new(());

    let prepared =
        PagesService::prepare(&config_for(&origin, 120), &checkout, PageType::Wiki, &gate)
            .await
            .expect("a local clone should succeed");

    // README is excluded by config and there is none; the three markdown files
    // are the whole corpus.
    assert_eq!(3, prepared.len());
}

#[tokio::test]
async fn a_second_refresh_pulls_into_the_existing_checkout() {
    // The clone and the pull are different branches of sync_repository_static,
    // and only the first one runs on a fresh machine. A conversion that got the
    // pull wrong would look perfect until the second refresh of the day.
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    fs::create_dir_all(&origin).unwrap();
    fixture_repo(&origin);

    let checkout = tmp.path().join("checkout");
    let gate = tokio::sync::Mutex::new(());
    let config = config_for(&origin, 120);

    PagesService::prepare(&config, &checkout, PageType::Wiki, &gate)
        .await
        .expect("first refresh clones");

    fs::write(origin.join("TOOLS/MILL.md"), "# Mill\n").unwrap();
    for args in [vec!["add", "."], vec!["commit", "-m", "another page"]] {
        let out = Command::new("git")
            .arg("-C")
            .arg(&origin)
            .args(&args)
            .output()
            .unwrap();
        assert!(out.status.success());
    }

    let prepared = PagesService::prepare(&config, &checkout, PageType::Wiki, &gate)
        .await
        .expect("second refresh pulls");

    assert_eq!(
        4,
        prepared.len(),
        "the pull did not pick up the new page, so the refresh button would \
         report success and change nothing -- which is #81 all over again"
    );
}

#[tokio::test]
async fn the_deadline_is_wired_to_the_git_call() {
    // A zero-second budget cannot be met by anything that has to spawn a
    // process, so this asserts the timeout is actually applied rather than
    // configured and ignored. Without it an unreachable remote holds the pages
    // service open for as long as the transport allows.
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin");
    fs::create_dir_all(&origin).unwrap();
    fixture_repo(&origin);

    let checkout = tmp.path().join("checkout");
    let gate = tokio::sync::Mutex::new(());

    let err = PagesService::prepare(&config_for(&origin, 0), &checkout, PageType::Wiki, &gate)
        .await
        .expect_err("a zero-second budget must fail rather than run to completion");

    let message = format!("{err:#}");
    assert!(
        message.contains("did not finish within"),
        "the refresh failed for some reason other than the deadline, so this \
         proves nothing about the deadline: {message}"
    );
}

#[tokio::test]
async fn a_missing_repository_is_an_error_not_a_hang() {
    let tmp = tempfile::tempdir().unwrap();
    let checkout = tmp.path().join("checkout");
    let gate = tokio::sync::Mutex::new(());
    let nowhere = tmp.path().join("no-such-repository");

    let err = PagesService::prepare(&config_for(&nowhere, 120), &checkout, PageType::Wiki, &gate)
        .await
        .expect_err("cloning a path that does not exist must fail");

    let message = format!("{err:#}");
    assert!(
        message.contains("Git clone failed"),
        "expected a clone failure, got: {message}"
    );
}
