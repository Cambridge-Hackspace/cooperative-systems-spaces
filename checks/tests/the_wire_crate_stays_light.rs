//! `css_wire` depends on serde and nothing else.
//!
//! The crate exists so that both ends of the local broker share one definition
//! of what goes over it (#219), and one of those ends is firmware: spaces-firmware
//! depends on this crate by tag and builds it for an ESP32. A dependency that is
//! natural here -- `serde_json` for a helper, `chrono` for a timestamp, a crypto
//! crate -- compiles fine in this workspace and breaks the firmware's build in
//! another repository, where nobody here will see it fail.
//!
//! So the rule is checked rather than remembered: the `[dependencies]` table of
//! `css_wire/Cargo.toml` names serde alone. Test-only dependencies are free.
//!
//! Text-level, like its neighbours: no compiler needed.

use css_checks::read;

const MANIFEST: &str = "css_wire/Cargo.toml";

/// What `css_wire` may depend on outside its tests.
const ALLOWED: &[&str] = &["serde"];

/// The dependency names in one `[table]` of a manifest, in order.
///
/// Reads `name = ...` lines from the table's header to the next header. Enough
/// for this manifest, which is short and plain; it does not try to be a TOML
/// parser, and the self-tests below drive it on the shapes that matter.
fn table_keys(manifest: &str, table: &str) -> Vec<String> {
    let header = format!("[{table}]");
    let mut inside = false;
    let mut keys = Vec::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            inside = line == header;
            continue;
        }
        if !inside || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, _)) = line.split_once('=') {
            keys.push(key.trim().to_string());
        }
    }
    keys
}

#[test]
fn the_wire_crate_depends_on_serde_alone() {
    let manifest = read(MANIFEST);
    let deps = table_keys(&manifest, "dependencies");
    assert!(
        !deps.is_empty(),
        "no [dependencies] found in {MANIFEST}; the table moved or was renamed, \
         and this check would pass on anything"
    );
    let extra: Vec<&String> = deps
        .iter()
        .filter(|d| !ALLOWED.contains(&d.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "{MANIFEST} depends on {extra:?} beyond {ALLOWED:?}. css_wire is built by \
         the firmware for an ESP32 (spaces-firmware depends on it by tag); put \
         anything heavier in css_lib, which re-exports this crate."
    );
}

#[test]
fn an_added_dependency_is_caught() {
    let manifest = read(MANIFEST).replace(
        "[dependencies]\n",
        "[dependencies]\nchrono = { workspace = true }\n",
    );
    let deps = table_keys(&manifest, "dependencies");
    assert!(
        deps.iter().any(|d| d == "chrono"),
        "the mutant did not apply; the manifest's text has changed and this \
         self-test no longer drives the check"
    );
}

#[test]
fn test_only_dependencies_are_not_counted() {
    let manifest = "[dependencies]\nserde = \"1\"\n\n[dev-dependencies]\nserde_json = \"1\"\n";
    assert_eq!(table_keys(manifest, "dependencies"), vec!["serde"]);
    assert_eq!(table_keys(manifest, "dev-dependencies"), vec!["serde_json"]);
}
