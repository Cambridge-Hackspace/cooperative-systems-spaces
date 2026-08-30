//! A stage named in `.reaper.toml` has to be a stage that exists.
//!
//! `[profiles.hunt]` set `CSS_E2E_STAGES = "up,journeys"`. There is no
//! `journeys` stage -- the Tier 9 driver was never written -- so the profile
//! could not run at all. Worse than that: `e2e/run.sh` dispatches stages as
//! `"stage_${stage}"`, so before it learned to validate names, an unknown one
//! became a command-not-found that was recorded as a stage which ran and
//! failed. The report would have said the journeys tier was broken, when what
//! was broken was one word in a config file.
//!
//! `run.sh` now rejects an unknown stage by name at startup, which catches this
//! for anybody who runs it. This check catches it without running anything at
//! all -- which matters because the profile it applies to is the long,
//! occasional one that nobody invokes on a normal day, and a configuration
//! error that only surfaces when you finally reach for the thing is the same
//! failure mode as the eight-times-too-small iteration count next door.
//!
//! What this does not prove: that the stage list is a sensible one to run
//! together, or that the profile's other settings mean anything.

use css_checks::repo_root;

/// The stage names `e2e/run.sh` knows, read from its own `STAGES_ALL`.
///
/// Read from the script rather than restated here. A list copied into this file
/// would agree with itself forever and stop tracking the thing it checks.
fn known_stages(run_sh: &str) -> Vec<String> {
    for line in run_sh.lines() {
        if let Some(rest) = line.trim().strip_prefix("STAGES_ALL=") {
            return rest
                .trim_matches('"')
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

#[test]
fn every_stage_a_profile_names_exists() {
    let root = repo_root();
    let run_sh = std::fs::read_to_string(root.join("e2e/run.sh"))
        .expect("e2e/run.sh must be readable from the repo root");
    let manifest = std::fs::read_to_string(root.join(".reaper.toml"))
        .expect(".reaper.toml must be readable from the repo root");

    let known = known_stages(&run_sh);
    assert!(
        known.len() >= 8,
        "read {} stages from run.sh's STAGES_ALL, which is too few to be right -- \
         this check reads that assignment by line, so a reformat turns it into a \
         permanent silent pass: {known:?}",
        known.len()
    );

    // The two names run.sh resolves itself rather than dispatching.
    let meta = ["all", "default"];

    let mut failures = Vec::new();
    let mut checked = 0usize;

    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('#') {
            continue;
        }
        let Some(at) = t.find("CSS_E2E_STAGES") else {
            continue;
        };
        let after = &t[at..];
        let Some(open) = after.find('"') else {
            continue;
        };
        let rest = &after[open + 1..];
        let Some(close) = rest.find('"') else {
            continue;
        };

        for stage in rest[..close].split(',') {
            let stage = stage.trim();
            if stage.is_empty() {
                continue;
            }
            checked += 1;
            if meta.contains(&stage) || known.iter().any(|k| k == stage) {
                continue;
            }
            failures.push(format!(
                "`{stage}` is named in .reaper.toml and is not a stage. \
                 run.sh has: {}",
                known.join(", ")
            ));
        }
    }

    assert!(
        checked > 0,
        "no CSS_E2E_STAGES values found in .reaper.toml -- this check reads them \
         by line, so it would pass by finding nothing"
    );

    assert!(
        failures.is_empty(),
        "a reaper profile names a stage that does not exist, so invoking that \
         profile cannot work.\n\n  {}",
        failures.join("\n  ")
    );
}
