//! The pages service shares a runtime with the HTTP server, so its git calls
//! must never block a thread -- and must never be made while holding a lock a
//! request handler needs.
//!
//! `PagesService` shells out to `git` to clone and pull the wiki and site
//! repositories. It used to do so with `std::process::Command::output()`, whose
//! contract is to block the calling OS thread until the child exits. Inside an
//! `async fn` that parks a tokio worker: a parked thread cannot be preempted
//! and its task cannot be stolen, so a `git pull` against an unreachable remote
//! took a worker out of the pool for as long as the transport allowed. Tokio
//! sizes its pool at one worker per core.
//!
//! Worse, `refresh_wiki_pages` held `state.pages_service.write().await` -- a
//! *tokio* lock -- across the whole of that. Every reader of `/api/pages/*`
//! takes the matching read lock, so while an administrator's refresh was in
//! flight the wiki, the site pages, the navigation and the site index could not
//! be served; and because of the parked worker they could not be served by
//! another thread either. One button press, one stalled site.
//!
//! This is the same defect `mqtt_never_blocks_the_runtime.rs` exists to catch,
//! in a different file, and it is filed and fixed as #94. That oracle only
//! reads `mqtt.rs`, which is why it did not see this one.
//!
//! A text-level check on purpose, for the same reason as its neighbour: it
//! needs no database, no network and no compiler, so it runs on the FreeBSD
//! workstation where `css-server` cannot be built at all. Reproducing the real
//! failure needs an unreachable remote and a loaded runtime, which no unit test
//! can arrange cheaply -- but the defect is entirely visible in the source.
//!
//! What this does NOT prove: that a refresh works, that the timeout is long
//! enough, or that the runtime stays responsive under load. It proves the
//! constructs that caused the stall are absent, that the async ones that
//! replaced them are present, and that the store's lock is taken after the
//! fetch rather than around it.

use css_checks::read;

fn pages_source() -> String {
    read("server/src/pages.rs")
}

fn admin_source() -> String {
    read("server/src/api/admin.rs")
}

/// Source with `//` line comments stripped.
///
/// Both directions matter. The prose above, and the prose in `pages.rs` itself,
/// explains the bug by naming the very construct being banned -- an unstripped
/// scan would read the explanation as the offence. The mirror case is worse: a
/// presence assertion satisfied by a comment that merely mentions the call it
/// wants.
fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of a top-level `async fn`, from its signature to the first line
/// that closes at column zero... except these are methods, so instead: from the
/// signature to the next line whose indentation returns to the signature's.
fn function_body<'a>(code: &'a str, signature: &str) -> &'a str {
    let start = code
        .find(signature)
        .unwrap_or_else(|| panic!("no `{signature}` in the source being checked"));
    let rest = &code[start..];
    let indent = " ".repeat(
        code[..start]
            .chars()
            .rev()
            .take_while(|c| *c == ' ')
            .count(),
    );
    let closer = format!("\n{indent}}}");
    match rest.find(&closer) {
        Some(end) => &rest[..end + closer.len()],
        None => rest,
    }
}

#[test]
fn the_pages_service_spawns_git_asynchronously() {
    let code = code_only(&pages_source());

    assert!(
        !code.contains("use std::process::Command"),
        "server/src/pages.rs imports std::process::Command, whose output() \
         blocks the calling OS thread until git exits. On the runtime that \
         serves HTTP that parks a worker for the length of a network fetch. \
         Use tokio::process::Command and await it."
    );

    assert!(
        code.contains("use tokio::process::Command"),
        "server/src/pages.rs no longer imports tokio::process::Command. Every \
         git invocation here runs on the runtime that serves HTTP and must be \
         awaited rather than waited on."
    );
}

/// Spawn sites whose future nothing drives, as `file:line  text` for the report.
///
/// "Immediately followed by `.await`" would be the obvious rule and is the
/// wrong one: the bounded form is `timeout(deadline, cmd.output()).await`,
/// where the await belongs to the timeout and the output future is its
/// argument. What actually matters is that an await appears in the same
/// expression, so the child is driven by the runtime rather than waited on by
/// the thread. The continuation line is included because a builder chain
/// routinely puts `.await` on the next one.
///
/// Shared with the self-test below on purpose. A self-test that re-implemented
/// this predicate would be checking its own copy and would agree with itself
/// however wrong the real one became.
fn unawaited_output_sites(code: &str) -> Vec<String> {
    let lines: Vec<&str> = code.lines().collect();
    let mut offenders = Vec::new();

    for (n, line) in lines.iter().enumerate() {
        let Some(pos) = line.find(".output()") else {
            continue;
        };
        let mut window = line[pos + ".output()".len()..].to_string();
        if let Some(next) = lines.get(n + 1) {
            window.push_str(next);
        }
        if !window.contains(".await") {
            offenders.push(format!("  line {}: {}", n + 1, line.trim()));
        }
    }

    offenders
}

#[test]
fn every_child_process_is_awaited() {
    let offenders = unawaited_output_sites(&code_only(&pages_source()));

    assert!(
        offenders.is_empty(),
        "server/src/pages.rs spawns a child whose future nothing awaits. On \
         std::process::Command that parks the worker thread until git exits; \
         on tokio's, an undriven output() is not doing what it looks like it \
         is doing.\n{}",
        offenders.join("\n")
    );
}

#[test]
fn every_git_invocation_goes_through_the_bounded_runner() {
    let code = code_only(&pages_source());

    // One spawn site, so that "every git call has a deadline" is a property of
    // the code rather than of whoever last added a call. A second `.output()`
    // anywhere in this file is a git call that can hang forever.
    let spawns = code.matches(".output()").count();
    assert_eq!(
        1, spawns,
        "server/src/pages.rs spawns a child process in {spawns} places. There \
         should be exactly one -- `run_git`, which applies the deadline. A \
         second spawn site is a git call nothing bounds, and an unbounded git \
         call against an unreachable remote is the outage this file was \
         changed to prevent."
    );

    assert!(
        code.contains("kill_on_drop(true)"),
        "server/src/pages.rs does not set kill_on_drop on its git commands. \
         When the deadline fires the future is dropped, and without \
         kill_on_drop the git process survives it -- still holding index.lock, \
         so the timeout meant to recover the service is what stops every later \
         refresh from starting."
    );

    assert!(
        code.contains("timeout("),
        "server/src/pages.rs no longer applies a timeout to its git calls. An \
         unreachable remote must fail a refresh, not hold the pages service \
         open until the transport gives up."
    );
}

#[test]
fn the_refresh_handler_fetches_before_it_takes_the_store() {
    let code = code_only(&admin_source());
    let body = function_body(&code, "async fn refresh_pages(");

    let prepare = body.find("PagesService::prepare(").unwrap_or_else(|| {
        panic!(
            "the refresh handler no longer calls PagesService::prepare. The \
             fetch has to happen outside the store's lock; if it moved, this \
             check has to move with it."
        )
    });
    let write = body.find(".write().await").unwrap_or_else(|| {
        panic!(
            "the refresh handler never takes the pages service write lock, so \
             it cannot be publishing anything."
        )
    });

    assert!(
        prepare < write,
        "server/src/api/admin.rs takes the pages service write lock before \
         fetching. That is #94: the lock is a tokio lock on the runtime that \
         serves HTTP, every reader of /api/pages/* takes the matching read \
         lock, and holding it across a git pull stalls all of them. Prepare \
         first, then take the lock to swap the result in."
    );
}

/// Self-tests. An oracle that has never been observed failing is an oracle of
/// unmeasured value -- and these are the exact texts the defect shipped as.
#[cfg(test)]
mod the_oracle_rejects_the_code_that_shipped_the_bug {
    use super::*;

    const OLD_PAGES: &str = r#"
use std::process::Command;
use std::sync::{Arc, RwLock};

impl PagesService {
    async fn sync_repository_static(repo_url: &str, repo_path: &Path) -> Result<()> {
        let output = Command::new("git")
            .arg("-C")
            .arg(repo_path)
            .arg("pull")
            .output()
            .context("Failed to execute git pull")?;
        Ok(())
    }
}
"#;

    const OLD_ADMIN: &str = r#"
async fn refresh_pages(state: AppState) -> Result<()> {
    let mut pages_service = state.pages_service.write().await;
    match PagesService::prepare(&config, &path).await {
        Ok(()) => Ok(()),
        Err(e) => Err(e),
    }
}
"#;

    #[test]
    fn it_sees_the_blocking_import() {
        let code = code_only(OLD_PAGES);
        assert!(code.contains("use std::process::Command"));
        assert!(!code.contains("use tokio::process::Command"));
    }

    #[test]
    fn it_sees_the_unawaited_output() {
        assert!(
            !unawaited_output_sites(&code_only(OLD_PAGES)).is_empty(),
            "the check for an undriven .output() passed on the code that \
             shipped the bug, so it proves nothing about the code that does not"
        );
    }

    /// And it does not simply flag everything: the bounded form this file was
    /// changed to use has to come back clean, or the test above is satisfied by
    /// a predicate that always says yes.
    #[test]
    fn it_accepts_the_bounded_form() {
        let fixed = "        match timeout(deadline, cmd.output()).await {";
        assert!(unawaited_output_sites(fixed).is_empty());
    }

    /// Including when the chain puts the await on the following line.
    #[test]
    fn it_accepts_an_await_on_the_next_line() {
        let wrapped = "        let out = cmd.output()\n            .await?;";
        assert!(unawaited_output_sites(wrapped).is_empty());
    }

    #[test]
    fn it_sees_the_missing_deadline() {
        let code = code_only(OLD_PAGES);
        assert!(!code.contains("kill_on_drop(true)"));
        assert!(!code.contains("timeout("));
    }

    #[test]
    fn it_sees_the_lock_held_across_the_fetch() {
        let code = code_only(OLD_ADMIN);
        let body = function_body(&code, "async fn refresh_pages(");
        let prepare = body.find("PagesService::prepare(").expect("fixture");
        let write = body.find(".write().await").expect("fixture");
        assert!(
            write < prepare,
            "the ordering check passed on the handler that shipped the bug, so \
             it would not have caught it"
        );
    }

    /// And the body extractor must actually stop at the function's end --
    /// otherwise the ordering check above is reading the whole file and the
    /// positions it compares are meaningless.
    #[test]
    fn the_body_extractor_stops_at_the_closing_brace() {
        let source = "\nasync fn refresh_pages(a: u8) {\n    inside();\n}\n\nfn later() {\n    .write().await\n}\n";
        let body = function_body(source, "async fn refresh_pages(");
        assert!(body.contains("inside()"));
        assert!(
            !body.contains(".write().await"),
            "the extractor ran past the end of the function and into the next \
             one, so anything it is asked about is an accident of file order"
        );
    }
}
