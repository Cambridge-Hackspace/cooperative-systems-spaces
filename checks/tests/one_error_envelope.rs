//! Every error response carries the same envelope.
//!
//! A client parsing responses generically has to be able to write one function.
//! `ApiError::into_response` produces `{"success": false, "error": "..."}` and
//! that is the shape the frontend's `ApiResponse<T>` type declares — but two
//! other places used to produce something else:
//!
//!   * `AuthError::into_response` built `{"error": "..."}` with no `success`,
//!     which meant a request rejected by an *extractor* carried a different
//!     shape from one rejected by a handler, on every guarded route in the API;
//!   * `api/pages.rs` added a `slug` key and dropped `success`, and
//!     `api/calendar.rs` added a `details` key carrying `e.to_string()` to the
//!     caller.
//!
//! The seeded fuzz tier found the first one from a standing start, with an
//! oracle that knows nothing about any endpoint. This check is the cheap version
//! of the same claim: it runs in milliseconds, on any host, with no stack.
//!
//! What it does NOT prove: that the `success` field is *false*, or that the
//! status matches. Those need a running server and belong to the contract and
//! fuzz tiers. This is about shape.

use css_checks::repo_root;

/// Rust files under `server/src`.
fn sources() -> Vec<std::path::PathBuf> {
    let root = repo_root().join("server/src");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn relative(path: &std::path::Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .to_string()
}

/// Sites that build a JSON object containing an `"error"` key.
///
/// Returns `(file, line, the object's source text)`. The object is taken from
/// the opening `{` of the `json!` macro to its matching `}`, so a key on any
/// line of it counts.
fn error_objects(src: &str, file: &str) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    let bytes: Vec<char> = src.chars().collect();

    for needle in ["json!({", "serde_json::json!({"] {
        let mut from = 0usize;
        while let Some(rel) = src[from..].find(needle) {
            let at = from + rel;
            from = at + needle.len();

            // Skip the ones inside a comment or a doc comment.
            let line_start = src[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let prefix = src[line_start..at].trim_start();
            if prefix.starts_with("//") || prefix.starts_with("///") {
                continue;
            }

            let open = at + needle.len() - 1; // the '{'
            let mut depth = 0i32;
            let mut end = open;
            for (i, c) in bytes.iter().enumerate().skip(open) {
                if *c == '{' {
                    depth += 1;
                } else if *c == '}' {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
            }
            if end <= open {
                continue;
            }
            let body: String = bytes[open..=end].iter().collect();
            if body.contains("\"error\"") {
                let line = src[..at].matches('\n').count() + 1;
                out.push((file.to_string(), line, body));
            }
            from = end;
        }
    }
    out
}

/// Places that legitimately build an object with an `error` key that is not an
/// HTTP error envelope, each with the reason.
///
/// Named individually. A pattern-based exemption -- "anything in webhooks.rs" --
/// would cover the next real one too.
const NOT_AN_ENVELOPE: &[(&str, &str)] = &[
    (
        "server/src/api/webhooks.rs",
        "the delivery *record* written into webhook_deliveries: an audit row \
         describing an attempt, with `delivered` and `error` columns. It is not \
         sent to any HTTP caller.",
    ),
    (
        "server/src/api/mfa.rs",
        "audit event data for a failed MFA attempt, carrying the method and the \
         reason into the audit log. Not an HTTP response body.",
    ),
];

#[test]
fn the_scan_found_something_to_check() {
    let total: usize = sources()
        .iter()
        .map(|p| {
            std::fs::read_to_string(p)
                .map(|s| error_objects(&s, &relative(p)).len())
                .unwrap_or(0)
        })
        .sum();
    assert!(
        total >= 5,
        "found only {total} JSON objects carrying an \"error\" key under \
         server/src; the scan is broken and every assertion below would pass \
         over nothing"
    );
}

#[test]
fn every_error_body_carries_success() {
    let mut offenders = Vec::new();

    for path in sources() {
        let rel = relative(&path);
        if NOT_AN_ENVELOPE.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (file, line, body) in error_objects(&src, &rel) {
            if !body.contains("\"success\"") {
                offenders.push(format!(
                    "{file}:{line}: {}",
                    body.replace('\n', " ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these error bodies do not carry a `success` field:\n{}\n\n\
         Every other error in this API is `{{\"success\": false, \"error\": ..}}`, \
         and that is the shape `frontend/src/types`'s ApiResponse<T> declares. A \
         second shape means a client parsing responses generically silently \
         fails to see the error -- and which shape it gets depends on where in \
         the request pipeline the failure happened, which no client can know.\n\n\
         If this object is not an HTTP response body, add its file to \
         NOT_AN_ENVELOPE with the reason.",
        offenders.join("\n")
    );
}

#[test]
fn auth_errors_do_not_build_their_own_envelope() {
    // The specific regression. `AuthError` is the only error type in the
    // codebase reached by two routes -- an extractor rejection and a handler's
    // `?` -- so it is the only one that can produce two shapes for one failure.
    // Delegating is the only version that cannot drift.
    let src = std::fs::read_to_string(repo_root().join("server/src/auth.rs"))
        .expect("server/src/auth.rs must exist");

    let at = src
        .find("impl IntoResponse for AuthError {")
        .expect("AuthError must still implement IntoResponse");
    let body = &src[at..];
    let end = body.find("\n}\n").map(|i| i + 2).unwrap_or(body.len());
    let body = &body[..end];

    assert!(
        body.contains("ApiError::from(self).into_response()"),
        "AuthError::into_response no longer delegates to ApiError. It used to \
         build its own `{{\"error\": ..}}` body -- no `success` -- so a request \
         rejected by an extractor carried a different envelope from one rejected \
         by a handler, across every guarded route."
    );
    assert!(
        !body.contains("StatusCode::"),
        "AuthError::into_response is deciding status codes again; that mapping \
         belongs in exactly one place, and the second copy is what diverged last \
         time"
    );
}

#[test]
fn no_error_body_repeats_an_internal_message() {
    // `details: e.to_string()` in the calendar handlers put the underlying
    // error -- a URL, a TLS failure, a parse position in somebody else's iCal
    // feed -- into a response to an unauthenticated caller. It is logged one
    // line above in both cases, which is where it belongs.
    let mut offenders = Vec::new();
    for path in sources() {
        let rel = relative(&path);
        if NOT_AN_ENVELOPE.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        for (file, line, body) in error_objects(&src, &rel) {
            if body.contains("e.to_string()") || body.contains("err.to_string()") {
                offenders.push(format!("{file}:{line}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these error bodies interpolate an internal error message into the \
         response:\n{}\n\n\
         Log it and answer with something the caller can act on. \
         `ApiError::DatabaseError` exists precisely so a database message is \
         scrubbed on the way out.",
        offenders.join("\n")
    );
}
