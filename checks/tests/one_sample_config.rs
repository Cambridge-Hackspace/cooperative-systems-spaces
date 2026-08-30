//! There is exactly one sample configuration, and it is the one that is tested.
//!
//! The repository shipped two: `config.sample.toml` (378 lines, referenced by
//! the Dockerfile and by the README) and `sample-config.toml` (170 lines, from
//! the initial commit, referenced by nothing). They had drifted -- the shorter
//! one predated `[calendar]`, `[pages]` and `[edge]` entirely.
//!
//! Two samples is worse than either one alone. Whichever a new deployment
//! copies, the odds are even it copies the stale one, and the failure that
//! produces is `AppConfig::from_file`'s missing-field path: the file is
//! rewritten with defaults and the server refuses to start -- handing the
//! operator `PagesConfig::default()`'s live GitHub URLs on the way.
//!
//! `sample-config.toml` was deleted. This is what stops a third appearing.
//! `the_shipped_sample_config_parses` in `server/src/config.rs` asserts the
//! survivor actually loads. This only asserts there is one of it, because that
//! check needs no `AppConfig` and therefore runs in seconds without the server
//! crate -- which is the whole reason the checks crate has no dependency on it.

use css_checks::repo_root;

#[test]
fn exactly_one_sample_configuration_is_tracked() {
    let root = repo_root();
    let mut samples = Vec::new();

    for entry in std::fs::read_dir(&root).expect("the repository root is readable") {
        let path = entry.expect("a readable entry").path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".toml") && lower.contains("sample") {
            samples.push(name.to_string());
        }
    }
    samples.sort();

    assert_eq!(
        samples,
        vec!["config.sample.toml".to_string()],
        "the repository root holds {samples:?}. Exactly one sample \
         configuration should exist, because a new deployment copies whichever \
         one it finds first and a stale copy is missing whole sections -- which \
         sends the very first boot down AppConfig::from_file's rewrite path."
    );
}

#[test]
fn the_sample_is_not_a_working_configuration() {
    // `server/config.toml` is tracked and is somebody's working configuration.
    // The sample must not become a second one: an operator who copies it should
    // get placeholders they are forced to change, not credentials that happen
    // to work somewhere.
    let text = std::fs::read_to_string(repo_root().join("config.sample.toml"))
        .expect("config.sample.toml must exist");

    for (needle, why) in [
        ("localhost:5432", "a database the reader does not have"),
        ("127.0.0.1:5432", "a database the reader does not have"),
    ] {
        assert!(
            !text.contains(needle),
            "the sample points at {needle} -- {why}. A sample that half-works is \
             harder to notice than one that does not work at all."
        );
    }

    // And the secrets are placeholders that say so, rather than blank fields a
    // reader could take for "no secret needed".
    assert!(
        text.contains("CHANGE THIS IN PRODUCTION"),
        "the sample no longer marks its secrets as placeholders"
    );
}
