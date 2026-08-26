//! The theme list exists in a fourth place, and it is the one that says no.
//!
//! `frontend/tests/structure/themes.spec.ts` asserts three copies agree:
//! `tailwind.config.js` (what daisyUI compiles), `ThemePicker.vue` (what a
//! person can pick) and `tests/fixtures/themes.json` (what the audit iterates).
//!
//! There is a fourth: `server/src/api/users.rs::update_user_theme` carries a
//! hardcoded `valid_themes` array and rejects anything else with a 400. It is
//! the only copy with teeth — the other three decide what is *offered*, this one
//! decides what is *accepted*.
//!
//! So a theme added to the picker and not here renders a button that produces a
//! 400 when pressed, and one removed here but left in the picker does the same.
//! Neither shows up in the frontend's own checks, because the frontend has no
//! reason to read a Rust file. This does.
//!
//! It lives in `checks/` rather than in the vitest suite for the same reason
//! everything else here does: it reads two files and needs no toolchain, so it
//! runs on the workstation where `css-server` cannot even be built.

use css_checks::{read, repo_root};

/// Theme names from `daisyui.themes` in `tailwind.config.js`, in order.
///
/// Deliberately a second implementation of the parser in
/// `frontend/tests/structure/themes.spec.ts`. The two are in different
/// languages and neither can see the other, which is the only arrangement in
/// which "both agree with tailwind.config.js" means anything.
fn tailwind_themes() -> Vec<String> {
    let src = read("frontend/tailwind.config.js");
    let start = src
        .find("themes: [")
        .expect("tailwind.config.js must declare daisyui.themes");

    let bytes: Vec<char> = src.chars().collect();
    let open = src[start..]
        .find('[')
        .map(|i| src[..start].chars().count() + i)
        .expect("themes list opens with [");
    let mut depth = 0i32;
    let mut end = open;
    for (i, c) in bytes.iter().enumerate().skip(open) {
        if *c == '[' {
            depth += 1;
        } else if *c == ']' {
            depth -= 1;
            if depth == 0 {
                end = i;
                break;
            }
        }
    }
    let body: String = bytes[open..end].iter().collect();

    let mut out = Vec::new();
    for line in body.lines() {
        let code = line.split("//").next().unwrap_or("").trim();
        // A custom theme is an object key -- quoted when it contains a hyphen
        // ('css-light'), bare when it does not (afterdark).
        if let Some(name) = key_before_brace(code) {
            out.push(name);
            continue;
        }
        // A built-in is a bare string in the list.
        if let Some(name) = quoted_only(code) {
            out.push(name);
        }
    }
    out
}

/// `'name': {` or `name: {` -> `name`
fn key_before_brace(code: &str) -> Option<String> {
    let rest = code.strip_suffix('{')?.trim_end().strip_suffix(':')?.trim();
    let name = rest.trim_matches('\'').trim_matches('"');
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return None;
    }
    Some(name.to_string())
}

/// `'name',` -> `name`
fn quoted_only(code: &str) -> Option<String> {
    let rest = code.strip_suffix(',').unwrap_or(code).trim();
    let name = rest.strip_prefix('\'')?.strip_suffix('\'')?;
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '-') {
        return None;
    }
    Some(name.to_string())
}

/// The `valid_themes` array in `update_user_theme`, in order.
fn server_themes() -> Vec<String> {
    let src = read("server/src/api/users.rs");
    let start = src
        .find("let valid_themes = [")
        .expect("server/src/api/users.rs must declare valid_themes");
    let body = &src[start..];
    let end = body.find("];").expect("valid_themes must be terminated");
    let body = &body[..end];

    let mut out = Vec::new();
    let mut rest = body;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

fn fixture_themes() -> Vec<String> {
    let text = std::fs::read_to_string(repo_root().join("frontend/tests/fixtures/themes.json"))
        .expect("frontend/tests/fixtures/themes.json must exist");
    // A three-line reader rather than a serde_json dependency: this crate's
    // whole value is that it compiles in seconds.
    let start = text.find("\"themes\"").expect("a themes key");
    let body = &text[start..];
    let end = body.find(']').unwrap_or(body.len());
    let mut out = Vec::new();
    let mut rest = &body[body.find('[').unwrap_or(0)..end];
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

#[test]
fn all_three_lists_were_actually_parsed() {
    // Any of them silently returning nothing would make the comparisons below
    // pass over empty sets.
    assert!(
        tailwind_themes().len() >= 14,
        "parsed only {:?} from tailwind.config.js",
        tailwind_themes()
    );
    assert!(
        server_themes().len() >= 14,
        "parsed only {:?} from update_user_theme",
        server_themes()
    );
    assert!(
        fixture_themes().len() >= 14,
        "parsed too few from themes.json"
    );
}

#[test]
fn the_server_accepts_exactly_the_themes_daisyui_compiles() {
    let tailwind = tailwind_themes();
    let server = server_themes();

    let refused: Vec<&String> = tailwind.iter().filter(|t| !server.contains(t)).collect();
    let phantom: Vec<&String> = server.iter().filter(|t| !tailwind.contains(t)).collect();

    assert!(
        refused.is_empty(),
        "daisyUI compiles these themes and `update_user_theme` rejects them with \
         a 400:\n{refused:?}\n\n\
         The picker offers whatever tailwind.config.js compiles, so each of \
         these is a button that produces an error when pressed."
    );
    assert!(
        phantom.is_empty(),
        "`update_user_theme` accepts these and daisyUI does not compile \
         them:\n{phantom:?}\n\n\
         Setting one succeeds and leaves the user on a theme with no CSS."
    );
}

#[test]
fn the_server_list_matches_the_audit_fixture_too() {
    // Not transitive by accident: the frontend suite compares the fixture to
    // tailwind.config.js and this compares the server to tailwind.config.js, so
    // asserting this pair directly is what makes the four-way agreement a
    // property rather than an inference across two test suites that can be run
    // separately.
    assert_eq!(
        server_themes(),
        fixture_themes(),
        "the server's accepted themes and the contrast audit's list disagree"
    );
}
