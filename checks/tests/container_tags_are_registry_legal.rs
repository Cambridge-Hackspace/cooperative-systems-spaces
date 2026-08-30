//! Every image tag CI builds is one a registry will accept.
//!
//! `ghcr.io` rejects a tag whose repository component is not lowercase, and
//! `${{ github.repository }}` preserves the owner's case as typed. This
//! repository's owner is `Cambridge-Hackspace`, so every tag the two docker
//! jobs built was invalid and `docker build` refused before it started:
//!
//!     invalid tag "ghcr.io/Cambridge-Hackspace/cooperative-systems-spaces/app:...":
//!     repository name must be lowercase
//!
//! It survived because those jobs are gated on
//! `github.ref_name == 'master' || 'dev'`. They skipped on every feature
//! branch, so the tag was never evaluated until the first push to `dev` --
//! which is the worst possible moment to learn it, since by then the change
//! has already been merged.
//!
//! That gate is right and stays: a publish job that runs on a branch it will
//! not publish from reports success for doing nothing. The answer is to check
//! the tags without running them, which is what this does.
//!
//! What this does NOT prove: that the image builds, that the Dockerfile is
//! correct, or that the push is authorized. It proves only that the *name* is
//! one a registry can accept -- the failure that cost a merge to find.

use css_checks::read;

fn workflow() -> String {
    read(".github/workflows/css-ci.yml")
}

/// Every `-t <tag>` and `docker push <tag>` argument in the workflow.
///
/// The `${{ ... }}` forms are collapsed first, and that is not cosmetic: a
/// GitHub expression contains spaces, so splitting an argument on whitespace
/// truncates `ghcr.io/${{ github.repository }}/app:...` to `ghcr.io/${{` --
/// which contains no `github.repository` and passes the very check written to
/// find it. Caught by mutation-checking this file: reinstating the bad tags
/// left all three tests green.
fn collapse_expressions(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;

    while let Some(start) = rest.find("${{") {
        out.push_str(&rest[..start]);
        match rest[start..].find("}}") {
            Some(rel) => {
                let end = start + rel + 2;
                out.extend(rest[start..end].chars().filter(|c| !c.is_whitespace()));
                rest = &rest[end..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

fn image_refs(source: &str) -> Vec<String> {
    let mut refs = Vec::new();

    for raw in source.lines() {
        let collapsed = collapse_expressions(raw.trim());
        let line = collapsed.as_str();
        for marker in ["-t ", "docker push "] {
            if let Some(i) = line.find(marker) {
                let rest = &line[i + marker.len()..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == '\\')
                    .unwrap_or(rest.len());
                let candidate = rest[..end].trim();
                if candidate.contains('/') || candidate.contains(':') {
                    refs.push(candidate.to_string());
                }
            }
        }
    }

    refs
}

#[test]
fn the_workflow_still_builds_images() {
    // Anti-vacuity. If the docker jobs are removed or rewritten in a shape
    // this parser does not recognise, every assertion below passes over an
    // empty list -- which is exactly the "a check that finds nothing looks
    // like a check that found nothing wrong" failure this crate exists for.
    let refs = image_refs(&workflow());
    assert!(
        refs.len() >= 8,
        "parsed only {refs:?} out of the workflow. The two docker jobs build \
         and push four tags each; if that changed shape, this check is no \
         longer reading them."
    );
}

#[test]
fn no_image_ref_can_carry_an_uppercase_repository() {
    // The literal text, before any expansion. `${{ github.repository }}` is
    // the trap: it looks lowercase in the file and is not at runtime.
    let bad: Vec<String> = image_refs(&workflow())
        .into_iter()
        .filter(|r| r.contains("github.repository"))
        .collect();

    assert!(
        bad.is_empty(),
        "these image refs interpolate `github.repository` directly:\n{bad:?}\n\n\
         That expands with the owner's case as typed -- `Cambridge-Hackspace` \
         here -- and ghcr.io refuses any tag whose repository component is not \
         lowercase. Compute a lowercase base once per job instead:\n\n\
         \x20   run: echo \"IMAGE_BASE=ghcr.io/${{GITHUB_REPOSITORY,,}}\" >>\"${{GITHUB_ENV}}\"\n\n\
         and build the tags from that."
    );
}

#[test]
fn the_lowercase_base_is_computed_wherever_it_is_used() {
    // The other half: a job that uses ${IMAGE_BASE} without setting it builds
    // a tag beginning `/app:`, which fails differently and just as late.
    let source = workflow();
    let uses = source.matches("${IMAGE_BASE}").count();
    let sets = source
        .matches("IMAGE_BASE=ghcr.io/${GITHUB_REPOSITORY,,}")
        .count();

    assert!(
        uses > 0,
        "nothing uses ${{IMAGE_BASE}} any more -- if the tags were rewritten, \
         re-derive what this file checks rather than deleting it"
    );
    assert_eq!(
        sets, 2,
        "{sets} job(s) compute IMAGE_BASE but {uses} reference(s) use it. \
         Each job needs its own step: GITHUB_ENV does not carry between jobs, \
         so a missing one yields a tag that starts with a bare slash."
    );
}
