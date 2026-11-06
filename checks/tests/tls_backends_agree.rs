//! Every crate in this workspace that speaks TLS has a crypto provider, and it
//! is the post-quantum one.
//!
//! WHAT THIS COST, and why a build cannot see it. rustls 0.23 chooses its
//! crypto backend at runtime from a process-wide provider, and panics the first
//! time a TLS config is built if it cannot determine one. The edge pulled rustls
//! through `tokio-tungstenite`'s `rustls-tls-native-roots`, which enables rustls
//! with DEFAULT FEATURES OFF -- so neither `aws_lc_rs` nor `ring` was compiled
//! in, and nothing installed a provider. The result (#207) was an edge whose
//! WebSocket task panicked at startup on every boot while the rest of it kept
//! working: HTTP sync, the local broker and scan answers all fine, the edge
//! showing **offline** in the portal, and nothing logged above INFO. It ran that
//! way from 2026-10-14 to 2026-11-06.
//!
//! Every tier missed it. It compiles, because the fault is a runtime provider
//! lookup. The unit suites missed it, because nothing built a TLS client config.
//! The e2e stack missed it, because it does not terminate TLS. A panic in a
//! `tokio::spawn`ed task whose `JoinHandle` is dropped takes its own reconnect
//! loop down with it and reports nothing, so the edge's own log looked healthy.
//!
//! So the oracle is the DEPENDENCY GRAPH, read as data. `Cargo.lock` records
//! which optional dependencies were actually resolved, and a provider feature on
//! rustls shows up there as a dependency edge -- which makes "rustls has a
//! provider" a static, buildless assertion.
//!
//! The post-quantum half is policy, stated once here: ML-KEM exists only in
//! rustls's `aws_lc_rs` provider (`src/crypto/aws_lc_rs/pq/`), and
//! `prefer-post-quantum` is literally defined as `["aws_lc_rs"]`, so choosing
//! `ring` silently opts the control plane out of hybrid key exchange. BoneMesh
//! is post-quantum on the wire; the plane that unlocks doors should not be
//! weaker by accident.
//!
//! WHAT THIS DOES NOT PROVE:
//!   - That any connection is actually post-quantum. That depends on what
//!     terminates TLS in front of css-server, which is a reverse proxy outside
//!     this repository -- css-server itself serves plain HTTP. The edge reports
//!     the group it negotiated so the deployed answer is readable, and that is
//!     where the real claim lives.
//!   - That the provider install runs before the first TLS config is built.
//!     Ordering inside `main` is not visible here; the edge's own unit test
//!     builds a client config to cover it.
//!   - That the images build. aws-lc-rs compiles a C library and needs cmake,
//!     which `Dockerfile.edge`'s builder already installs; proving the build is
//!     a container engine's job and there is none on this workstation.

use css_checks::{read, repo_root};

/// One `[[package]]` stanza from `Cargo.lock`.
struct Pkg {
    name: String,
    version: String,
    deps: Vec<String>,
}

fn lock() -> Vec<Pkg> {
    let doc: toml::Value = toml::from_str(&read("Cargo.lock")).expect("Cargo.lock is not TOML");
    doc.get("package")
        .and_then(|p| p.as_array())
        .expect("Cargo.lock has no [[package]] array")
        .iter()
        .map(|p| Pkg {
            name: p["name"]
                .as_str()
                .expect("package without a name")
                .to_string(),
            version: p["version"]
                .as_str()
                .expect("package without a version")
                .to_string(),
            // A dependency is "name" or "name version"; the version is present
            // only when the name alone is ambiguous. Take the name.
            deps: p
                .get("dependencies")
                .and_then(|d| d.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|d| d.as_str())
                        .map(|d| d.split_whitespace().next().unwrap_or(d).to_string())
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// `0.23.35` -> `0.23`. rustls is pre-1.0, so the minor IS the major.
fn series(version: &str) -> String {
    version.split('.').take(2).collect::<Vec<_>>().join(".")
}

const PROVIDERS: [&str; 2] = ["aws-lc-rs", "ring"];

#[test]
fn rustls_resolves_with_exactly_one_crypto_provider() {
    // Scoped to 0.23 and later ON PURPOSE: the provider abstraction does not
    // exist before it. rustls 0.21 depends on `ring` unconditionally, so
    // including it here would make this pass for a reason that has nothing to
    // do with what is being checked. `only_one_rustls_series_is_linked` is what
    // keeps an old one out.
    let pkgs = lock();
    let modern: Vec<&Pkg> = pkgs
        .iter()
        .filter(|p| p.name == "rustls" && series(&p.version).as_str() >= "0.23")
        .collect();
    assert!(
        !modern.is_empty(),
        "no rustls 0.23+ in Cargo.lock; this check is about the provider \
         abstraction and has nothing to assert. Delete it or fix the filter."
    );
    for p in modern {
        let found: Vec<&str> = PROVIDERS
            .iter()
            .copied()
            .filter(|prov| p.deps.iter().any(|d| d == prov))
            .collect();
        assert_eq!(
            found.len(),
            1,
            "rustls {} resolved with {} crypto provider(s) {:?}, not exactly one.\n\
             Its resolved dependencies are {:?}.\n\
             None means a panic the first time a TLS config is built (#207); two \
             means rustls cannot choose. Enable exactly one provider feature on \
             the rustls dependency.",
            p.version,
            found.len(),
            found,
            p.deps
        );
    }
}

#[test]
fn only_one_rustls_series_is_linked() {
    let pkgs = lock();
    let mut found: Vec<String> = pkgs
        .iter()
        .filter(|p| p.name == "rustls")
        .map(|p| p.version.clone())
        .collect();
    found.sort();
    assert_eq!(
        found.len(),
        1,
        "Cargo.lock carries {} rustls versions {:?}. Two series means two TLS \
         stacks with two independent provider policies, and a provider installed \
         for one does nothing for the other -- server/Cargo.toml already names \
         that cost as its reason for staying on native-tls. Bring the laggard up.",
        found.len(),
        found
    );
}

#[test]
fn the_provider_is_the_post_quantum_one() {
    let pkgs = lock();
    for p in pkgs
        .iter()
        .filter(|p| p.name == "rustls" && series(&p.version).as_str() >= "0.23")
    {
        assert!(
            p.deps.iter().any(|d| d == "aws-lc-rs"),
            "rustls {} is not using the aws-lc-rs provider. ML-KEM exists only \
             under rustls's `aws_lc_rs` provider, and `prefer-post-quantum` is \
             defined as `[\"aws_lc_rs\"]`, so any other choice opts the control \
             plane out of hybrid key exchange. Resolved deps: {:?}",
            p.version,
            p.deps
        );
    }
}

/// Workspace member directories, read from the workspace manifest rather than
/// listed here, so a new member cannot be quietly exempt from the check below.
fn members() -> Vec<String> {
    let doc: toml::Value = toml::from_str(&read("Cargo.toml")).expect("root Cargo.toml");
    doc["workspace"]["members"]
        .as_array()
        .expect("workspace.members")
        .iter()
        .filter_map(|m| m.as_str())
        .map(str::to_string)
        .collect()
}

#[test]
fn every_member_that_declares_rustls_asks_for_post_quantum() {
    let mut declaring = Vec::new();
    for m in members() {
        let manifest = format!("{m}/Cargo.toml");
        if !repo_root().join(&manifest).exists() {
            continue;
        }
        let text = read(&manifest);
        let Some(line) = text.lines().find(|l| l.trim_start().starts_with("rustls")) else {
            continue;
        };
        declaring.push(m.clone());
        for want in ["aws_lc_rs", "prefer-post-quantum"] {
            assert!(
                line.contains(want),
                "{manifest} declares rustls without `{want}`:\n  {}\n\
                 Both must be named explicitly. tungstenite pulls rustls with \
                 DEFAULT FEATURES OFF, so rustls's own default list -- which \
                 contains both -- never applies.",
                line.trim()
            );
        }
    }
    assert!(
        !declaring.is_empty(),
        "no workspace member declares rustls, yet Cargo.lock links it. It is \
         arriving only transitively, which is exactly how #207 happened: no \
         crate owned the provider decision, so nobody made it."
    );
}

/// Every workspace member whose dependency graph REACHES rustls, whether it
/// declares it or not. Reachability and not declaration, because #207's whole
/// mechanism was that the edge never named rustls: it arrived through
/// tokio-tungstenite, so no crate owned the decision and a check that only read
/// manifests would have seen nothing to complain about.
///
/// ASKED OF CARGO, not computed from Cargo.lock, and the first version of this
/// got it wrong in the direction that matters. A lock file's dependency edges
/// are the UNION of optional dependencies across the whole workspace's feature
/// resolution, so walking them reported `server` as reaching rustls -- which is
/// false, and would have demanded a provider install in the one crate that
/// deliberately uses native-tls (see server/Cargo.toml). `cargo tree -i` answers
/// for one package's real graph with its real features:
///
///     $ cargo tree -p css-server -i rustls
///     error: package ID specification `rustls` did not match any packages
///
/// Cargo is not a new dependency here -- this check only runs under `cargo test`
/// in the first place.
fn members_reaching(target: &str) -> Vec<String> {
    let mut reaching = Vec::new();
    for dir in members() {
        let manifest = format!("{dir}/Cargo.toml");
        if !repo_root().join(&manifest).exists() {
            continue;
        }
        let doc: toml::Value = toml::from_str(&read(&manifest)).expect("member manifest");
        let name = doc["package"]["name"]
            .as_str()
            .expect("member name")
            .to_string();

        let out = std::process::Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
            .args(["tree", "-p", &name, "-i", target, "--edges", "normal"])
            .current_dir(repo_root())
            .output()
            .expect("could not run `cargo tree`");

        if out.status.success() {
            reaching.push(dir);
            continue;
        }
        // The ONE failure that means "not in this graph". Any other failure is
        // this check being unable to answer, which must not read as a clean
        // bill of health -- that is the direction a false negative hides in.
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("did not match any packages"),
            "`cargo tree -p {name} -i {target}` failed for a reason that is not \
             \"not in the graph\", so this check cannot say whether {dir} links \
             {target}:\n{err}"
        );
    }
    reaching
}

#[test]
fn every_member_that_reaches_rustls_installs_a_provider() {
    let reaching = members_reaching("rustls");
    assert!(
        !reaching.is_empty(),
        "no workspace member reaches rustls, so this check has nothing to assert"
    );
    for dir in &reaching {
        let src = repo_root().join(dir).join("src");
        let mut found = Vec::new();
        for entry in walkdir::WalkDir::new(&src)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.path().extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(entry.path()).unwrap_or_default();
            if text.contains("install_default") {
                found.push(entry.path().display().to_string());
            }
        }
        assert!(
            !found.is_empty(),
            "{dir} reaches rustls but nothing under {dir}/src calls \
             `install_default`. rustls 0.23 panics the first time a TLS config \
             is built if the process-wide provider was never installed, and a \
             panic in a spawned task is not an Err -- it takes the task's own \
             retry loop with it and reports nothing (#207). Install one \
             explicitly at the top of main, which also makes a future \
             dependency bump that adds a second provider harmless instead of \
             fatal.\nMembers reaching rustls: {reaching:?}"
        );
    }
}
