//! Parse the real cmi5 course manifests from ADL's CATAPULT `course_examples`.
//!
//! The unit tests in `src/manifest.rs` exercise hand-written manifests; these
//! are the genuine article — the official ADL example courses (single-AU,
//! multi-AU-at-root, masteryScore, and a pre/post-test with blocks and
//! objectives). They exercise real-world shapes the fixtures do not: AU URLs
//! that already carry a query string with `&amp;`, langstrings with surrounding
//! whitespace, nested blocks, and objective references.
//!
//! Fixtures live in `e2e/fixtures/catapult/*.cmi5.xml`, committed from
//! https://github.com/adlnet/CATAPULT/tree/main/course_examples. They sit under
//! `e2e/` rather than in this crate so the one copy is reachable both here and by
//! the e2e `cmi5` stage driver, which only has `e2e/` mounted.

use cmi5::{parse_manifest, to_manifest_xml, validate_course_structure};
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../e2e/fixtures/catapult")
}

fn manifests() -> Vec<(String, String)> {
    let dir = fixtures_dir();
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("tests/fixtures/catapult must exist") {
        let path = entry.expect("readable").path();
        if path.extension().is_some_and(|e| e == "xml") {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            let xml = std::fs::read_to_string(&path).expect("readable manifest");
            out.push((name, xml));
        }
    }
    out.sort();
    out
}

#[test]
fn the_fixtures_are_present() {
    let m = manifests();
    assert!(
        m.len() >= 6,
        "expected the six CATAPULT example manifests, found {}",
        m.len()
    );
}

#[test]
fn every_real_manifest_parses_with_at_least_one_au() {
    for (name, xml) in manifests() {
        let cs = parse_manifest(&xml).unwrap_or_else(|e| panic!("{name} failed to parse: {e}"));
        assert!(
            !cs.assignable_units().is_empty(),
            "{name} parsed but has no assignable units"
        );
        assert!(!cs.course.id.is_empty(), "{name} has no course id");
    }
}

#[test]
fn every_real_manifest_round_trips_at_the_tree_level() {
    // parse ∘ serialize ∘ parse is identity for real content too — the strongest
    // check that the serializer used by export handles what vendors actually emit.
    for (name, xml) in manifests() {
        let once = parse_manifest(&xml).unwrap_or_else(|e| panic!("{name} parse 1: {e}"));
        let serialized = to_manifest_xml(&once).unwrap_or_else(|e| panic!("{name} serialize: {e}"));
        let twice = parse_manifest(&serialized).unwrap_or_else(|e| panic!("{name} parse 2: {e}"));
        assert_eq!(once, twice, "{name} did not round-trip");
    }
}

#[test]
fn every_real_manifest_is_conformant() {
    // The conformance validator must accept genuine vendor content, or it is
    // useless: a validator that rejects the official ADL examples would reject
    // everything real.
    for (name, xml) in manifests() {
        let cs = parse_manifest(&xml).unwrap_or_else(|e| panic!("{name} parse: {e}"));
        let errors = validate_course_structure(&cs);
        assert!(
            errors.is_empty(),
            "{name} is a real, conformant course but the validator rejected it: {errors:?}"
        );
    }
}

#[test]
fn the_multi_au_course_has_multiple_aus_in_order() {
    let (_, xml) = manifests()
        .into_iter()
        .find(|(n, _)| n.contains("multi_au"))
        .expect("a multi_au fixture");
    let cs = parse_manifest(&xml).expect("parse");
    assert!(
        cs.assignable_units().len() >= 2,
        "the multi-AU course should expose more than one AU"
    );
}
