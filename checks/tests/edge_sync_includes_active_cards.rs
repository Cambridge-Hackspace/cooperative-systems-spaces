//! A device's sync allow-list must include a member's ACTIVE first-class cards,
//! and only the active ones (#33).
//!
//! `DatabaseManager::get_toolguard_sync_data` builds the per-device allow-list
//! the edges cache and enforce against offline (`edge_local`) and that the door
//! reader uses. Before #33 it read only the legacy `users.profile[field]`
//! value; a member whose credential lives in `user_cards` would be absent from
//! every edge, and their tools would never open when the server was unreachable.
//!
//! ## Why this is a source check rather than a behavioral one
//!
//! The runtime allow-list is served by `GET /api/toolguard/sync`, which requires
//! a registered device. A device is registered from an invite code, and an
//! invite code is eight emoji -- it needs a UTF-8 database, which only the
//! `utf8` reaper profile provides. The default battery's stack (SQL_ASCII) can
//! neither create an invite nor register a device, so the `cards` e2e stage
//! cannot reach `/sync`. That stage does exercise the same `user_cards` rows
//! behaviorally through the online tool-on path; what it cannot reach, and what
//! this file pins, is the SYNC builder itself: that it unions active `user_cards`
//! codes in and leaves disabled/released ones out.
//!
//! It slices the function body so the assertion is mutation-sensitive: the same
//! symbols appear in `resolve_card` earlier in the file, so a whole-file search
//! would pass even if the sync union were deleted.

use css_checks::read;

/// The body of `get_toolguard_sync_data`, from its signature to the next method.
fn sync_fn_body(src: &str) -> String {
    let start = src
        .find("pub fn get_toolguard_sync_data")
        .expect("pub fn get_toolguard_sync_data not found in database.rs");
    // Skip past this signature's own `pub` so the next `\n    pub fn ` is the
    // following method, not this one.
    let after_kw = start + "pub".len();
    let end = src[after_kw..]
        .find("\n    pub fn ")
        .map(|i| after_kw + i)
        .unwrap_or(src.len());
    src[start..end].to_string()
}

#[test]
fn sync_unions_active_user_cards_only() {
    let src = read("server/src/database.rs");
    let body = sync_fn_body(&src);

    assert!(
        body.contains("user_cards"),
        "the sync-payload builder must read user_cards, or a member whose card is \
         first-class (not a legacy profile value) is absent from every edge"
    );
    assert!(
        body.contains("CardStatus::Active"),
        "the sync builder must include ONLY active cards -- a disabled or released \
         card must never reach a device's offline allow-list"
    );
    assert!(
        body.contains("identifiers"),
        "the active card codes must be merged into the per-user identifier list \
         the sync payload is built from (each identifier becomes a sync-user row)"
    );
}
