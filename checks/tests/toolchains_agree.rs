//! The container builds use the same toolchains as everything else.
//!
//! `Dockerfile` and `Dockerfile.edge` are the only place in this repository
//! that pins its own Node and Rust, and they are the only place a CI failure
//! cannot be seen from a feature branch -- both docker jobs are gated on
//! `master`/`dev`. So drift here is discovered on a merge, by which point it
//! is already on the integration branch.
//!
//! It cost exactly that. The Dockerfiles built on `node:20-alpine` while CI
//! and `e2e/build.sh` used Node 24, and `npm ci` failed on the first push to
//! dev that got far enough to run it. Node 20 is below the floor the
//! dependency tree states -- jsdom, undici and the @asamuzakjp packages all
//! declare `node: ^22.x || >=24` -- so it was never going to work. The Rust
//! pin had drifted the same way, 1.90.0 against a 1.97 build image, and was
//! simply lucky.
//!
//! What this does NOT prove: that the images build. Only that the versions
//! they name agree with the versions the rest of the project uses. A
//! container engine is needed for the rest, and there is none on the FreeBSD
//! workstation where this runs.

use css_checks::read;

/// The major version in a `FROM node:<v>...` line.
fn dockerfile_node_major(file: &str) -> u32 {
    let source = read(file);
    let line = source
        .lines()
        .find(|l| l.trim_start().starts_with("FROM node:"))
        .unwrap_or_else(|| panic!("{file} has no `FROM node:` line"));

    let tag = line
        .trim()
        .trim_start_matches("FROM node:")
        .split(['-', ' '])
        .next()
        .unwrap_or("");

    tag.split('.')
        .next()
        .and_then(|m| m.parse().ok())
        .unwrap_or_else(|| panic!("{file}: cannot read a major version from {line:?}"))
}

/// The first run of digits after `marker` on each line that contains it.
///
/// Written this way because the same key appears in two YAML shapes --
/// `node-version: '24'` on its own line and `with: { node-version: '24', ... }`
/// inline -- and a first version that split on quotes and commas found two of
/// the seven. A parser that silently sees less than it should is the failure
/// this crate exists to avoid, so it scans for digits rather than trying to
/// anticipate the punctuation.
fn majors_after(source: &str, marker: &str) -> Vec<u32> {
    let mut found = Vec::new();
    for line in source.lines() {
        let Some(rest) = line.split(marker).nth(1) else {
            continue;
        };
        let digits: String = rest
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(v) = digits.parse() {
            found.push(v);
        }
    }
    found
}

/// The `node-version: '24'` values in the CI workflow.
fn ci_node_majors() -> Vec<u32> {
    majors_after(&read(".github/workflows/css-ci.yml"), "node-version:")
}

/// The major version of `NODE_VERSION="v24.20.0"` in e2e/build.sh.
fn build_sh_node_major() -> u32 {
    let source = read("e2e/build.sh");
    let line = source
        .lines()
        .find(|l| l.trim_start().starts_with("NODE_VERSION="))
        .expect("e2e/build.sh has no NODE_VERSION");

    line.split('"')
        .nth(1)
        .and_then(|v| v.trim_start_matches('v').split('.').next())
        .and_then(|m| m.parse().ok())
        .unwrap_or_else(|| panic!("cannot read a major version from {line:?}"))
}

#[test]
fn every_source_of_a_node_version_was_found() {
    // Anti-vacuity: each parser panics rather than defaulting, so this test
    // failing means one of them stopped finding its version -- which would
    // otherwise let the comparison below run over nothing.
    let ci = ci_node_majors();
    assert!(
        ci.len() >= 7,
        "found {} node-version keys in the workflow; there are seven. A parser \
         that sees fewer than it should makes the comparison pass over the rest.",
        ci.len()
    );
    assert!(build_sh_node_major() >= 20);
    assert!(dockerfile_node_major("Dockerfile") >= 20);
    assert!(dockerfile_node_major("Dockerfile.edge") >= 20);
}

#[test]
fn the_dockerfiles_build_on_the_same_node_as_everything_else() {
    let expected = build_sh_node_major();
    let mut sources: Vec<(String, u32)> = vec![("e2e/build.sh".into(), expected)];

    for (i, v) in ci_node_majors().into_iter().enumerate() {
        sources.push((format!("css-ci.yml node-version #{}", i + 1), v));
    }
    for f in ["Dockerfile", "Dockerfile.edge"] {
        sources.push((f.into(), dockerfile_node_major(f)));
    }

    let disagreeing: Vec<&(String, u32)> = sources.iter().filter(|(_, v)| *v != expected).collect();

    assert!(
        disagreeing.is_empty(),
        "these name a different Node major than e2e/build.sh's {expected}:\n\
         {disagreeing:?}\n\n\
         The dependency tree declares `node: ^22.x || >=24` in several places \
         (jsdom, undici, @asamuzakjp/*), so an older builder fails `npm ci` \
         outright. The docker jobs only run on master/dev, so a mismatch here \
         is found on a merge rather than on a branch."
    );
}

#[test]
fn the_dockerfiles_build_with_the_same_rust_as_the_session_image() {
    // .reaper.toml pins the build image by digest with the version in a
    // comment beside it; the Dockerfiles name a tag. Comparing the majors and
    // minors is enough to catch the drift that mattered (1.90 vs 1.97).
    let reaper = read(".reaper.toml");
    // The version sits in an indented comment beside the digest, as
    // `#   1.97 matches the workstation's rustc ...`, so the prefix is not a
    // bare "# ".
    let pinned = reaper
        .lines()
        .filter(|l| l.trim_start().starts_with('#'))
        .find_map(|l| {
            let c = l.trim_start().trim_start_matches('#').trim_start();
            c.strip_prefix("1.")
                .and_then(|r| r.split(['.', ' ']).next())
                .and_then(|m| m.parse::<u32>().ok())
        })
        .expect("no `# 1.<minor>` comment naming the rust image in .reaper.toml");

    for f in ["Dockerfile", "Dockerfile.edge"] {
        let source = read(f);
        let line = source
            .lines()
            .find(|l| l.trim_start().starts_with("FROM rust:"))
            .unwrap_or_else(|| panic!("{f} has no `FROM rust:` line"));
        let minor: u32 = line
            .trim()
            .trim_start_matches("FROM rust:")
            .split(['.', ' ', '-'])
            .nth(1)
            .and_then(|m| m.parse().ok())
            .unwrap_or_else(|| panic!("{f}: cannot read a minor version from {line:?}"));

        assert_eq!(
            minor, pinned,
            "{f} builds with Rust 1.{minor} and the reaper build image is \
             1.{pinned}. Nothing else reconciles them, and this file's build \
             only runs on a push to master or dev -- so a mismatch surfaces \
             after a merge, not before one."
        );
    }
}
