//! The `[server]` CORS settings must reach a `CorsLayer`.
//!
//! `ServerConfig` has carried `cors_enabled` and `cors_origins` since it was
//! written, and for just as long nothing read either one: there was no
//! `CorsLayer` in the tree and no `Access-Control-*` header was ever emitted.
//! `cors_enabled = true` was the default, so the config announced a capability
//! the server did not have -- and an operator listing `cors_origins` got no
//! effect and no warning. That is issue #21, and it is the quiet kind of defect:
//! nothing errors, the field just does nothing.
//!
//! This pins the wiring so it cannot rot back. It reads source as data, like
//! the other drift checks here, and asserts three joints:
//!
//!   1. both fields are read *outside* config.rs, where they are merely
//!      defined -- a field only its own struct mentions is a field nothing uses;
//!   2. main.rs builds the layer from config and applies it;
//!   3. validate_config runs the boot/reload validation, so an unusable setting
//!      is refused rather than surfacing as an absent header.
//!
//! What it deliberately does not check: that the layer allows the right origins
//! or that preflight works. That is behaviour, and the contract tier asserts it
//! against a running server (`e2e/drivers/contract.mjs`, the CORS cases). A
//! source grep proving a real request is admitted would be proving it against
//! itself.

use css_checks::read;

/// Whole-identifier match, not `contains`: `cors_origins` contains `cors_orig`
/// and a bare substring test would report a truncated or renamed field as read.
fn mentions_identifier(corpus: &str, name: &str) -> bool {
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let bytes = corpus.as_bytes();
    let mut from = 0;
    while let Some(at) = corpus[from..].find(name) {
        let start = from + at;
        let end = start + name.len();
        let before_ok = start == 0 || !ident(bytes[start - 1] as char);
        let after_ok = end >= bytes.len() || !ident(bytes[end] as char);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Source with comments *and* string literals removed, leaving only code.
///
/// Both removals are load-bearing, and the string one is why this is a scanner
/// rather than a `split("//")`. The whole defect was a field described and not
/// used, and this module's own error messages quote the field names back --
/// `"server.cors_enabled is true but server.cors_origins ..."`. A stripper that
/// dropped comments but kept string bodies would find those names and report
/// the fields as read on the strength of the message that complains they are
/// misconfigured: a check passing because of the prose about the thing it is
/// supposed to be checking. String literals also carry `//` inside URLs, which
/// a line-comment split would mis-cut -- the scanner ignores `//` inside a
/// string for the same reason it ignores the field name there.
fn code_only(src: &str) -> String {
    #[derive(PartialEq)]
    enum S {
        Code,
        Str,
        Line,
        Block,
    }
    let mut state = S::Code;
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match state {
            S::Code => match c {
                '"' => state = S::Str,
                '/' if chars.peek() == Some(&'/') => {
                    chars.next();
                    state = S::Line;
                }
                '/' if chars.peek() == Some(&'*') => {
                    chars.next();
                    state = S::Block;
                }
                _ => out.push(c),
            },
            S::Str => match c {
                // Skip the escaped character whole, so a `\"` does not end the
                // string and a `\\` does not swallow the next quote.
                '\\' => {
                    chars.next();
                }
                '"' => state = S::Code,
                _ => {}
            },
            S::Line => {
                if c == '\n' {
                    out.push('\n');
                    state = S::Code;
                }
            }
            S::Block => {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    state = S::Code;
                }
            }
        }
    }
    out
}

/// A source file with its `#[cfg(test)]` module and everything after it cut off.
///
/// The field check must see *production* uses. cors.rs's own unit tests set
/// `c.cors_enabled` / `c.cors_origins` in a fixture builder, so a corpus that
/// kept the test module would report the fields as read even if the real code
/// that builds the layer stopped touching them -- the tests would then be
/// exercising fields nothing in production reads, and this check would say
/// everything is fine. Cutting at the marker keeps the check pointed at the
/// code that actually serves requests.
fn without_test_module(src: &str) -> String {
    match src.find("#[cfg(test)]") {
        Some(at) => src[..at].to_string(),
        None => src.to_string(),
    }
}

#[test]
fn both_cors_fields_are_read_outside_config() {
    // Everything that could consume the fields in production, minus config.rs
    // (which defines them) and minus the test modules (which reference them in
    // fixtures). If the only mention of a field is in the file that declares it
    // or in a test that props it up, the field is dead in the running server --
    // exactly the state this check exists to forbid.
    let corpus = code_only(&format!(
        "{}\n{}",
        without_test_module(&read("server/src/cors.rs")),
        without_test_module(&read("server/src/main.rs")),
    ));

    for field in ["cors_enabled", "cors_origins"] {
        assert!(
            mentions_identifier(&corpus, field),
            "server.{field} is defined in config.rs and read by no other server \
             source. A config field nothing consumes is the issue-#21 defect: a \
             described capability that does nothing. Wire it into cors.rs / \
             main.rs, or remove it from ServerConfig."
        );
    }
}

#[test]
fn main_builds_and_applies_the_cors_layer() {
    let main = code_only(&read("server/src/main.rs"));
    assert!(
        mentions_identifier(&main, "build_layer"),
        "main.rs does not call cors::build_layer, so no CorsLayer is ever \
         constructed from config -- the fields are inert again."
    );
    assert!(
        main.contains("option_layer(cors)"),
        "main.rs builds the CORS layer but does not apply it to a router. It is \
         applied inline as `.layer(option_layer(cors))` on the /api nest -- \
         option_layer so a disabled (None) config is a no-op without moving the \
         nest off api::api_routes(), which main_composes_the_same_router pins. \
         Constructing the layer and dropping it emits no headers and is the same \
         defect wearing a build step."
    );
}

#[test]
fn validate_config_refuses_an_unusable_cors_setting() {
    let config = code_only(&read("server/src/config.rs"));
    assert!(
        mentions_identifier(&config, "validate") && config.contains("cors::validate"),
        "config.rs::validate_config does not call cors::validate, so \
         cors_enabled = true with no origins would not be refused at boot or \
         reload -- it would start and silently emit no headers."
    );
}
