//! A single-use token must be spent by the statement that reads it, and a
//! rejected one must never answer 401.
//!
//! Three separate claims, kept in one file because they are three ways the same
//! endpoint stops being safe.
//!
//! ## The claim has to be atomic
//!
//! `api/devices.rs:229-247` records the defect this guards, found in the device
//! invite flow: a `SELECT` that found a row unused, followed by an `UPDATE`
//! that spent it, let two concurrent requests each pass the check and one
//! invite mint two devices. The fix was to make the filter part of the write
//! -- claim first, then do the work.
//!
//! On a password reset the consequence is worse than a duplicate device. Two
//! requests redeeming one emailed link could each set a *different* password,
//! and only one of the two people would know which won. Expiry belongs in the
//! same statement for the same reason: a preceding `if expires_at > now` races
//! the clock as well as other writers.
//!
//! ## It must not answer 401
//!
//! `frontend/src/utils/api.ts` calls `authStore.logout()` on **any** 401 from
//! **any** endpoint. A reset handler that answered 401 for an expired token
//! would therefore sign out a user who happened to have a live session and
//! pasted a stale link -- and they would experience it as a mysterious session
//! expiry, not as a stale link. 400 is also the honest status: the token is a
//! parameter of the request, not a credential authenticating the caller.
//!
//! That justification depends on the interceptor still behaving that way, so
//! this file asserts the interceptor too. If it ever stops logging out on 401,
//! this rule deserves a fresh decision by a person rather than continued
//! enforcement by a check nobody remembers the reason for.
//!
//! ## The request endpoint must not reveal who has an account
//!
//! `/password-reset/request` answers identically whether or not the address
//! exists. The single most likely regression is somebody improving the error
//! message into `.ok_or(ApiError::NotFound("No account with that address"))`.
//!
//! What this does NOT prove: that the claim is atomic *at runtime* -- that is
//! the concurrency tier's job, and it should grow a case redeeming one token N
//! ways. This is the half that can be stated without a database, and it is the
//! half that runs on the workstation where the edit is made.

use css_checks::read;

/// Source with line comments stripped, so prose cannot satisfy a scan and --
/// more importantly here -- so this file's own subject matter, discussed at
/// length in the comments of the code it checks, cannot either.
fn code(rel: &str) -> String {
    read(rel)
        .lines()
        .map(|line| line.split("//").next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of a function, from its signature to the first line that is a lone
/// closing brace at the given indent.
fn body_of(source: &str, signature: &str, closing: &str) -> String {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("`{signature}` not found; the signature changed"));
    let rest = &source[start..];
    let end = rest.find(closing).unwrap_or(rest.len());
    rest[..end].to_string()
}

/// The database writers that spend a token.
const CLAIMS: &[&str] = &[
    "pub fn claim_password_reset_token(",
    "pub fn claim_email_verification_token(",
];

/// The public handlers that must never answer 401.
const RECOVERY_HANDLERS: &[&str] = &[
    "async fn password_reset_request(",
    "async fn password_reset_consume(",
];

#[test]
fn every_claim_spends_its_token_in_the_statement_that_finds_it() {
    let db = code("server/src/database.rs");

    for signature in CLAIMS {
        let body = body_of(&db, signature, "\n    }");

        assert!(
            body.contains("used_at.is_null()"),
            "`{signature}` no longer filters on `used_at.is_null()`, so the \
             UPDATE that spends the token does not check that it is unspent. \
             Two concurrent redemptions of one link would both succeed, each \
             setting a different password."
        );
        assert!(
            body.contains("expires_at.gt("),
            "`{signature}` no longer checks expiry inside the statement. A \
             preceding `if` would race the clock as well as other writers, \
             which is the same defect one layer along."
        );
        assert!(
            !body.contains("first::<") && !body.contains(".load::<"),
            "`{signature}` reads rows before writing. That is the check-then-\
             claim shape api/devices.rs:229-247 exists to record: between the \
             read finding the token unused and the write spending it, another \
             request can do the same."
        );
    }
}

#[test]
fn the_device_invite_precedent_still_claims_the_same_way() {
    // A positive control. If the pattern this file describes ever disappears
    // from the codebase it was learned in, the assertions above are enforcing a
    // convention nothing else follows and the reader should know.
    let devices = code("server/src/api/devices.rs");
    assert!(
        devices.contains("used_at.is_null()"),
        "api/devices.rs no longer claims its invite atomically. Either the fix \
         recorded in its own comments was reverted, or the flow moved -- and \
         this file's reasoning, which cites it, needs re-deriving."
    );
}

#[test]
fn no_recovery_handler_answers_401() {
    let auth = code("server/src/api/auth.rs");

    for signature in RECOVERY_HANDLERS {
        let body = body_of(&auth, signature, "\n}");
        assert!(
            !body.contains("ApiError::Unauthorized"),
            "`{signature}` can answer 401.\n\n\
             frontend/src/utils/api.ts logs the user out on any 401, so this \
             signs out anyone with a live session who opens a stale reset link, \
             and presents as a session expiry rather than as a stale link. Use \
             400: the token is a parameter of the request, not a credential."
        );
    }
}

#[test]
fn the_reason_401_is_forbidden_here_still_holds() {
    // The paired premise. This rule is only correct while the interceptor
    // behaves this way; if it changes, the rule above should be re-decided by a
    // person rather than enforced by a check whose reason has quietly expired.
    let api_client = read("frontend/src/utils/api.ts");
    assert!(
        api_client.contains("401") && api_client.contains("logout()"),
        "frontend/src/utils/api.ts no longer logs out on 401. That is the entire \
         justification for `no_recovery_handler_answers_401`. Re-read both and \
         decide again -- do not simply delete one of them."
    );
}

#[test]
fn the_request_endpoint_does_not_say_whether_the_account_exists() {
    let auth = code("server/src/api/auth.rs");
    let body = body_of(&auth, "async fn password_reset_request(", "\n}");

    assert!(
        !body.contains("ApiError::NotFound"),
        "`password_reset_request` can answer 404, which tells an unauthenticated \
         caller whether an address has an account. The endpoint answers \
         identically on both branches by design; an improved error message is \
         the most likely way that gets undone."
    );
    assert!(
        body.contains("RESET_REQUESTED_MESSAGE"),
        "`password_reset_request` no longer returns the shared message constant. \
         The constant exists so the found and not-found branches cannot drift \
         into saying subtly different things -- which is how an enumeration \
         oracle gets built out of a helpful message."
    );
    assert!(
        body.contains("record_failed_attempt"),
        "`password_reset_request` no longer records a throttle attempt. It must \
         record one whether or not the account was found: a 429 that appears \
         only for real addresses is the same enumeration oracle wearing a \
         different hat."
    );
}

#[test]
fn the_scan_discriminates() {
    // Guards the guard, in both directions.
    //
    // Upward: every body must actually have parsed, or the negative assertions
    // above pass over an empty string -- the classic way a `!contains` check
    // certifies nothing.
    let auth = code("server/src/api/auth.rs");
    let db = code("server/src/database.rs");

    for signature in RECOVERY_HANDLERS {
        let body = body_of(&auth, signature, "\n}");
        assert!(
            body.len() > 400,
            "`{signature}` parsed as {} bytes, too short to be the handler",
            body.len()
        );
    }
    for signature in CLAIMS {
        let body = body_of(&db, signature, "\n    }");
        assert!(
            body.len() > 200,
            "`{signature}` parsed as {} bytes, too short to be the writer",
            body.len()
        );
    }

    // Downward: a handler that legitimately *does* distinguish must not be
    // matched by the same predicate, or the negative assertions are true of
    // everything and discriminate nothing. `register` answers Conflict naming
    // the taken field, on purpose -- an account you are creating is one you
    // are entitled to know about.
    let register = body_of(&auth, "async fn register(", "\n}");
    assert!(
        register.contains("ApiError::Conflict"),
        "`register` no longer reports a taken username or address. If that \
         changed deliberately, fine -- but this assertion exists to prove the \
         enumeration scan above is discriminating rather than vacuously true, \
         so it needs a new control."
    );
}
