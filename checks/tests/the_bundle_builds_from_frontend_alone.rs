//! The production build depends on nothing outside `frontend/`.
//!
//! `Dockerfile` copies `frontend/` and nothing else, so anything the build
//! reaches for beyond that directory is missing at image-build time. Nowhere
//! else notices: CI's `frontend` job has the whole repository checked out, and
//! the docker jobs only run on master/dev -- so the failure appears on a merge
//! to the integration branch and not before.
//!
//! It happened exactly that way. `build` was `vue-tsc && vite build`, which
//! uses `tsconfig.json`, and that includes `tests/**/*.ts`. So producing the
//! shipping bundle required the whole test suite to type-check -- and
//! `tests/components/CalendarEvents.spec.ts` imports
//! `../../../e2e/corpus/hostile.json`, the one hostile-input corpus shared by
//! the frontend specs and both e2e drivers. Deliberately one file read by two
//! languages, and deliberately outside `frontend/`.
//!
//! The fix was to give the build its own tsconfig covering `src/` only. The
//! tests are still type-checked by `type-check` and `type-check:strict`, which
//! CI runs; this narrows what the *build* checks, not what the project does.
//!
//! What this does NOT prove: that the image builds, or that `src/` itself
//! never reaches outside the directory. It pins the specific coupling that
//! cost a merge -- the build type-checking the test tree.

use css_checks::read;

fn package_json() -> String {
    read("frontend/package.json")
}

/// The value of a `"name": "value"` script entry.
fn script(name: &str) -> String {
    let source = package_json();
    let key = format!("\"{name}\":");
    let after = source
        .split(&key)
        .nth(1)
        .unwrap_or_else(|| panic!("frontend/package.json has no `{name}` script"));

    after
        .trim_start()
        .trim_start_matches('"')
        .split('"')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// The `include` array of a tsconfig, as raw entries.
///
/// Parsed rather than grepped, and the reason is a false positive this check
/// produced against itself: `tsconfig.build.json` explains in a comment *why*
/// it excludes `tests/`, and a search of the file text found that sentence and
/// reported the opposite of the truth. A check that reads prose as
/// configuration is worse than no check.
fn include_entries(file: &str) -> Vec<String> {
    let source = read(file);
    // Strip `//` comments; tsconfig is JSONC and these files carry prose.
    let stripped: String = source
        .lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n");

    let after = match stripped.split("\"include\"").nth(1) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let open = match after.find('[') {
        Some(i) => i,
        None => return Vec::new(),
    };
    let close = match after[open..].find(']') {
        Some(i) => open + i,
        None => return Vec::new(),
    };

    after[open + 1..close]
        .split(',')
        .map(|e| e.trim().trim_matches('"').to_string())
        .filter(|e| !e.is_empty())
        .collect()
}

#[test]
fn the_scripts_were_actually_parsed() {
    // Anti-vacuity: an empty script string would satisfy every `!contains`
    // assertion below.
    for name in ["build", "type-check"] {
        let value = script(name);
        assert!(
            value.len() > 5,
            "parsed `{name}` as {value:?}, which is too short to be a real \
             script -- package.json's shape changed and this check is no \
             longer reading it"
        );
    }
}

#[test]
fn the_build_does_not_type_check_the_test_tree() {
    let build = script("build");

    assert!(
        build.contains("tsconfig.build.json"),
        "`build` is {build:?}. Without an explicit project it uses \
         tsconfig.json, which includes `tests/**/*.ts` -- so the shipping \
         bundle cannot be produced unless the test suite type-checks, and the \
         specs import fixtures from outside frontend/ that the Dockerfile \
         never copies."
    );

    let include = include_entries("frontend/tsconfig.build.json");
    assert!(
        !include.is_empty(),
        "frontend/tsconfig.build.json has no `include`, so it inherits \
         tsconfig.json's -- which is the test tree this file exists to leave out"
    );
    assert!(
        !include.iter().any(|e| e.contains("tests")),
        "frontend/tsconfig.build.json includes the test tree again. It exists \
         precisely to exclude it; if the tests need to be in the build's \
         project for some reason, that reason has to be written down here."
    );
}

#[test]
fn the_tests_are_still_type_checked_somewhere() {
    // The other half of the split, and the thing that stops the fix above
    // becoming a way to stop checking the tests at all.
    let type_check = script("type-check");
    assert!(
        !type_check.contains("tsconfig.build.json"),
        "`type-check` now uses the build's project too, so nothing type-checks \
         the test suite. The split is only sound while one of these two still \
         covers `tests/`."
    );

    assert!(
        include_entries("frontend/tsconfig.json")
            .iter()
            .any(|e| e.contains("tests")),
        "frontend/tsconfig.json no longer includes `tests/`, and `type-check` \
         reads that file -- so the test suite is unchecked by any script CI \
         runs."
    );
}
