//! The interlock engine must be *reached* by production code, not merely exist.
//!
//! #83 shipped a complete, careful lease implementation in `edge/src/modules.rs`
//! -- interlock evaluation, hazard polarity, latching, per-module disconnect
//! policy, and `tick()`, which returns the lease grant or refusal for every
//! bound power module. It has 18 unit tests and four of them were individually
//! mutation-tested. Every one of those properties was true.
//!
//! And none of it ran. `tick()`, `renew()`, `evaluate()` and `lease_valid()`
//! had **thirty call sites, all of them inside the crate's own `#[cfg(test)]`
//! module and none anywhere else**. The edge cached `module/state` and
//! evaluated it never; no lease was ever issued, renewed or refused. The commit
//! that wrote it said so plainly -- delivery needed a wire contract with module
//! firmware that did not exist yet -- and the issue was then closed with that
//! follow-up outstanding, where it sat until somebody went looking.
//!
//! Nothing could have noticed. A unit test suite cannot see that it is the only
//! caller; that is precisely the shape of its blind spot, and the richer the
//! suite the more convincing the silence. So this is the check that can:
//! **does the decision reach the wire?**
//!
//! Two properties, because either alone is satisfiable by the bug:
//!
//! * `ModuleState::tick` is called from production code. Without this the
//!   engine never runs at all -- the original defect.
//! * `publish_tool_lease` is called from production code. With `tick` running
//!   but nothing publishing, the edge computes every lease correctly and throws
//!   them away, which from a module's side is indistinguishable from the first.
//!
//! "Production" means outside `edge/src/modules.rs` (where the engine and its
//! tests live) and before the first `#[cfg(test)]` in any file. That truncation
//! is deliberately blunt: it can only discard real code and make this check
//! stricter, never laxer, and house style puts test modules last.
//!
//! What this does NOT prove: that a lease is correct, that it arrives, or that
//! a module acts on it. It proves the engine is wired to something. The e2e
//! `lease` stage is the other oracle -- it subscribes to the broker and watches
//! renewals arrive and then stop, which is the claim a source check cannot make.

use css_checks::read;

/// Edge sources that are not the engine itself, with test modules cut off.
///
/// `modules.rs` is excluded for the reason this file exists: it is the one
/// place guaranteed to mention these names, and a corpus that includes the
/// thing being checked agrees with itself no matter what.
fn edge_production_sources() -> Vec<(&'static str, String)> {
    const FILES: &[&str] = &[
        "edge/src/main.rs",
        "edge/src/mqtt.rs",
        "edge/src/edge_inbound.rs",
        "edge/src/ws.rs",
        "edge/src/power.rs",
        "edge/src/doors.rs",
        "edge/src/toolguard.rs",
        "edge/src/web_server.rs",
        "edge/src/lib.rs",
    ];
    FILES
        .iter()
        .map(|f| (*f, production_only(&read(f))))
        .collect()
}

/// Everything before the first `#[cfg(test)]`.
fn production_only(src: &str) -> String {
    match src.find("#[cfg(test)]") {
        Some(i) => src[..i].to_string(),
        None => src.to_string(),
    }
}

/// Files whose production half calls `name`, ignoring the definition itself.
///
/// A `fn name(` line contains `name` too, so a bare substring search is
/// satisfied by the declaration of a function nobody calls -- which is the
/// exact condition being tested for.
fn callers_of(sources: &[(&'static str, String)], name: &str) -> Vec<&'static str> {
    sources
        .iter()
        .filter(|(_, src)| !call_sites(src, name).is_empty())
        .map(|(f, _)| *f)
        .collect()
}

/// Byte offsets of genuine calls to `name` -- every occurrence of `name(` that
/// is not preceded by `fn `.
fn call_sites(src: &str, name: &str) -> Vec<usize> {
    let call = format!("{name}(");
    let define = format!("fn {name}(");
    src.match_indices(&call)
        .filter(|(i, _)| !src[..i + call.len()].ends_with(&define))
        .map(|(i, _)| i)
        .collect()
}

/// How far back from a publish call to look for the `tick` that fed it.
///
/// Generous enough to span the loop and its bindings, tight enough that an
/// unrelated `tick` elsewhere in a 500-line file cannot vouch for it.
const LOCALITY: usize = 400;

/// Whether some call to `name` is fed by a nearby *engine* tick.
///
/// "Engine tick" means a `.tick(` that takes arguments. The distinction is the
/// whole point: `edge/src/main.rs` is full of `ticker.tick().await` from tokio
/// intervals, which take none, and an earlier draft of this check accepted
/// them -- so it passed on the very tree that had no watchdog at all. A check
/// satisfied by the thing it was written to rule out is worse than no check,
/// because it reads as evidence.
fn is_fed_by_an_engine_tick(sources: &[(&'static str, String)], name: &str) -> bool {
    sources.iter().any(|(_, src)| {
        call_sites(src, name).into_iter().any(|at| {
            let from = at.saturating_sub(LOCALITY);
            src[from..at].match_indices(".tick(").any(|(j, _)| {
                let after = from + j + ".tick(".len();
                src[after..].starts_with(|c: char| c != ')')
            })
        })
    })
}

#[test]
fn the_lease_decisions_come_from_the_engine_rather_than_from_nowhere() {
    let sources = edge_production_sources();

    assert!(
        is_fed_by_an_engine_tick(&sources, "publish_tool_lease"),
        "leases are published, but not from a `tick(..)` call with arguments \
         anywhere near the publish. Either the interlock engine is no longer \
         what decides them -- in which case the rules, the latches and the \
         disconnect policy are all being bypassed -- or the wiring moved far \
         enough apart that this check can no longer see it is still connected."
    );
}

#[test]
fn the_lease_decisions_are_published_rather_than_discarded() {
    let sources = edge_production_sources();
    let callers = callers_of(&sources, "publish_tool_lease");

    assert!(
        !callers.is_empty(),
        "nothing calls `publish_tool_lease`. The watchdog may be running and \
         deciding correctly, but no lease reaches the broker -- and from a \
         power module's side that is the same event as an edge that never \
         evaluated anything: silence, and a correct refusal to energize."
    );
}

// ── Self-tests ───────────────────────────────────────────────────────────────
//
// The checks above are greps, and a grep is exactly the kind of check that
// passes for the wrong reason. Each way it could is pinned here.

#[cfg(test)]
mod the_check_rejects_what_it_is_for {
    use super::*;

    #[test]
    fn a_call_that_exists_only_in_tests_does_not_count() {
        // The defect's actual shape: thirty callers, all of them tests.
        let src = "fn main() { nothing(); }\n\
                   #[cfg(test)]\n\
                   mod tests { fn t() { lc.publish_tool_lease(&a); } }";
        let sources = vec![("fake.rs", production_only(src))];
        assert!(
            callers_of(&sources, "publish_tool_lease").is_empty(),
            "a caller behind #[cfg(test)] must not satisfy the check -- that is \
             precisely the condition that hid the original defect"
        );
    }

    /// The trap the first draft of this file fell into. `edge/src/main.rs` is
    /// full of `ticker.tick().await` from tokio intervals; accepting those made
    /// the check pass on a tree whose watchdog did not exist.
    #[test]
    fn an_argumentless_interval_tick_does_not_vouch_for_a_publish() {
        let src = "fn run() { ticker.tick().await; lc.publish_tool_lease(&a); }";
        let sources = vec![("fake.rs", production_only(src))];
        assert!(
            !is_fed_by_an_engine_tick(&sources, "publish_tool_lease"),
            "a tokio interval tick takes no arguments and decides nothing; it \
             must not be mistaken for the interlock engine"
        );
    }

    #[test]
    fn an_engine_tick_with_arguments_does_vouch_for_a_publish() {
        let src = "fn run() { for a in ms.tick(now, ttl, off) { lc.publish_tool_lease(&a); } }";
        let sources = vec![("fake.rs", production_only(src))];
        assert!(
            is_fed_by_an_engine_tick(&sources, "publish_tool_lease"),
            "the real wiring must be recognised, or this check can never pass"
        );
    }

    /// And distance matters: a `tick` in an unrelated function far above must
    /// not vouch for a publish that nothing feeds.
    #[test]
    fn a_distant_tick_does_not_vouch_for_a_publish() {
        let src = format!(
            "fn a() {{ ms.tick(now, ttl, off); }}\n{}\nfn b() {{ lc.publish_tool_lease(&x); }}",
            "// filler\n".repeat(60)
        );
        let sources = vec![("fake.rs", production_only(&src))];
        assert!(
            !is_fed_by_an_engine_tick(&sources, "publish_tool_lease"),
            "an engine tick elsewhere in the file must not vouch for an \
             unconnected publish"
        );
    }

    #[test]
    fn a_definition_with_no_caller_does_not_count() {
        let src = "pub fn publish_tool_lease(&self, a: &LeaseAction) { todo!() }";
        let sources = vec![("fake.rs", production_only(src))];
        assert!(
            callers_of(&sources, "publish_tool_lease").is_empty(),
            "declaring the function must not count as calling it"
        );
    }

    #[test]
    fn a_real_call_does_count() {
        let src = "pub fn publish_tool_lease(&self, a: &LeaseAction) { todo!() }\n\
                   fn run() { lc.publish_tool_lease(&action); }";
        let sources = vec![("fake.rs", production_only(src))];
        assert_eq!(
            vec!["fake.rs"],
            callers_of(&sources, "publish_tool_lease"),
            "a genuine call site must be found, or this check can never pass"
        );
    }

    /// And the corpus itself: an empty or unreadable file list would make both
    /// checks above fail, not pass -- but an *exclusion* that quietly grew to
    /// cover main.rs would make them pass on nothing at all.
    #[test]
    fn the_corpus_contains_the_file_the_watchdog_lives_in() {
        let sources = edge_production_sources();
        assert!(
            sources
                .iter()
                .any(|(f, src)| *f == "edge/src/main.rs" && !src.is_empty()),
            "edge/src/main.rs must be in the corpus and non-empty; the runtime \
             wiring lives there and excluding it would make this check vacuous"
        );
    }
}
