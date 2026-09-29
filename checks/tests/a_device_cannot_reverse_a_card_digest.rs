//! A device holds enough to *recognize* a card and not enough to *name* one (#109).
//!
//! Before #109 the ToolGuard sync payload carried members' card codes in the
//! clear, and an edge wrote them to disk. A plug unscrewed from a wall yielded
//! the credential of every member authorized to use it -- and #108's
//! encryption-at-rest bought nothing against that, because the plaintext was
//! being handed out the front door.
//!
//! The fix is a digest, and the *shape* of the fix is what this file pins. The
//! server keeps three keys; a device is given exactly one of them, a pepper,
//! and [`CardDigester`] is the type that holds only that. From the pepper you
//! can recompute a digest from a swipe -- which is the whole job -- and you
//! cannot decrypt a stored card, cannot compute a blind index, and cannot
//! invert a digest in less than a multi-year argon2 campaign.
//!
//! **Why a source check and not a behavioral one.** The e2e `toolmodules`
//! stage asserts what crosses the wire: the payload carries a 32-byte hex
//! digest and not the code the fixture issued. That is the right oracle for
//! the wire, and it is also blind to every regression this file exists to
//! catch, because all of them leave the wire format untouched:
//!
//!   * hand the edge a `CardCipher` instead of a `CardDigester` -- say, because
//!     a kiosk screen wants to show a card number -- and the sync payload still
//!     carries digests. The e2e stage stays green. The plug on the wall now
//!     holds the key that decrypts the database.
//!   * give `CardDigester` a second field. Same.
//!
//! Both are plausible edits made for a good reason by someone who has not read
//! this comment, and each is separately sufficient to undo the issue.
//!
//! A third downgrade belongs to the same family -- swapping `wire_digest`'s
//! argon2 for the HMAC that sits ten lines away in the same file, which is
//! byte-identical on the wire and drops the 2^32 search behind a digest from
//! years to seconds. That exact swap is caught one tier cheaper, without a grep,
//! by `css_lib`'s `the_wire_digest_is_not_a_bare_hmac_of_the_code`.
//!
//! A fourth is closely related and used to be uncovered: argon2 kept but its
//! `Params` weakened -- the memory cost lowered, most temptingly to fit a
//! microcontroller that cannot spare 19 MiB (#146). The wall-plug threat is
//! identical and lowering the cost forfeits the very "2^32 is expensive"
//! property the digest exists for, so it is pinned here by
//! `the_wire_digest_argon2_params_are_not_weakened`.
//!
//! What this does NOT prove: that the digest actually reaches the device, or
//! that a swipe matches it. That is the `toolmodules` stage under
//! `--profile utf8` -- the default LATIN1 cluster cannot register a device at
//! all, so it skips exactly the assertions that would notice.

use css_checks::read;

/// Edge sources, with each file's `#[cfg(test)]` tail cut off.
///
/// Truncating at the first `#[cfg(test)]` can only discard real code and make
/// this check stricter, never laxer; house style puts test modules last. It
/// matters here because `edge/src/toolguard.rs`'s own tests legitimately
/// construct key material.
fn edge_production_sources() -> Vec<(&'static str, String)> {
    const FILES: &[&str] = &[
        "edge/src/main.rs",
        "edge/src/mqtt.rs",
        "edge/src/edge_inbound.rs",
        "edge/src/ws.rs",
        "edge/src/power.rs",
        "edge/src/doors.rs",
        "edge/src/toolguard.rs",
        "edge/src/modules.rs",
        "edge/src/config.rs",
        "edge/src/web_server.rs",
        "edge/src/lib.rs",
    ];
    FILES
        .iter()
        .map(|f| (*f, production_only(&read(f))))
        .collect()
}

/// Everything before the first `#[cfg(test)]`.
fn production_only(src: &str) -> String {
    match src.find("#[cfg(test)]") {
        Some(i) => src[..i].to_string(),
        None => src.to_string(),
    }
}

/// Occurrences of `name` that are code rather than prose.
///
/// The distinction is load-bearing and the naive search gets it backwards:
/// `edge/src/toolguard.rs` says, in a doc comment, "a `CardDigester` and
/// deliberately not a `CardCipher`" -- a bare substring search for `CardCipher`
/// is satisfied by the sentence explaining why it is absent, so the check would
/// fail on the fixed tree and pass on a tree where someone deleted the comment
/// along with the property.
///
/// House style backticks every type and function name in prose, so an
/// occurrence adjacent to a backtick on either side is prose. Anything else is
/// counted. That errs toward counting: an un-backticked prose mention trips
/// this check, and the fix is to backtick it, which the style wanted anyway.
fn code_mentions(src: &str, name: &str) -> usize {
    src.match_indices(name)
        .filter(|(i, _)| {
            let before = src[..*i].chars().next_back();
            let after = src[i + name.len()..].chars().next();
            before != Some('`') && after != Some('`')
        })
        .count()
}

/// Files whose production half uses `name` as code.
fn users_of(sources: &[(&'static str, String)], name: &str) -> Vec<&'static str> {
    sources
        .iter()
        .filter(|(_, src)| code_mentions(src, name) > 0)
        .map(|(f, _)| *f)
        .collect()
}

/// The field names declared in `struct <name>`, from its opening brace to the
/// matching close.
///
/// Brace-counted rather than delimited by the next item, because a struct with
/// one field is small enough that a blunt scan would run past it into the impl.
fn struct_fields(src: &str, name: &str) -> Vec<String> {
    let decl = format!("struct {name} ");
    let start = src
        .find(&decl)
        .unwrap_or_else(|| panic!("`struct {name}` not found"));
    let open = start
        + src[start..]
            .find('{')
            .unwrap_or_else(|| panic!("`struct {name}` has no body"));
    let mut depth = 0usize;
    let mut close = open;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = open + i;
                    break;
                }
            }
            _ => {}
        }
    }
    src[open + 1..close]
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .filter_map(|l| l.split(':').next())
        .map(|f| f.trim_start_matches("pub ").trim().to_string())
        .filter(|f| !f.is_empty())
        .collect()
}

/// The text inside the first brace-balanced `{ … }` in `src`.
fn brace_body(src: &str) -> String {
    let open = src
        .find('{')
        .unwrap_or_else(|| panic!("no opening brace in the given text"));
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return src[open + 1..open + i].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces");
}

/// The body of the `wire_digest` that actually builds argon2.
///
/// `card_crypto.rs` has two: `CardDigester::wire_digest` (the argon2 one this
/// file cares about) and `CardCipher::wire_digest` (which just delegates to it).
/// The delegating one holds no `Argon2`, so selecting on that is what tells them
/// apart without depending on which is declared first.
fn wire_digest_argon2_body(src: &str) -> String {
    for (i, _) in src.match_indices("fn wire_digest") {
        let body = brace_body(&src[i..]);
        if body.contains("Argon2") {
            return body;
        }
    }
    panic!("no `fn wire_digest` in card_crypto.rs builds argon2");
}

// ── The properties ───────────────────────────────────────────────────────────

#[test]
fn the_edge_holds_no_key_that_could_reverse_a_digest() {
    let sources = edge_production_sources();

    for forbidden in [
        "CardCipher",
        "blind_index",
        "card_encryption_key",
        "card_index_key",
    ] {
        let found = users_of(&sources, forbidden);
        assert!(
            found.is_empty(),
            "edge production code uses `{forbidden}` in {found:?}. An edge runs on a \
             plug screwed to a wall in a shared workshop; whatever it holds is \
             recoverable by anyone who can reach it with a screwdriver. The \
             encryption key decrypts every card in the database and the index key \
             recovers them by search -- neither may ever be shipped to a device. \
             The pepper in `CardDigester` is the only card key an edge gets, and it \
             buys recognition without recovery."
        );
    }
}

#[test]
fn the_digester_carries_nothing_but_the_pepper() {
    let fields = struct_fields(&read("css_lib/src/card_crypto.rs"), "CardDigester");

    assert_eq!(
        fields,
        vec!["device_pepper".to_string()],
        "`CardDigester` must hold the device pepper and nothing else. It is the \
         type an edge is handed, and its value is entirely in what it CANNOT do: \
         a second field carrying a cipher or the index key would leave every \
         wire-format assertion green while handing a wall plug the keys to the \
         card table."
    );
}

#[test]
fn the_wire_digest_argon2_params_are_not_weakened() {
    let body = wire_digest_argon2_body(&read("css_lib/src/card_crypto.rs"));

    assert!(
        body.contains("Params::default()"),
        "`CardDigester::wire_digest` must build argon2 with `Params::default()`. That is the \
         memory/time cost the whole scheme leans on: a card is a 4-byte UID (2^32), a device \
         holds the pepper and the digests of its own members, and only a costly KDF turns \
         recovering the codes behind those digests into a multi-year campaign instead of \
         seconds (#109). If you are changing this deliberately -- to STRENGTHEN it -- update \
         this assertion in the same commit and say why."
    );
    assert!(
        !body.contains("Params::new"),
        "`CardDigester::wire_digest` builds its argon2 with `Params::new(...)`, which is how \
         the cost gets lowered -- most temptingly to fit a microcontroller that cannot spare \
         19 MiB (#146). Lowering it forfeits exactly the 2^32-is-expensive property the digest \
         exists for. A small reader must not hash at all: it reports the card to an edge that \
         does, or it runs online (see FIRMWARE.md). Keep `Params::default()`."
    );
}

// ── Self-tests ───────────────────────────────────────────────────────────────
//
// Every property above is a grep over source text, and a grep is the kind of
// check that passes for the wrong reason. Each way these could is pinned here,
// against the broken world they exist to reject.

#[cfg(test)]
mod the_check_rejects_what_it_is_for {
    use super::*;

    /// The trap this file had to be written around: the fixed tree explains, in
    /// prose, the very thing it forbids.
    #[test]
    fn a_backticked_mention_in_a_comment_is_not_a_use() {
        let src = "/// A `CardDigester` and deliberately not a `CardCipher`.\n\
                   pub struct S { d: CardDigester }";
        assert_eq!(
            code_mentions(src, "CardCipher"),
            0,
            "the doc comment that explains the absence must not be mistaken for \
             the presence -- otherwise this check fails on the fixed tree"
        );
    }

    #[test]
    fn an_actual_use_is_seen_through_the_comment_that_denies_it() {
        // The regression's real shape: the comment survives the edit.
        let src = "/// A `CardDigester` and deliberately not a `CardCipher`.\n\
                   pub struct S { c: Arc<CardCipher> }";
        assert_eq!(
            code_mentions(src, "CardCipher"),
            1,
            "a genuine field must be counted even when a comment above it claims \
             it is not there; the comment is what a careless edit leaves behind"
        );
    }

    #[test]
    fn a_use_that_exists_only_in_tests_does_not_count() {
        let src = "pub fn run() {}\n#[cfg(test)]\nmod t { fn f() { CardCipher::from_hex(k); } }";
        assert_eq!(
            code_mentions(&production_only(src), "CardCipher"),
            0,
            "an edge's own unit tests may construct whatever they need; it is the \
             shipped binary that must not hold the key"
        );
    }

    #[test]
    fn a_second_field_on_the_digester_is_noticed() {
        let src = "pub struct CardDigester {\n    device_pepper: [u8; KEY_LEN],\n    \
                   index_key: [u8; KEY_LEN],\n}\n";
        assert_eq!(
            struct_fields(src, "CardDigester"),
            vec!["device_pepper".to_string(), "index_key".to_string()],
            "the field scan must see the smuggled key, or the assertion above is \
             decoration"
        );
    }

    /// And it must read the real struct correctly, or the test above proves
    /// only that a fake string can be parsed.
    #[test]
    fn the_real_digester_parses_to_one_field() {
        let fields = struct_fields(&read("css_lib/src/card_crypto.rs"), "CardDigester");
        assert_eq!(fields.len(), 1, "got {fields:?}");
    }

    #[test]
    fn a_weakened_params_new_is_rejected() {
        // The regression the params check exists for: argon2 kept, cost lowered
        // via `Params::new` to fit a small device.
        let src = "pub fn wire_digest(&self) {\n    \
                   let a = Argon2::new_with_secret(&p, Algorithm::Argon2id, \
                   Version::V0x13, Params::new(64, 1, 1, None));\n}\n";
        let body = wire_digest_argon2_body(src);
        assert!(
            body.contains("Params::new"),
            "the weakening must be visible to the check"
        );
        assert!(
            !body.contains("Params::default()"),
            "and it must NOT read as the default, or the check passes on the weakened tree"
        );
    }

    #[test]
    fn the_default_params_body_passes() {
        let src = "pub fn wire_digest(&self) {\n    \
                   let a = Argon2::new_with_secret(&p, Algorithm::Argon2id, \
                   Version::V0x13, Params::default());\n}\n";
        let body = wire_digest_argon2_body(src);
        assert!(body.contains("Params::default()"));
        assert!(!body.contains("Params::new"));
    }

    #[test]
    fn the_argon2_body_is_the_digester_not_the_delegating_wire_digest() {
        // Two `wire_digest`s exist; the selector must land on the argon2 one even
        // when the delegating one is declared first.
        let src = "impl A { pub fn wire_digest(&self) { self.d.wire_digest(c) } }\n\
                   impl B { pub fn wire_digest(&self) { \
                   Argon2::new_with_secret(&p, X, Y, Params::default()); } }\n";
        let body = wire_digest_argon2_body(src);
        assert!(body.contains("Argon2") && body.contains("Params::default()"));
        assert!(
            !body.contains("self.d.wire_digest"),
            "must skip the delegating one"
        );
    }
}
