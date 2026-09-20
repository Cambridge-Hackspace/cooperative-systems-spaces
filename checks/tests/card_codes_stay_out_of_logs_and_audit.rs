//! A card swipe must not write the card's code anywhere durable (#107).
//!
//! Six sites did. Three `tracing::info!` lines interpolated `req.card` on every
//! tool-on, tool-off and tool-log; three audit `event_data` objects carried
//! `"card": req.card` or `"card_code": card.code`. So the best-protected copy of
//! a card identifier was the one in Postgres, and the least-protected copies
//! were generated continuously, at volume, by normal use -- into a file that is
//! uploaded as a CI artifact on every battery run and, in production, readable
//! by anyone who can reach podman on that host.
//!
//! This is a privacy and data-minimisation check, not an access-control one.
//! The platform deliberately accepts whatever card a member already has, so
//! UID-only cards are in scope by requirement and a UID is readable by anyone
//! standing nearby. The identifier is **not a secret** and nothing here pretends
//! otherwise. What it is, at volume and in plaintext, is a record of members'
//! movements through the space by name-equivalent identifier, in files nobody
//! audits.
//!
//! ## Why this exists alongside the e2e `logs` stage
//!
//! The battery greps the running server's log for the fixture's card code. That
//! is a real oracle and it is the one that proves the claim end-to-end, but it
//! can only see code paths the battery executes. A `tracing::info!` on an error
//! branch no e2e case reaches leaks in production and stays invisible to it
//! forever -- and error branches are exactly where a card gets logged, because
//! that is when somebody wants to know which card it was.
//!
//! This check reads the source instead, so coverage is irrelevant: it needs no
//! database, no stack, and would have caught all six original sites. The two
//! are not redundant. The grep proves the deployed binary is quiet on the paths
//! it walks; this proves no path can be noisy.

use css_checks::read;
use std::path::PathBuf;

/// Every `.rs` file under `dir`, relative to the repo root.
fn rust_files(dir: &str) -> Vec<String> {
    let root = css_checks::repo_root();
    let mut out = Vec::new();
    let mut stack = vec![root.join(dir)];
    while let Some(p) = stack.pop() {
        let entries = std::fs::read_dir(&p).unwrap_or_else(|e| panic!("read_dir {p:?}: {e}"));
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|x| x == "rs") {
                let rel: PathBuf = path.strip_prefix(&root).expect("under root").to_path_buf();
                out.push(rel.to_string_lossy().into_owned());
            }
        }
    }
    assert!(!out.is_empty(), "no .rs files found under {dir}");
    out.sort();
    out
}

/// String literal contents blanked, so `"Unknown card"` is not mistaken for a
/// card-bearing expression. Keeps the quotes, so JSON keys survive for the
/// audit rule, which looks at keys rather than at values.
fn blank_string_contents(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.char_indices().peekable();
    let mut in_str = false;
    while let Some((_, c)) = chars.next() {
        match c {
            '\\' if in_str => {
                out.push(' ');
                if chars.next().is_some() {
                    out.push(' ');
                }
            }
            '"' => {
                in_str = !in_str;
                out.push('"');
            }
            _ if in_str => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Every `tracing::<level>!( .. )` invocation in `src`, parens balanced.
///
/// Balanced rather than "to the next `;`", because these calls routinely span
/// a dozen lines and contain `format!`, method calls and nested parens.
fn tracing_invocations(src: &str) -> Vec<String> {
    const LEVELS: &[&str] = &["info", "warn", "error", "debug", "trace"];
    let mut out = Vec::new();
    for level in LEVELS {
        let needle = format!("tracing::{level}!");
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(&needle) {
            let start = from + rel;
            let Some(open_rel) = src[start..].find('(') else {
                break;
            };
            let open = start + open_rel;
            let mut depth = 0usize;
            let mut end = open;
            for (i, c) in src[open..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            out.push(src[start..=end].to_string());
            from = end + 1;
        }
    }
    out
}

/// Card-bearing expressions inside one invocation.
///
/// Strict on purpose: any path whose segments mention `card` counts, with one
/// exception. `card_id` and `card.id` are allowed because they are the
/// *sanctioned alternative* -- #107's fix is to log the id, which identifies
/// which card without reproducing it -- so the exception is the documented
/// escape hatch rather than dead allowance. Anything else mentioning a card
/// trips this, and the fix is to log the id, not to widen this list.
fn card_expressions(invocation: &str) -> Vec<String> {
    let body = blank_string_contents(invocation);
    let mut found: Vec<String> = Vec::new();
    let mut token = String::new();
    for c in body.chars() {
        if c.is_alphanumeric() || c == '_' || c == '.' {
            token.push(c);
        } else {
            if is_card_expression(&token) {
                found.push(token.clone());
            }
            token.clear();
        }
    }
    if is_card_expression(&token) {
        found.push(token);
    }
    found.sort();
    found.dedup();
    found
}

fn is_card_expression(token: &str) -> bool {
    if !token.to_ascii_lowercase().contains("card") {
        return false;
    }
    // The id is the sanctioned thing to log.
    !(token.ends_with("_id") || token.ends_with(".id"))
}

/// JSON object keys in `src` that reproduce a card code.
///
/// Key-exact rather than "contains card", so `card_id`, `card_status`,
/// `card_provided` and `card_id_attempted` -- all of which describe a card
/// without being one -- stay legal.
///
/// Runs on **raw** source, deliberately. The first draft passed it through
/// `blank_string_contents` for symmetry with the log rule, which blanked the
/// keys themselves -- `"card":` became `"    ":` -- so the rule could never
/// match anything and passed on every tree including the broken one. The
/// self-tests below are what caught it. A key is quoted text, so the thing that
/// makes the log rule correct is exactly what makes it wrong here.
fn card_code_keys(src: &str) -> Vec<String> {
    const FORBIDDEN: &[&str] = &["card", "card_code", "code"];
    let mut out = Vec::new();
    for key in FORBIDDEN {
        let needle = format!("\"{key}\":");
        let spaced = format!("\"{key}\" :");
        if src.contains(&needle) || src.contains(&spaced) {
            out.push((*key).to_string());
        }
    }
    out
}

// ── The two rules ────────────────────────────────────────────────────────────

#[test]
fn no_log_line_reproduces_a_card_code() {
    let mut offenders: Vec<String> = Vec::new();
    for dir in ["server/src", "edge/src"] {
        for file in rust_files(dir) {
            for invocation in tracing_invocations(&read(&file)) {
                let exprs = card_expressions(&invocation);
                if !exprs.is_empty() {
                    let one_line = invocation.split_whitespace().collect::<Vec<_>>().join(" ");
                    let shown: String = one_line.chars().take(100).collect();
                    offenders.push(format!("{file}: {exprs:?} in `{shown}`"));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a log line reproduces a card code:\n  {}\n\nThe server log is collected as a \
         CI artifact on every battery run and, in production, is readable by anyone who \
         can reach podman on that host. A card identifier there is a record of one \
         member's movement through the space, retained indefinitely, in a file nobody \
         audits. Log the card's **id** instead -- it says which card without \
         reproducing it, and the tool id and resolved user id are already on the line \
         and are the useful parts.",
        offenders.join("\n  ")
    );
}

/// Scoped to the server, and deliberately not to the edge.
///
/// `edge/src/mqtt.rs` builds `json!({ "card": card, .. })` four times, and those
/// are correct: they are the **request bodies** the edge POSTs to `tool-on`,
/// `tool-off` and `tool-log`. #107 leaves the online path alone by design --
/// "the reader still sends the raw card it just read, and the server resolves
/// it" -- so the card must appear there, and a check that forbade it would be
/// read as license to delete the field that makes a swipe work.
///
/// The server is where durable records are written, and it has no legitimate
/// reason to put a card code in a JSON object at all: today it has zero.
#[test]
fn no_audit_record_reproduces_a_card_code() {
    let mut offenders: Vec<String> = Vec::new();
    for file in rust_files("server/src") {
        let keys = card_code_keys(&read(&file));
        if !keys.is_empty() {
            offenders.push(format!("{file}: {keys:?}"));
        }
    }

    assert!(
        offenders.is_empty(),
        "an audit record reproduces a card code:\n  {}\n\nThe audit trail needs to \
         identify *which* card, not to reproduce it -- `card_id` is a UUID and says \
         the same thing. A code here outlives the log: it is in every database backup, \
         and #108 encrypts `user_cards.code` at rest while this would leave a plaintext \
         copy of the same value sitting in `audit_log.event_data` beside it.",
        offenders.join("\n  ")
    );
}

// ── Self-tests ───────────────────────────────────────────────────────────────
//
// Both rules are source greps, and a grep that finds nothing is
// indistinguishable from a grep that cannot find anything. These feed each one
// the six sites as they actually looked before #107, plus the shapes that must
// stay legal.

#[cfg(test)]
mod the_check_rejects_what_it_is_for {
    use super::*;

    /// Verbatim from `620b66c^` -- the real defect, not a model of it.
    const THE_ORIGINAL_LEAK: &str = r#"
    tracing::info!(
        "Tool on request: card={}, tool_id={}",
        req.card,
        req.tool_id
    );"#;

    #[test]
    fn the_original_log_leak_is_caught() {
        let calls = tracing_invocations(THE_ORIGINAL_LEAK);
        assert_eq!(calls.len(), 1, "the invocation must parse: {calls:?}");
        assert_eq!(
            card_expressions(&calls[0]),
            vec!["req.card".to_string()],
            "the line that shipped a card code to the log on every swipe must be caught"
        );
    }

    #[test]
    fn the_original_audit_leaks_are_caught() {
        for src in [
            r#"json!({ "card": req.card })"#,
            r#"json!({ "card_code": card.code })"#,
        ] {
            assert!(!card_code_keys(src).is_empty(), "must be caught: {src}");
        }
    }

    /// The trap: the format string says "card" on every one of these lines, so
    /// a check that searched the whole invocation would flag the fixed version
    /// too -- and would then be "fixed" by deleting it.
    #[test]
    fn the_word_card_in_a_message_is_not_a_card() {
        let calls = tracing_invocations(r#"tracing::info!("Unknown card on tool {}", tool_id);"#);
        assert_eq!(calls.len(), 1);
        assert!(
            card_expressions(&calls[0]).is_empty(),
            "a message mentioning cards must stay legal, or this check fails on the \
             fixed tree and teaches people to delete it"
        );
    }

    #[test]
    fn logging_the_card_id_stays_legal() {
        let calls = tracing_invocations(r#"tracing::warn!("revoked card {} presented", card.id);"#);
        assert!(
            card_expressions(&calls[0]).is_empty(),
            "the id is the sanctioned alternative #107 names; forbidding it would leave \
             no way to say which card"
        );
    }

    #[test]
    fn keys_that_describe_a_card_without_being_one_stay_legal() {
        let src = r#"json!({ "card_id": c.id, "card_status": c.status,
                             "card_provided": b, "card_id_attempted": a })"#;
        assert!(
            card_code_keys(src).is_empty(),
            "these four are in the tree today and are all correct"
        );
    }

    /// The edge's request bodies are the online wire and must not be flagged --
    /// pinned here because the audit rule's scope is the only thing keeping
    /// them legal, and a future widening would break tool-on for everybody.
    #[test]
    fn the_edges_request_body_is_out_of_scope() {
        let edge = read("edge/src/mqtt.rs");
        assert!(
            !card_code_keys(&edge).is_empty(),
            "if the edge ever stops sending `\"card\"` in its request body, this \
             test is the one to delete -- but while it does, the audit rule must \
             stay scoped to the server or it would forbid a swipe from working"
        );
    }

    #[test]
    fn a_multiline_invocation_is_parsed_whole() {
        let src = "tracing::error!(\n    \"failed for {} ({})\",\n    fmt(a, b),\n    req.card\n);";
        let calls = tracing_invocations(src);
        assert_eq!(calls.len(), 1, "nested parens must not truncate the scan");
        assert_eq!(card_expressions(&calls[0]), vec!["req.card".to_string()]);
    }

    /// And the corpus must be real: if `rust_files` silently returned nothing,
    /// both rules above would pass on any tree at all.
    #[test]
    fn the_corpus_is_not_empty() {
        for dir in ["server/src", "edge/src"] {
            let files = rust_files(dir);
            assert!(files.len() > 5, "{dir} yielded only {files:?}");
            assert!(files.iter().any(|f| f.ends_with("toolguard.rs")), "{dir}");
        }
    }
}
