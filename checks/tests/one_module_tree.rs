//! `main.rs` is a shim. It must not declare modules the library already owns.
//!
//! Before the lib/bin split, `server/src/main.rs` declared all eighteen
//! modules itself. The split moved them to `lib.rs` -- which is what lets
//! `server/tests/*` exist at all, since an integration test can only reach a
//! library target. `main.rs` now reaches them through `css_server::`.
//!
//! The hazard is that both forms compile. If a merge brings a `mod config;`
//! back into `main.rs`, the bin gets a *second, independent* copy of that
//! module: same source file, distinct types. `css_server::config::AppConfig`
//! and `config::AppConfig` are then different types, and the error rustc
//! reports names the same path twice, which reads like a compiler fault rather
//! than a duplicated module. Worse, if the duplicated module has no boundary
//! with the library it can compile clean and silently run two copies of
//! whatever state it holds.
//!
//! This has been reachable twice on this branch, both times while merging a
//! branch whose `main.rs` predates the split. `dr-metrix-axum` is Linux-only,
//! so `css-server` cannot be compiled on the FreeBSD workstation where the
//! merges happen -- meaning the compiler, the only other thing that would
//! notice, does not run until a container build minutes later.
//!
//! So: text-level, no compiler, runs anywhere.
//!
//! What this does NOT prove: that `main.rs` imports everything it uses. That
//! failure is loud (E0433, immediately, on the next build) and needs a resolver
//! to check properly. This covers the quiet one.

use css_checks::{read, repo_root};

/// Modules the library owns, from `pub mod` declarations in `server/src/lib.rs`.
fn library_modules() -> Vec<String> {
    let source = read("server/src/lib.rs");
    let found: Vec<String> = source
        .lines()
        .filter_map(|l| l.trim().strip_prefix("pub mod "))
        .filter_map(|l| l.strip_suffix(';'))
        .map(str::to_string)
        .collect();

    // Anti-vacuity: a parse that found nothing would make every assertion
    // below pass over an empty set, which is exactly the shape this file
    // exists to stop elsewhere.
    assert!(
        found.len() >= 15,
        "parsed only {found:?} out of server/src/lib.rs -- the declaration \
         syntax changed and this file is no longer reading it"
    );
    found
}

/// `mod x;` declarations in a file, ignoring `mod x { .. }` inline modules and
/// `#[cfg(test)] mod tests;`, which are local by definition.
fn module_declarations(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .filter_map(|l| {
            l.strip_prefix("mod ")
                .or_else(|| l.strip_prefix("pub mod "))
        })
        .filter_map(|l| l.strip_suffix(';'))
        .filter(|m| *m != "tests")
        .map(str::to_string)
        .collect()
}

#[test]
fn main_declares_no_module_the_library_owns() {
    let owned = library_modules();
    let declared = module_declarations(&read("server/src/main.rs"));

    let duplicated: Vec<&String> = declared.iter().filter(|m| owned.contains(m)).collect();

    assert!(
        duplicated.is_empty(),
        "server/src/main.rs declares modules that server/src/lib.rs already \
         owns:\n{duplicated:?}\n\n\
         Both compile, and the bin then holds a second copy of each: same \
         source, distinct types. Reach them through `css_server::` instead. \
         If a merge brought these across from a branch that predates the \
         lib/bin split, the rest of that file's `mod` block probably came too."
    );
}

#[test]
fn main_stays_a_shim() {
    // The general form of the same rule. `main.rs` may declare nothing at all;
    // anything it needs is the library's. Stated separately from the test
    // above because that one only fires on a *collision* -- a brand-new module
    // declared only in `main.rs` would pass it while still being unreachable
    // from any integration test, which is the reason the split happened.
    let declared = module_declarations(&read("server/src/main.rs"));

    assert!(
        declared.is_empty(),
        "server/src/main.rs declares modules:\n{declared:?}\n\n\
         The bin is a shim. Code declared here cannot be reached by anything \
         in server/tests/, so it is unreachable from the contract tier and \
         every other integration test by construction. Move it to the library \
         and re-export it, or say here why it must live in the bin."
    );
}

#[test]
fn the_library_is_what_the_tests_import() {
    // The premise the two tests above defend. If the crate ever stops
    // producing a library target, they would both pass vacuously while every
    // integration test failed to build for an unrelated-looking reason.
    // `repo_root()`, not a bare relative path: a test binary's cwd is the
    // crate directory, so `Path::new("server/src/lib.rs")` is resolved from
    // `checks/` and is always absent -- which would have made this assertion
    // fail for a reason that has nothing to do with what it checks.
    assert!(
        repo_root().join("server/src/lib.rs").exists(),
        "server/ no longer has a library target, so server/tests/* cannot \
         import it"
    );

    let contract = read("server/tests/contract_matrix.rs");
    assert!(
        contract.contains("css_server::"),
        "server/tests/contract_matrix.rs no longer imports `css_server`, so \
         the lib/bin split these tests defend is not actually load-bearing \
         any more"
    );
}
