//! Every tool the suite asserts it needs under `--provision=external` is a tool
//! CI actually installs.
//!
//! Preflight knows what the external path requires: it checks `command -v` and
//! records a `tool/<name>` case, so a missing one is named rather than
//! discovered later as something that reads like a product bug. CI installs
//! packages in an entirely separate file. Nothing connected the two, and that
//! gap is not hypothetical -- it is why this file exists.
//!
//! The `mqttloss` stage publishes with `mosquitto_pub`. That binary ships in
//! `mosquitto-clients`, a *different* package from the `mosquitto` broker, and
//! CI installed only the broker. Under container provisioning the publish runs
//! inside the broker's own image, so every local run and every reaper session
//! passed. CI failed with
//!
//! ```text
//! FAIL  mqttloss/heartbeat-published-before-outage -- mqtt_pub failed with the broker up
//! ```
//!
//! in a stage about broker outages -- which reads as the server having broken,
//! and cost a full CI cycle to find out otherwise.
//!
//! The same shape as `ci_runs_scripts_that_exist`: a workflow that encodes an
//! intention rather than a fact, with nothing to contradict it.
//!
//! The tool -> package mapping is duplicated here deliberately. It is a
//! judgment (`mosquitto_pub` comes from `mosquitto-clients`; `psql` from
//! `postgresql-client`) and there is nowhere to derive it from, so writing it
//! down twice and comparing is the check. Adding a tool to preflight without
//! deciding which package provides it now fails here rather than in CI.
//!
//! What this does NOT prove: that the packages install, that the binaries work,
//! or that a tool preflight does *not* assert is unnecessary. It proves the two
//! lists agree about the external path.

use css_checks::read;

/// Tools preflight requires that are NOT expected from an apt package: shell
/// builtins and coreutils present on any runner, plus toolchain-provided
/// binaries CI installs by other means (actions, not apt).
const ASSUMED_PRESENT: &[&str] = &[
    "sed", "grep", "awk", "tr", "sort", "comm", "install", "find", "openssl", "git",
];

/// The deliberate duplicate: which apt package provides which binary.
const PROVIDED_BY: &[(&str, &str)] = &[
    ("mosquitto", "mosquitto"),
    ("mosquitto_pub", "mosquitto-clients"),
    ("psql", "postgresql-client"),
];

fn run_sh() -> String {
    read("e2e/run.sh")
}

fn workflow() -> String {
    read(".github/workflows/css-ci.yml")
}

/// The `tool/<name>` cases preflight records, with `//`-comments stripped so
/// prose naming a tool cannot satisfy the check by mentioning it.
fn required_tools(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in source.lines() {
        let code = match line.find('#') {
            Some(i) => &line[..i],
            None => line,
        };
        let Some(i) = code.find("record_case \"tool/") else {
            continue;
        };
        let rest = &code[i + "record_case \"tool/".len()..];
        let Some(end) = rest.find('"') else { continue };
        let name = rest[..end].trim();
        // The generic loop records `tool/${t}`; its members are the literal
        // list in the loop header, handled by ASSUMED_PRESENT.
        if name.starts_with("${") || name.is_empty() {
            continue;
        }
        if !out.iter().any(|n: &String| n == name) {
            out.push(name.to_string());
        }
    }
    out
}

/// Everything the workflow hands to `apt-get install`, across continuations.
fn apt_packages(yaml: &str) -> Vec<String> {
    let mut pkgs = Vec::new();
    let mut collecting = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t.contains("apt-get install") {
            collecting = true;
            // Anything on the same line after the flags.
            for w in t.split_whitespace() {
                if !w.starts_with('-') && !w.contains("apt-get") && w != "install" && w != "sudo" {
                    pkgs.push(w.trim_end_matches('\\').to_string());
                }
            }
            if !t.ends_with('\\') {
                collecting = false;
            }
            continue;
        }
        if collecting {
            for w in t.split_whitespace() {
                if !w.starts_with('-') {
                    pkgs.push(w.trim_end_matches('\\').to_string());
                }
            }
            if !t.ends_with('\\') {
                collecting = false;
            }
        }
    }
    pkgs.retain(|p| !p.is_empty());
    pkgs
}

#[test]
fn every_tool_preflight_requires_has_a_known_package() {
    let unmapped: Vec<String> = required_tools(&run_sh())
        .into_iter()
        .filter(|t| !ASSUMED_PRESENT.contains(&t.as_str()))
        .filter(|t| !PROVIDED_BY.iter().any(|(tool, _)| tool == t))
        .collect();

    assert!(
        unmapped.is_empty(),
        "preflight asserts it needs these tools and this check does not know \
         which package provides them, so it cannot tell whether CI installs \
         them. Add each to PROVIDED_BY (or to ASSUMED_PRESENT if it is a \
         coreutil that needs no package):\n  {unmapped:?}"
    );
}

#[test]
fn ci_installs_every_package_those_tools_come_from() {
    let installed = apt_packages(&workflow());
    assert!(
        installed.iter().any(|p| p == "mosquitto"),
        "parsed no packages from the workflow's apt-get install line, so this \
         check would pass vacuously. Parsed: {installed:?}"
    );

    let required = required_tools(&run_sh());
    let missing: Vec<String> = required
        .iter()
        .filter(|t| !ASSUMED_PRESENT.contains(&t.as_str()))
        .filter_map(|t| {
            PROVIDED_BY
                .iter()
                .find(|(tool, _)| tool == t)
                .map(|(_, p)| (t, p))
        })
        .filter(|(_, pkg)| !installed.iter().any(|i| i == *pkg))
        .map(|(tool, pkg)| format!("{tool} (from {pkg})"))
        .collect();

    assert!(
        missing.is_empty(),
        "preflight requires these under --provision=external and the CI \
         workflow never installs the package that provides them. The run will \
         fail somewhere later, in a stage whose name has nothing to do with \
         the real cause:\n  {missing:?}\n\nInstalled: {installed:?}"
    );
}
