//! `log_event`'s last argument is `user_agent`, and seven call sites treat it
//! as a description.
//!
//! The signature is
//! `log_event(event_type, user_id, actor_id, event_data, ip_address, user_agent)`.
//! Seven calls -- four in `api/training.rs`, three in `api/trainers.rs` -- pass
//! a human sentence to the last one, so rows in `audit_logs` carry text like
//! `Training step 'Lathe safety' created` in the column that is supposed to
//! record which client made the request.
//!
//! Why this is worth pinning rather than shrugging at. The audit log exists to
//! be produced later as a record of who did what. A field that confidently
//! states something untrue about the request is worse than an empty one,
//! because nothing marks it as unreliable, and the two kinds are mixed: most
//! call sites correctly pass `None`, so a reader has no way to tell a real
//! user-agent row from a sentence except by recognising the prose.
//!
//! Not fixed here. Moving seven sentences into `event_data` changes the shape
//! of rows that already exist in deployed databases, which is a decision with a
//! question attached rather than a side effect of an unrelated feature. What
//! this does is stop the count going up: an eighth instance was nearly written
//! by copying the seventh, which is how it reached seven.
//!
//! What this does NOT prove: that the `None` sites should be passing a real
//! user agent (they probably should -- the request's own header is right
//! there), or that any of the seven sentences are wrong in content. Only that
//! the number of call sites putting prose in that field has not grown.

use css_checks::read;

const SOURCES: [&str; 2] = ["server/src/api/training.rs", "server/src/api/trainers.rs"];

/// The sixth argument of every `.log_event(` call in `source`, split by paren
/// depth. Operates on chars throughout: mixing char and byte offsets is how a
/// scanner like this silently starts reading from the wrong place.
fn user_agent_args(source: &str) -> Vec<String> {
    let chars: Vec<char> = source.chars().collect();
    let needle: Vec<char> = ".log_event(".chars().collect();
    let mut out = Vec::new();

    for start in 0..chars.len() {
        if !chars[start..].starts_with(&needle[..]) {
            continue;
        }
        let mut i = start + needle.len();
        let mut depth = 1i32;
        let mut args: Vec<String> = Vec::new();
        let mut cur = String::new();

        while i < chars.len() && depth > 0 {
            let c = chars[i];
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            if c == ',' && depth == 1 {
                args.push(cur.trim().to_string());
                cur.clear();
            } else {
                cur.push(c);
            }
            i += 1;
        }
        args.push(cur.trim().to_string());

        if args.len() >= 6 {
            out.push(args[5].clone());
        }
    }
    out
}

/// A sixth argument that is a human sentence rather than a client identifier.
fn is_prose(arg: &str) -> bool {
    arg.starts_with("Some(format!") || arg.starts_with("Some(\"")
}

fn all_user_agent_args() -> Vec<String> {
    SOURCES
        .iter()
        .flat_map(|p| user_agent_args(&read(p)))
        .collect()
}

#[test]
fn the_scanner_still_finds_the_call_sites() {
    // Anti-vacuity. If `.log_event(` is renamed or the argument list is
    // restructured, every count below becomes zero and the ratchet passes
    // forever while checking nothing.
    let args = all_user_agent_args();
    assert!(
        args.len() >= 9,
        "parsed {} log_event call sites out of {SOURCES:?}, which is too few to \
         be right: {args:?}",
        args.len()
    );
}

#[test]
fn no_more_call_sites_put_prose_in_the_user_agent_field() {
    let prose: Vec<String> = all_user_agent_args()
        .into_iter()
        .filter(|a| is_prose(a))
        .collect();

    assert_eq!(
        prose.len(),
        7,
        "the number of log_event calls passing a human sentence as `user_agent` \
         changed.\n\n\
         If it went UP: the sixth argument is the client's user agent, not a \
         description. Put the sentence in `event_data` under a `summary` key -- \
         see `complete_training_session`, which does -- so the audit row does \
         not assert something untrue about the request.\n\n\
         If it went DOWN: good. Lower this number and say so.\n\n\
         Found: {prose:#?}"
    );
}

#[test]
fn the_completion_handler_is_not_one_of_them() {
    // The site this ratchet was written beside, asserted directly so a revert
    // of it fails here and not only as a change in the count above.
    let source = read("server/src/api/training.rs");
    let start = source
        .find("async fn complete_training_session(")
        .expect("no complete_training_session in api/training.rs");
    let rest = &source[start..];
    let body = &rest[..rest.find("\n}\n").unwrap_or(rest.len())];

    // Both branches, counted rather than `contains`. The handler builds two
    // audit payloads -- one for an attestation, one for an instructor-led
    // completion -- and a `contains` check passes while either one has lost its
    // summary, which is what a mutation check caught this assertion doing.
    assert_eq!(
        body.matches("\"summary\":").count(),
        2,
        "`complete_training_session` should record a summary in `event_data` on \
         both the attestation and the instructor-led branch. If one moved back \
         to the `user_agent` argument, that column then reports a sentence where \
         a client identifier belongs; if a branch was removed, update this."
    );
    for arg in user_agent_args(body) {
        assert!(
            !is_prose(&arg),
            "`complete_training_session` passes prose as `user_agent` again: {arg}"
        );
    }
}
