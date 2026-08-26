//! `test-support` must never reach a shipped binary.
//!
//! The feature exposes `DatabaseManager::disconnected()`, a constructor that
//! builds a connection pool without opening a connection. That is exactly what
//! the contract tier needs and exactly what production must never have: a
//! server that started with it would appear healthy and fail every query.
//!
//! Cargo features are additive and unify across a build, so a single
//! `[dependencies]` entry anywhere in the workspace turning it on would enable
//! it for the binary too — silently, and with no compiler error to notice. This
//! is the check that says so.

use css_checks::read;

/// Lines that enable `test-support` on a dependency that ships.
///
/// Split out so it can be tested directly. This guard cannot be
/// mutation-checked the usual way — by breaking the thing it watches — because
/// every mutation that would trip it requires a manifest cargo refuses to load
/// (a crate depending on itself outside `[dev-dependencies]` is a cycle), so
/// the test binary would not build to report the failure. The synthetic cases
/// at the bottom of this file are the substitute, and they are named as such
/// rather than left implied.
fn shipped_enablements(manifest: &str) -> Vec<&str> {
    // Only dependency *enablement* counts. The `[features]` section declaring
    // `test-support = []` is not enabling it, and an earlier version of this
    // check flagged that declaration -- which would have had to be weakened to
    // pass, and that is how a check ends up asserting nothing.
    manifest
        .split("[dev-dependencies]")
        .next()
        .unwrap_or(manifest)
        .lines()
        .filter(|l| l.contains("test-support") && l.contains("features"))
        .collect()
}

#[test]
fn the_feature_exists_and_is_not_a_default() {
    let manifest = read("server/Cargo.toml");

    assert!(
        manifest.contains("test-support = []"),
        "server/Cargo.toml no longer declares the `test-support` feature. If it \
         was renamed, this check needs to follow it; if it was removed, so \
         should DatabaseManager::disconnected()."
    );

    // A `default = [...]` list that mentions it, in any spacing.
    let defaults: String = manifest
        .lines()
        .filter(|l| l.trim_start().starts_with("default"))
        .collect();
    assert!(
        !defaults.contains("test-support"),
        "`test-support` is in the default feature set, so every ordinary build \
         of css-server now contains a constructor that fabricates a dead \
         database pool: {defaults}"
    );
}

#[test]
fn no_ordinary_dependency_turns_it_on() {
    // Cargo features unify: if any *non-dev* dependency edge enables
    // test-support, the binary gets it as well, with nothing to warn you.
    for manifest in [
        "server/Cargo.toml",
        "cli/Cargo.toml",
        "edge/Cargo.toml",
        "css_lib/Cargo.toml",
        "checks/Cargo.toml",
        "kiosk/Cargo.toml",
        "Cargo.toml",
    ] {
        let src = read(manifest);

        // Only dependency *enablement* counts. The `[features]` section
        // declaring `test-support = []` is not enabling it, and an earlier
        // version of this check flagged that declaration and would have had to
        // be weakened to pass -- which is how a check ends up asserting
        // nothing. What matters is a `features = [.., "test-support", ..]` on a
        // dependency entry that is not a dev-dependency.
        let offenders = shipped_enablements(&src);

        assert!(
            offenders.is_empty(),
            "{manifest} enables `test-support` on a non-dev dependency; cargo \
             unifies features, so this reaches the shipped binary: {offenders:?}"
        );
    }
}

#[test]
fn the_constructor_is_gated() {
    let src = read("server/src/database.rs");
    let idx = src
        .find("pub fn disconnected()")
        .expect("DatabaseManager::disconnected() should exist; the contract tier depends on it");

    // The gate must be within the preceding impl block, not merely somewhere in
    // the file.
    let preceding = &src[..idx];
    let gate = preceding
        .rfind(r#"#[cfg(any(test, feature = "test-support"))]"#)
        .expect("disconnected() must be behind #[cfg(any(test, feature = \"test-support\"))]");
    let impl_start = preceding
        .rfind("impl DatabaseManager")
        .expect("disconnected() should live in an impl DatabaseManager block");

    assert!(
        gate < impl_start,
        "the cfg gate must sit on the impl block that contains disconnected(); \
         as written the constructor is compiled unconditionally"
    );
}

#[test]
fn the_guard_itself_catches_what_it_claims_to() {
    // A dependency enabling it, before [dev-dependencies]: must be caught.
    assert_eq!(
        shipped_enablements(
            "[dependencies]\nfoo = { version = \"1\", features = [\"test-support\"] }\n"
        )
        .len(),
        1
    );

    // The feature's own declaration: must NOT be caught.
    assert!(shipped_enablements("[features]\ntest-support = []\n").is_empty());

    // A dev-dependency enabling it: must NOT be caught, that is the supported
    // arrangement.
    assert!(shipped_enablements(
        "[features]\ntest-support = []\n\n[dev-dependencies]\n         css-server = { path = \".\", features = [\"test-support\"] }\n"
    )
    .is_empty());
}
