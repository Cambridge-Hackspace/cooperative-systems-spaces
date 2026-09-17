//! The stack's runtime image must carry what the shipping image carries.
//!
//! `e2e/stack.sh` builds its own runtime container rather than pulling the
//! published one, and describes itself as the shipping runtime stage "minus the
//! parts that only matter in production". That is a narrowing, and like every
//! narrowing it is only as good as the reason behind it -- which nothing
//! recorded, and nothing checked.
//!
//! It was wrong. The shipping image installs `git`; the stack's did not. `git`
//! is not a production convenience: `PagesService` shells out to it to clone
//! and pull the wiki and site repositories, so a stack could not have served a
//! wiki even if one had been configured. The pages pipeline consequently had no
//! end-to-end coverage of any kind, which is how #81 shipped a navigation that
//! silently discarded sixty of the live wiki's sixty-seven pages while
//! reporting "67 pages loaded".
//!
//! The failure was invisible in both directions. Nothing said the test image
//! was missing a runtime dependency, and nothing said the feature was untested
//! -- the tier simply did not exist, and an absent tier is quiet.
//!
//! So: every package the shipping runtime installs must be installed here too,
//! unless it is named in [`PRODUCTION_ONLY`] with a reason. The exclusion list
//! is the point. "Minus the parts that only matter in production" is a claim
//! about a specific set of packages, and writing that set down is what turns it
//! from an assumption into something that can be wrong out loud.
//!
//! What this does not prove: that either image works, or that the *versions*
//! agree. The base image is digest-pinned and the packages are not; this checks
//! the set, not the bytes.

use std::collections::BTreeSet;

use css_checks::read;

/// Packages the shipping image installs that the stack deliberately does not.
///
/// Each entry is a claim that the package cannot affect behaviour under test.
/// `git` was never a defensible member of this list and is the reason the file
/// exists.
const PRODUCTION_ONLY: &[(&str, &str)] = &[(
    "vim",
    "an interactive editor for poking at a running container; nothing the \
     server does can reach it",
)];

/// The final stage of a multi-stage Dockerfile.
///
/// Scoped on purpose: the builder stages install `build-essential`, `cmake` and
/// the `-dev` packages needed to compile, and none of that is in the shipped
/// image or wanted in the stack's. A check that compared every `apt-get` line
/// in the file would demand a C toolchain in the test runtime and be switched
/// off within a week.
fn runtime_stage(dockerfile: &str) -> &str {
    match dockerfile.rfind("\nFROM ") {
        Some(at) => &dockerfile[at..],
        // Single-stage, or not a Dockerfile at all -- the anti-vacuity check in
        // the test catches the second case.
        None => dockerfile,
    }
}

/// Packages named in an `apt-get install` line, from whichever file.
///
/// Both files write the install as one continued line, so this walks lines
/// rather than trying to parse shell.
fn apt_packages(source: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let lines: Vec<&str> = source.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        if !line.contains("apt-get install") {
            continue;
        }
        // The package list may continue across backslash-continued lines.
        let mut j = i;
        loop {
            let text = lines[j];
            for tok in text.split_whitespace() {
                let tok = tok.trim_end_matches('\\');
                if tok.is_empty()
                    || tok.starts_with('-')
                    || tok.starts_with('&')
                    || tok.starts_with("RUN")
                    || tok.starts_with("apt-get")
                    || tok == "install"
                    || tok == "update"
                    || tok == "y"
                    || tok == "rm"
                    || tok.starts_with('/')
                {
                    continue;
                }
                out.insert(tok.to_string());
            }
            if !text.trim_end().ends_with('\\') || j + 1 >= lines.len() {
                break;
            }
            j += 1;
        }
    }
    out
}

#[test]
fn the_stack_runtime_installs_everything_the_shipping_runtime_does() {
    let shipping = apt_packages(runtime_stage(&read("Dockerfile")));
    let stack = apt_packages(&read("e2e/stack.sh"));

    assert!(
        shipping.contains("libpq5") && stack.contains("libpq5"),
        "the package extraction found nothing recognisable (shipping: \
         {shipping:?}, stack: {stack:?}); a check that parses neither file \
         passes whatever they say"
    );

    let excused: BTreeSet<String> = PRODUCTION_ONLY.iter().map(|(p, _)| p.to_string()).collect();
    let missing: Vec<&String> = shipping
        .difference(&stack)
        .filter(|p| !excused.contains(*p))
        .collect();

    assert!(
        missing.is_empty(),
        "the shipping runtime image installs {missing:?} and the stack's does \
         not.\n\nEither install them in `build_runtime_image` (e2e/stack.sh), or \
         add them to PRODUCTION_ONLY with a reason they cannot affect anything \
         under test. `git` was missing for exactly this reason and the pages \
         pipeline was untestable as a result -- see #81."
    );
}

/// An exclusion nobody needs is an exclusion nobody has reconsidered.
#[test]
fn every_excused_package_is_actually_in_the_shipping_image() {
    let shipping = apt_packages(runtime_stage(&read("Dockerfile")));
    for (package, _reason) in PRODUCTION_ONLY {
        assert!(
            shipping.contains(*package),
            "PRODUCTION_ONLY excuses {package:?}, which the shipping image no \
             longer installs. The exclusion is stale; drop it."
        );
    }
}

#[cfg(test)]
mod the_check_rejects_the_state_that_shipped_the_bug {
    use super::*;

    const SHIPPING: &str = r#"
FROM rust:1.97 AS backend-builder
RUN apt-get update && apt-get install -y build-essential cmake libpq-dev

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y \
    libpq5 \
    ca-certificates  \
    vim \
    git \
    && rm -rf /var/lib/apt/lists/*
"#;

    /// The stack image as it actually stood: no git.
    const STACK_BEFORE: &str = r#"
 && apt-get install -y --no-install-recommends libpq5 ca-certificates tzdata \
"#;

    const STACK_AFTER: &str = r#"
 && apt-get install -y --no-install-recommends libpq5 ca-certificates tzdata git \
"#;

    #[test]
    fn it_reads_both_file_shapes() {
        let shipping = apt_packages(runtime_stage(SHIPPING));
        assert!(shipping.contains("git"), "got {shipping:?}");
        assert!(shipping.contains("vim"), "got {shipping:?}");
        assert!(shipping.contains("libpq5"), "got {shipping:?}");
        assert!(apt_packages(STACK_BEFORE).contains("tzdata"));
    }

    #[test]
    fn it_sees_the_missing_git() {
        let missing: Vec<_> = apt_packages(runtime_stage(SHIPPING))
            .difference(&apt_packages(STACK_BEFORE))
            .filter(|p| *p != "vim")
            .cloned()
            .collect();
        assert_eq!(
            vec!["git".to_string()],
            missing,
            "the check has to name git on the image that shipped the bug"
        );
    }

    /// The builder stage's packages must not leak into the comparison, or the
    /// check demands a C toolchain in the test runtime.
    #[test]
    fn builder_stage_packages_are_not_counted() {
        let runtime = apt_packages(runtime_stage(SHIPPING));
        assert!(!runtime.contains("build-essential"), "got {runtime:?}");
        assert!(!runtime.contains("cmake"), "got {runtime:?}");
        assert!(runtime.contains("git"));
    }

    #[test]
    fn it_accepts_the_fixed_image() {
        let missing: Vec<_> = apt_packages(runtime_stage(SHIPPING))
            .difference(&apt_packages(STACK_AFTER))
            .filter(|p| *p != "vim")
            .cloned()
            .collect();
        assert!(missing.is_empty(), "still reported {missing:?}");
    }
}
