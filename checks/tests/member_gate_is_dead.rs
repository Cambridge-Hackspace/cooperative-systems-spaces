//! `MemberUser` is an authorization gate that guards nothing.
//!
//! `server/src/auth.rs` defines three role extractors — `AdminUser`,
//! `StaffUser` and `MemberUser` — each with its own `FromRequestParts` and its
//! own rejection. Two of them are used. `MemberUser` is used by no route at
//! all, so the entire "member or above" tier of the authorization model is
//! unexercised: it compiles, it has a test, and no request has ever gone
//! through it.
//!
//! That is worth a check rather than a deletion. The gate is not wrong, and a
//! route that ought to be member-only is more likely to appear than the
//! extractor is to be removed. What is wrong is that the fact is invisible:
//! `rustc` reports the *symptom* — `variant Member is never constructed` in the
//! contract tier's route table — which reads like a tidying job in a test
//! fixture rather than a statement about the product's authorization model.
//!
//! So the count is asserted. When somebody adds the first member-only route,
//! this fails and says what to do; until then it is a written record that the
//! tier is empty, in a place a person reading the authorization code will find.

use css_checks::repo_root;

/// Handler signatures that take a `MemberUser`, across the whole API.
fn member_gated_handlers() -> Vec<String> {
    let api = repo_root().join("server/src/api");
    let mut out = Vec::new();

    for entry in std::fs::read_dir(&api).expect("server/src/api must exist") {
        let path = entry.expect("readable").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&path) else {
            continue;
        };
        let file = path.file_name().unwrap().to_string_lossy().to_string();

        // `_x: MemberUser` or `_: crate::auth::MemberUser` in a parameter list.
        let mut from = 0usize;
        while let Some(rel) = src[from..].find("MemberUser") {
            let at = from + rel;
            from = at + "MemberUser".len();

            // Skip the import line and any comment.
            let line_start = src[..at].rfind('\n').map(|i| i + 1).unwrap_or(0);
            let line_end = src[at..].find('\n').map(|i| at + i).unwrap_or(src.len());
            let line = &src[line_start..line_end];
            let trimmed = line.trim_start();
            if trimmed.starts_with("use ") || trimmed.starts_with("//") {
                continue;
            }
            out.push(format!("{file}: {}", line.trim()));
        }
    }
    out.sort();
    out
}

/// The extractor exists, so a scan finding nothing is meaningful rather than broken.
#[test]
fn the_extractor_is_still_defined() {
    let auth = std::fs::read_to_string(repo_root().join("server/src/auth.rs"))
        .expect("server/src/auth.rs must exist");
    assert!(
        auth.contains("pub struct MemberUser"),
        "MemberUser no longer exists. If it was deleted deliberately, delete \
         this file too and the `Member` arm of the contract tier's Guard enum \
         with it."
    );
    assert!(
        auth.contains("can_access_member"),
        "MemberUser no longer checks can_access_member"
    );
}

#[test]
fn no_route_is_member_gated_yet() {
    let handlers = member_gated_handlers();

    assert!(
        handlers.is_empty(),
        "MemberUser now guards {} handler(s):\n{}\n\n\
         Good -- but three things have to move together, and this check exists \
         so they do:\n\
         1. add the route's row to server/tests/common/mod.rs with Guard::Member;\n\
         2. the 998-pair matrix will then assert it refuses every invalid \
            credential;\n\
         3. add a live case to the contract stage proving a Member is accepted \
            and a Newbie is refused with 403 -- the offline matrix cannot show \
            that, because accepting a credential means querying.\n\n\
         Then delete this test.",
        handlers.len(),
        handlers.join("\n")
    );
}

#[test]
fn the_route_table_still_knows_about_the_gate() {
    // The Guard enum keeps its `Member` arm even though nothing constructs it,
    // and `checks/tests/route_table_matches.rs` still maps the extractor to it.
    // If either were removed, the first member-gated route added would be
    // classified as something else -- most likely `Public` -- and the matrix
    // would assert that it is reachable without a credential.
    let table = std::fs::read_to_string(repo_root().join("server/tests/common/mod.rs"))
        .expect("the route table must exist");
    assert!(
        table.contains("    Member,"),
        "the contract tier's Guard enum lost its Member arm; the first \
         member-gated route added would be classified as Public"
    );

    let derived = std::fs::read_to_string(repo_root().join("checks/tests/route_table_matches.rs"))
        .expect("route_table_matches.rs must exist");
    assert!(
        derived.contains("(\"MemberUser\", \"Member\")"),
        "the derived inventory no longer maps MemberUser to Guard::Member"
    );
}
