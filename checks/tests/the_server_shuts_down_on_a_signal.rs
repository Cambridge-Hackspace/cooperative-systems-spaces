//! The server must stop when it is asked to, rather than be killed.
//!
//! The container runs `css-server` directly as PID 1, and the kernel does not
//! deliver a signal to PID 1 unless PID 1 has installed a handler for it. With
//! no handler installed, `SIGTERM` was discarded outright: podman waited its
//! full ten-second stop timeout on every stop and then `SIGKILL`ed the process
//! (#102).
//!
//! That was the normal path, not an edge case. `css-update.timer` checks for a
//! new `:dev` build every fifteen minutes, so every deployment ended this way.
//! And a `SIGKILL` unwinds nothing: in-flight responses are severed mid-write
//! and every spawned task dies where it stands, including audit writes -- a
//! record that exists precisely to survive.
//!
//! Two things have to be true, and only together:
//!
//! * a `SIGTERM` handler is installed, which is what makes the signal
//!   deliverable to PID 1 at all;
//! * `axum::serve` is driven with that signal, which is what makes delivery
//!   mean an orderly stop rather than an abrupt one.
//!
//! Either alone is useless. A handler with no graceful shutdown catches the
//! signal and ignores it; a graceful shutdown with no handler is never reached.
//!
//! Text-level, like its neighbours, so it runs on the workstation where
//! `css-server` cannot be built.
//!
//! A third thing has to be true, and it is the half that actually loses data:
//! the *work* has to stop politely too. A graceful HTTP shutdown that still
//! drops a half-delivered webhook has only moved the problem. So this also
//! asserts that the stop path drains -- `shutdown.finish()` is reached after
//! `serve` returns -- and that the audit writes in the toolguard path are
//! awaited rather than detached, since a record that a routine deploy can drop
//! is not a record.
//!
//! What this does NOT prove: that shutdown completes quickly, or that anything
//! survives the drain deadline expiring. Work still in flight when the deadline
//! passes is still lost; `Shutdown::finish` reports that rather than hiding it,
//! and `server/src/shutdown.rs` tests the timeout path directly.

use css_checks::read;

const MAIN: &str = "server/src/main.rs";

/// Source with `//` line comments stripped -- which also removes `///` docs.
///
/// Necessary in both directions here. The prose in `main.rs` explains the bug
/// by naming the very constructs being required, so an unstripped scan would
/// find them in the explanation; and a presence assertion satisfied by a
/// comment mentioning the call it wants is not an assertion at all.
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

/// Whether `axum::serve(...)` is driven with a shutdown future.
///
/// The call is written across several lines, so this looks at what follows the
/// `axum::serve` token rather than at a single line.
fn serve_is_graceful(code: &str) -> bool {
    match code.find("axum::serve") {
        Some(at) => {
            let tail = &code[at..];
            let end = tail
                .find(".await")
                .map(|e| e + ".await".len())
                .unwrap_or(tail.len());
            tail[..end].contains("with_graceful_shutdown")
        }
        None => panic!("no `axum::serve` in {MAIN}; this check has to move with it"),
    }
}

#[test]
fn the_http_server_is_driven_with_a_shutdown_future() {
    assert!(
        serve_is_graceful(&code_only(&read(MAIN))),
        "{MAIN} calls axum::serve without with_graceful_shutdown. In-flight \
         responses are severed rather than finished, and -- because this binary \
         is PID 1 in its container -- the process has to be SIGKILLed to stop \
         at all."
    );
}

#[test]
fn a_sigterm_handler_is_installed() {
    let code = code_only(&read(MAIN));
    assert!(
        code.contains("SignalKind::terminate"),
        "{MAIN} installs no SIGTERM handler. This binary runs as PID 1, and the \
         kernel does not deliver a signal to PID 1 unless PID 1 has installed a \
         handler for it -- so SIGTERM is discarded, the stop timeout elapses, \
         and the process is killed. Installing the handler is what makes the \
         signal deliverable, not merely what makes it polite."
    );
}

#[test]
fn the_stop_path_drains_background_work() {
    let code = code_only(&read(MAIN));
    let serve = code.find("axum::serve").expect("axum::serve");
    let finish = code.find("shutdown.finish()").unwrap_or_else(|| {
        panic!(
            "{MAIN} never calls shutdown.finish(). Stopping the listener is \
             only half of a shutdown: without the drain, a webhook delivery \
             already in flight is dropped exactly as it was under SIGKILL."
        )
    });
    assert!(
        serve < finish,
        "{MAIN} drains before it stops serving. The drain has to come after \
         `serve` returns, or it waits for work that new requests are still \
         creating."
    );
}

/// Audit writes in the toolguard path must be awaited, not detached.
///
/// They used to be `tokio::spawn`ed and their result discarded with `let _ =`,
/// inside helpers that were themselves `async` and already awaited -- so the
/// detachment bought nothing and cost durability. An audit row exists to be the
/// record that survives; one a routine deployment can drop is not doing its job.
#[test]
fn the_toolguard_audit_writes_are_not_detached() {
    let code = code_only(&read("server/src/api/toolguard.rs"));
    let spawns: Vec<_> = code
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("tokio::spawn"))
        .map(|(n, l)| format!("  line {}: {}", n + 1, l.trim()))
        .collect();

    assert!(
        spawns.is_empty(),
        "server/src/api/toolguard.rs detaches work with tokio::spawn:\n{}\n\n\
         The audit writes here are awaited on purpose (#102). If this is new \
         work that genuinely should outlive the request, spawn it through \
         `state.shutdown.spawn()` so the stop path waits for it.",
        spawns.join("\n")
    );
}

#[cfg(test)]
mod the_check_rejects_the_code_that_shipped_the_bug {
    use super::*;

    const OLD_MAIN: &str = r#"
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
"#;

    const FIXED_MAIN: &str = r#"
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    async fn shutdown_signal() {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    }
"#;

    #[test]
    fn it_sees_the_bare_serve_call() {
        assert!(
            !serve_is_graceful(&code_only(OLD_MAIN)),
            "the check passed on the call that shipped the bug, so it proves \
             nothing about the call that does not"
        );
    }

    /// And it is not simply always-false: the fixed form has to come back clean,
    /// across the line break, or the test above is satisfied by a predicate that
    /// never says yes.
    #[test]
    fn it_accepts_the_multi_line_fixed_form() {
        assert!(serve_is_graceful(&code_only(FIXED_MAIN)));
    }

    #[test]
    fn it_sees_the_missing_handler() {
        assert!(!code_only(OLD_MAIN).contains("SignalKind::terminate"));
        assert!(code_only(FIXED_MAIN).contains("SignalKind::terminate"));
    }

    /// A `with_graceful_shutdown` that appears *after* the await -- in a later
    /// function, say -- is not driving this server. The window has to end at the
    /// await or the check is really asking "does this string appear anywhere".
    #[test]
    fn a_later_mention_does_not_count() {
        let elsewhere =
            "axum::serve(listener, app).await?;\nfn other() { with_graceful_shutdown() }";
        assert!(!serve_is_graceful(elsewhere));
    }
}
