//! Settings that gate a feature must be read by the code that gates it.
//!
//! `[email]` shipped as nine fully specified configuration fields -- host,
//! port, username, password, use_tls, use_ssl, from_email, from_name, enabled
//! -- documented in `config.sample.toml` as being for "notifications, password
//! resets, etc.", present in both tracked config files, parsed and validated on
//! every boot, and **read by nothing**. A deployment could be configured with
//! working SMTP credentials, restart cleanly, and send no mail, with no error
//! anywhere to say so. `auth.password_reset_enabled` (default `true`) and
//! `auth.require_email_verification` are the same shape.
//!
//! This is the env-var check `configured_env_is_read.rs` performs, applied to
//! configuration: a setting nothing reads is a promise to the operator that the
//! code does not keep.
//!
//! ## Why the obvious version of this check is worthless
//!
//! The first form of this file asked whether each flag was mentioned anywhere
//! in `server/src` outside `config.rs`. It passed before a line of the feature
//! existed, because `server/src/api/admin.rs` reads
//! `new_config.auth.require_email_verification` and puts it in the
//! reload-config JSON response:
//!
//! ```text
//! "auth_config": {
//!     "allow_registration": new_config.auth.allow_registration,
//!     "require_email_verification": new_config.auth.require_email_verification,
//! ```
//!
//! That is a *report of* the setting, not a *use of* it. The flag still gated
//! nothing. A check satisfied by it would have certified the exact defect it
//! was written to catch -- which is worse than no check, because it also tells
//! the next reader the question has been settled.
//!
//! So each flag names the *function* that must act on it -- not merely the
//! module -- and `ECHO_ONLY` lists the files whose mentions do not count, each
//! with its reason.
//!
//! What this does NOT prove: that the flag is read *correctly*, or that the
//! branch it selects does the right thing. A module could read
//! `password_reset_enabled` into a variable and ignore it and this would pass.
//! It proves the wiring exists, which is the half that was missing.

use css_checks::read;

/// A configuration flag, and the function that has to act on it.
///
/// `within` is what makes this check worth having, and it was added because the
/// first version failed its own mutation check. That version asked whether
/// `email.enabled` appeared anywhere in `server/src/mail.rs`. Deleting the gate
/// from `send` did not break it, because `MailService::is_enabled` mentions the
/// same field one accessor away -- so the check certified a module that read
/// the flag and sent mail regardless. Naming the function narrows the claim to
/// the place the flag actually decides something.
struct Flag {
    /// The access as it appears in Rust source, e.g. `email.enabled`.
    token: &'static str,
    /// The file that must contain a real (non-comment) read of it.
    consumer: &'static str,
    /// Signature of the function whose body must contain the read.
    within: &'static str,
    /// The line that closes it: `"\n}"` for a free function, `"\n    }"` for a
    /// method inside an `impl`.
    ///
    /// Carried per flag rather than assumed, because assuming it is how the
    /// first version of this list silently truncated `login` at the first
    /// four-space brace in its body -- an inner `if` block -- and reported a
    /// gate that was plainly there as missing.
    closes_with: &'static str,
    /// Smallest plausible size for that function's body, in bytes.
    ///
    /// Per flag rather than one global floor. `reset_available` is legitimately
    /// two lines, and lowering a shared floor to accommodate it would slacken
    /// the guard for `login` and `send`, where a body that parsed to 130 bytes
    /// really would mean the extractor had lost it.
    min_bytes: usize,
    /// What breaks if nothing reads it. Quoted in the failure.
    consequence: &'static str,
}

const FLAGS: &[Flag] = &[
    Flag {
        token: "email.enabled",
        consumer: "server/src/mail.rs",
        within: "pub async fn send(",
        closes_with: "\n    }",
        min_bytes: 400,
        consequence: "a deployment with `enabled = false` would send mail anyway, and \
                      the caller could not tell a switched-off mailer from a delivered \
                      message",
    },
    Flag {
        token: "config.auth.password_reset_enabled",
        consumer: "server/src/api/auth.rs",
        within: "fn reset_available(",
        closes_with: "\n}",
        min_bytes: 100,
        consequence: "an operator who turned account recovery off would still have \
                      working reset endpoints",
    },
    Flag {
        token: "config.auth.require_email_verification",
        consumer: "server/src/api/auth.rs",
        within: "async fn login(",
        closes_with: "\n}",
        min_bytes: 400,
        consequence: "an operator who required confirmed addresses would get \
                      unconfirmed accounts signing in, which is the state this \
                      setting has been in since it was added",
    },
];

/// Files whose mention of a flag is not a use of it.
///
/// Kept deliberately small. Every entry is a place this check stops being able
/// to see the truth, not a place the truth was inconvenient.
const ECHO_ONLY: &[(&str, &str)] = &[
    (
        "server/src/config.rs",
        "declares the fields; a declaration is not a consumer",
    ),
    (
        "server/src/api/admin.rs",
        "the reload-config response echoes auth settings back into its JSON \
         payload. Reporting a setting is not acting on one -- this is the exact \
         mention that made the first version of this check vacuous",
    ),
];

/// Source with line comments **and string literals** stripped.
///
/// Both halves are load-bearing, and each was added after a mutation check
/// caught this file passing when it should have failed.
///
/// Comments: `server/src/mail.rs` discusses `email.enabled` at length in its
/// module doc, so an unstripped scan is satisfied by documentation describing
/// the feature instead of by the feature -- the "copy can satisfy a check by
/// quoting itself" failure the methodology names. Same idiom as
/// `route_parity.rs:79` and `cli_api_paths.rs:37`.
///
/// String literals: less obvious, and it is the one that actually bit. The
/// gating function's own error message reads "email.enabled is true but
/// email.host is empty". With the gate deleted, that literal still contained
/// the token, so the check passed over a mailer that ignored the setting
/// entirely. A scan that reads an error message as evidence of the behavior the
/// message describes is exactly as wrong as one that reads a comment that way.
fn code(rel: &str) -> String {
    let no_comments = read(rel)
        .lines()
        .map(|line| line.split("//").next().unwrap_or("").to_string())
        .collect::<Vec<_>>()
        .join("\n");

    // A deliberately simple literal stripper: it tracks escapes so `\"` does not
    // end a string, and does not attempt raw strings. Anything it cannot parse
    // it leaves alone, and the length guards below catch a stripper that ate
    // the file.
    let mut out = String::with_capacity(no_comments.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in no_comments.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
                out.push('"');
            }
        } else if c == '"' {
            in_string = true;
            out.push('"');
        } else {
            out.push(c);
        }
    }
    out
}

/// The body of a function, from its signature to the line that closes it.
fn function_body(source: &str, signature: &str, closes_with: &str) -> String {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("no `{signature}` found; the signature changed"));
    let rest = &source[start..];
    let end = rest.find(closes_with).unwrap_or(rest.len());
    rest[..end].to_string()
}

#[test]
fn every_flag_is_read_by_the_function_that_acts_on_it() {
    for flag in FLAGS {
        let body = function_body(&code(flag.consumer), flag.within, flag.closes_with);
        assert!(
            body.contains(flag.token),
            "`{}` is not read inside `{}` in {}.\n\n\
             A configuration field nothing acts on is a setting the operator can \
             change with no effect, which is how `[email]` came to be nine fully \
             documented fields wired to nothing. Reading it elsewhere in the \
             module does not count -- that is precisely the hole that made the \
             first version of this check vacuous. If the gate moved, point this \
             check at its new home; if it was removed, {}.",
            flag.token,
            flag.within,
            flag.consumer,
            flag.consequence
        );
    }
}

#[test]
fn the_scraper_reads_real_source() {
    // Anti-vacuity. `read` panics on a missing file, but a consumer that was
    // emptied, or a comment-stripper that returned "" for everything, would
    // make every assertion above pass over nothing.
    for flag in FLAGS {
        let module = code(flag.consumer);
        assert!(
            module.len() > 500,
            "{} stripped to {} bytes, which is too short to be a module. The \
             comment stripper is broken.",
            flag.consumer,
            module.len()
        );

        let body = function_body(&module, flag.within, flag.closes_with);
        assert!(
            body.len() >= flag.min_bytes,
            "`{}` in {} parsed as {} bytes, below its floor of {}. The extractor \
             is finding the signature and then losing the body, which would make \
             the assertion above run over almost nothing.",
            flag.within,
            flag.consumer,
            body.len(),
            flag.min_bytes
        );
    }
}

#[test]
fn the_flags_still_exist_under_these_names() {
    // Guards the guard. If a field is renamed, the assertion above would look
    // for a token nothing uses and fail with "not read anywhere", pointing the
    // reader at the consumer when the problem is the declaration. Worse, a
    // token could be renamed on both sides and this file would keep asserting
    // a rule about a field that no longer exists.
    let config = code("server/src/config.rs");
    for field in [
        "pub enabled: bool",
        "pub password_reset_enabled: bool",
        "pub require_email_verification: bool",
    ] {
        assert!(
            config.contains(field),
            "`{field}` is no longer declared in server/src/config.rs, so this \
             check is asserting a rule about a field that does not exist. \
             Re-derive the rule rather than re-pointing it."
        );
    }
}

#[test]
fn the_echo_sites_are_still_echoes() {
    // The exemption list is only honest while the thing it exempts is still
    // what it says. If `api/admin.rs` ever starts genuinely gating on
    // `require_email_verification`, this exemption is hiding a real consumer
    // and the entry should be deleted rather than left to rot.
    // Read raw, not through `code`: this assertion is *about* a JSON key, and
    // `code` strips string literals. The rest of the file wants them gone; this
    // one test is the exception, because the echo it pins is half literal.
    let admin = read("server/src/api/admin.rs");
    assert!(
        admin
            .contains("\"require_email_verification\": new_config.auth.require_email_verification"),
        "the reload-config echo in server/src/api/admin.rs has changed shape. \
         If the flag is now actually acted on there, delete its ECHO_ONLY entry \
         and add api/admin.rs as a consumer; if the echo simply moved, update \
         this assertion. Do not leave the exemption in place unexamined -- it \
         is the one thing that can make this check vacuous."
    );

    for (path, reason) in ECHO_ONLY {
        assert!(
            !reason.is_empty(),
            "{path} is exempted without a stated reason. Every entry is a place \
             this check stops being able to see the truth."
        );
        assert!(
            !read(path).is_empty(),
            "{path} is exempted but does not exist, so the exemption is silently \
             covering nothing."
        );
    }
}

#[test]
fn the_mail_dependency_still_selects_its_tls_backend_explicitly() {
    // Not about config being read, but it belongs with the mailer's other
    // cheap structural claims and there is no better home for it.
    //
    // `css-server` already links OpenSSL -- webauthn-rs 0.5 forces it, and
    // reqwest's default-tls does too -- so lettre was added with native-tls to
    // reuse a stack the runtime image already carries. lettre's *default*
    // features would pick a backend on its own, and `Cargo.lock` already
    // carries two rustls majors; a third arriving silently is how a container
    // grows a second TLS trust store, with its own root certificates, and an
    // SMTP failure that reproduces only against one operator's relay.
    //
    // A `cargo add lettre` or a dependabot bump reverts this line without
    // anybody noticing. This is the thing that notices.
    let manifest = read("server/Cargo.toml");
    let start = manifest.find("lettre = ").expect(
        "server/Cargo.toml no longer depends on lettre; if the mailer \
                 moved to another crate, move this check with it",
    );
    let entry = &manifest[start..start + 400.min(manifest.len() - start)];
    let entry = entry.split("\n#").next().unwrap_or(entry);

    assert!(
        entry.contains("default-features = false"),
        "the lettre dependency no longer sets `default-features = false`, so its \
         defaults now choose a TLS backend on this project's behalf:\n\n{entry}"
    );
    assert!(
        entry.contains("tokio1-native-tls"),
        "the lettre dependency no longer names native-tls. If the project moved \
         to rustls deliberately, that is a real decision -- update this check and \
         say so in the commit. If it happened by itself, it should not have:\n\n{entry}"
    );
}
