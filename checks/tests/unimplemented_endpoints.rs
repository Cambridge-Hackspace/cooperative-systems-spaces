//! Routes that are registered and can never succeed.
//!
//! Four handlers under `server/src/api/tools.rs` return
//! `ApiError::NotImplemented`, which is a 501. The routes are registered, the
//! frontend can call them, and they answer "not yet implemented" every time.
//!
//! A 501 is an honest answer — far better than a 500, and better than a route
//! that silently does nothing. What is not honest is leaving the list
//! undocumented, because from outside the codebase a registered route is a
//! promise. The training UI calls two of these; nobody looking at the
//! frontend can tell they are stubs.
//!
//! So the list is pinned. Adding a stub is a deliberate act that has to be
//! written down here; finishing one fails this test, which is the moment to
//! delete its entry.
//!
//! The seeded fuzz tier flags them too — its no-5xx oracle sees 501 as a 5xx,
//! correctly, and its KNOWN list carries them with the same reasoning. This is
//! the version that costs nothing to run and does not depend on the fuzzer
//! happening to reach the endpoint.

use css_checks::{read, repo_root};

/// `(file, handler name, the message it returns)`.
fn unimplemented_handlers() -> Vec<(String, String, String)> {
    let api = repo_root().join("server/src/api");
    let mut out = Vec::new();

    for entry in std::fs::read_dir(&api).expect("server/src/api must exist") {
        let path = entry.expect("readable").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let file = path.file_name().unwrap().to_string_lossy().to_string();
        if file == "errors.rs" {
            // Where the variant is defined and where its status is asserted.
            continue;
        }
        let src = std::fs::read_to_string(&path).unwrap_or_default();

        let mut from = 0usize;
        while let Some(rel) = src[from..].find("ApiError::NotImplemented(") {
            let at = from + rel;
            from = at + 1;

            // The message, which is what a caller sees.
            let after = &src[at..];
            let message = after
                .find('"')
                .and_then(|o| {
                    let rest = &after[o + 1..];
                    rest.find('"').map(|c| rest[..c].to_string())
                })
                .unwrap_or_default();

            // The enclosing handler: the nearest `fn` above.
            let before = &src[..at];
            let handler = before
                .rmatch_indices("fn ")
                .next()
                .map(|(i, _)| {
                    before[i + 3..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect::<String>()
                })
                .unwrap_or_default();

            out.push((file.clone(), handler, message));
        }
    }
    out.sort();
    out
}

/// Every stub, named, with the route it is registered at.
///
/// Written out rather than derived, so that the list a reader sees is a
/// statement somebody made rather than a description of whatever the code
/// currently does.
const KNOWN_STUBS: &[(&str, &str, &str)] = &[
    (
        "tools.rs",
        "create_training_type",
        "POST /api/tools/{tool_id}/training-types",
    ),
    (
        "tools.rs",
        "authorize_trainer",
        "POST /api/tools/{tool_id}/trainers",
    ),
    (
        "tools.rs",
        "complete_training",
        "POST /api/tools/user-training/{training_id}",
    ),
    (
        "tools.rs",
        "revoke_training",
        "DELETE /api/tools/user-training/{training_id}",
    ),
];

#[test]
fn the_scan_found_the_stubs() {
    let found = unimplemented_handlers();
    assert!(
        !found.is_empty(),
        "found no NotImplemented handlers at all. Either they were all finished \
         -- delete this file -- or the scan is broken and the assertion below \
         would pass over nothing."
    );
}

#[test]
fn the_list_of_unimplemented_endpoints_has_not_changed() {
    let found = unimplemented_handlers();
    let actual: Vec<String> = found.iter().map(|(f, h, _)| format!("{f}::{h}")).collect();
    let expected: Vec<String> = KNOWN_STUBS
        .iter()
        .map(|(f, h, _)| format!("{f}::{h}"))
        .collect();

    let added: Vec<&String> = actual.iter().filter(|a| !expected.contains(a)).collect();
    let finished: Vec<&String> = expected.iter().filter(|e| !actual.contains(e)).collect();

    assert!(
        added.is_empty(),
        "these handlers now return 501 and are not in the list:\n{added:#?}\n\n\
         A registered route that can never succeed is a promise from outside the \
         codebase -- the frontend cannot tell a stub from a working endpoint. \
         Adding one is fine and has to be written down here, with the route it \
         is registered at."
    );

    assert!(
        finished.is_empty(),
        "these handlers no longer return 501 -- they were implemented:\n{finished:#?}\n\n\
         Good. Delete their entries from KNOWN_STUBS, and from the KNOWN list in \
         e2e/drivers/fuzz.mjs if they are named there."
    );
}

#[test]
fn every_stub_says_what_it_is() {
    // A 501 with an empty body is a 501 nobody can act on. Each of these
    // carries a message naming the feature, which is the difference between
    // "this is not built yet" and "something went wrong".
    for (file, handler, message) in unimplemented_handlers() {
        assert!(
            message.len() > 10,
            "{file}::{handler} returns 501 with the message {message:?}, which \
             tells a caller nothing about which feature is missing"
        );
        assert!(
            message.to_lowercase().contains("not yet implemented")
                || message.to_lowercase().contains("not implemented"),
            "{file}::{handler} returns 501 with {message:?} -- say that it is \
             unimplemented, so the reader is not left wondering whether it is a \
             transient failure"
        );
    }
}

#[test]
fn the_frontend_knows_which_calls_are_stubs() {
    // It does not, and this records that rather than asserting it.
    //
    // The training UI calls two of these four. Nothing in `api.ts` marks them,
    // and `.catch` turns the 501 into the same generic "Failed to ..." every
    // other failure produces -- so a user is told the operation failed and a
    // developer reading the frontend cannot tell a stub from a bug.
    //
    // Asserted as a count so that wiring any of them up, or marking them in the
    // client, fails here and prompts this note to be revised.
    let api = read("frontend/src/utils/api.ts");
    let mentioned = KNOWN_STUBS
        .iter()
        .filter(|(_, _, route)| {
            let path = route.split(' ').nth(1).unwrap_or("");
            // The client builds these paths with interpolation, so compare on
            // the stable prefix before the first parameter.
            let prefix = path.split("/{").next().unwrap_or(path);
            let prefix = prefix.trim_start_matches("/api");
            !prefix.is_empty() && api.contains(prefix)
        })
        .count();

    assert_eq!(
        mentioned, 2,
        "the number of unimplemented endpoints the frontend calls has changed \
         (was 2). Either one was implemented, one was removed from the client, \
         or a new stub is now being called. Whichever it is, the note in this \
         test needs updating: the frontend has no way to tell a 501 stub from a \
         real failure, because api.ts wraps every call in .catch and produces \
         the same generic message for both."
    );
}
