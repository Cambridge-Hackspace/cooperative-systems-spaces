//! Conformance validation of a parsed [`CourseStructure`] against the cmi5
//! CourseStructure rules, run at import before anything is persisted or written
//! to disk.
//!
//! Parsing already rejects a malformed manifest (a missing `<course>`, an AU
//! with no `<url>`, an out-of-range `masteryScore`, an unknown `moveOn`). This is
//! the next layer: the *semantic* rules the cmi5 specification places on a
//! well-formed document — the ones a schema check alone would not catch. In
//! particular cmi5 requires every AU and block `id` to be an IRI and to be
//! unique within the course, and objective references to resolve to a declared
//! objective. A package that broke these would parse but behave incoherently
//! (two AUs sharing an id cannot be told apart when a statement arrives), so it
//! is refused up front.

use crate::manifest::{Block, CourseStructure, Node};
use std::collections::HashSet;

/// A way in which a course structure fails cmi5 conformance. Named per rule so
/// the import path can report exactly what is wrong with a package and tests can
/// assert the specific rejection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConformanceError {
    #[error("the course id is missing or is not an IRI: {0:?}")]
    CourseIdNotIri(String),
    #[error("the course has no assignable units")]
    NoAssignableUnits,
    #[error("assignable unit id is missing or is not an IRI: {0:?}")]
    AuIdNotIri(String),
    #[error("block id is missing or is not an IRI: {0:?}")]
    BlockIdNotIri(String),
    #[error("duplicate assignable unit id: {0}")]
    DuplicateAuId(String),
    #[error("duplicate block id: {0}")]
    DuplicateBlockId(String),
    #[error("assignable unit {0} has an empty launch url")]
    EmptyAuUrl(String),
    #[error("objective reference '{reference}' on {owner} resolves to no declared objective")]
    DanglingObjectiveRef { owner: String, reference: String },
}

/// Whether a string is acceptable as a cmi5 id: an IRI, i.e. non-empty, no
/// whitespace, and carrying a scheme (a `:`). Deliberately lenient — it rejects
/// a bare token like `au1` while accepting any real IRI, rather than trying to
/// re-implement RFC 3987.
fn is_iri(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s.contains(':') && !s.chars().any(char::is_whitespace)
}

/// Validate a course structure against cmi5's semantic rules. Returns every
/// violation found (empty means conformant), so an author sees all the problems
/// at once rather than one per re-upload.
pub fn validate_course_structure(cs: &CourseStructure) -> Vec<ConformanceError> {
    let mut errors = Vec::new();

    if !is_iri(&cs.course.id) {
        errors.push(ConformanceError::CourseIdNotIri(cs.course.id.clone()));
    }

    let declared_objectives: HashSet<&str> = cs.objectives.iter().map(|o| o.id.as_str()).collect();

    let aus = cs.assignable_units();
    if aus.is_empty() {
        errors.push(ConformanceError::NoAssignableUnits);
    }

    let mut seen_au_ids: HashSet<&str> = HashSet::new();
    for au in &aus {
        if !is_iri(&au.id) {
            errors.push(ConformanceError::AuIdNotIri(au.id.clone()));
        } else if !seen_au_ids.insert(au.id.as_str()) {
            errors.push(ConformanceError::DuplicateAuId(au.id.clone()));
        }
        if au.url.trim().is_empty() {
            errors.push(ConformanceError::EmptyAuUrl(au.id.clone()));
        }
        for reference in &au.objective_ids {
            if !declared_objectives.contains(reference.as_str()) {
                errors.push(ConformanceError::DanglingObjectiveRef {
                    owner: au.id.clone(),
                    reference: reference.clone(),
                });
            }
        }
    }

    let mut seen_block_ids: HashSet<&str> = HashSet::new();
    check_blocks(
        &cs.nodes,
        &declared_objectives,
        &mut seen_block_ids,
        &mut errors,
    );

    errors
}

fn check_blocks<'a>(
    nodes: &'a [Node],
    declared_objectives: &HashSet<&str>,
    seen_block_ids: &mut HashSet<&'a str>,
    errors: &mut Vec<ConformanceError>,
) {
    for node in nodes {
        if let Node::Block(block) = node {
            validate_block(block, declared_objectives, seen_block_ids, errors);
            check_blocks(&block.children, declared_objectives, seen_block_ids, errors);
        }
    }
}

fn validate_block<'a>(
    block: &'a Block,
    declared_objectives: &HashSet<&str>,
    seen_block_ids: &mut HashSet<&'a str>,
    errors: &mut Vec<ConformanceError>,
) {
    if !is_iri(&block.id) {
        errors.push(ConformanceError::BlockIdNotIri(block.id.clone()));
    } else if !seen_block_ids.insert(block.id.as_str()) {
        errors.push(ConformanceError::DuplicateBlockId(block.id.clone()));
    }
    for reference in &block.objective_ids {
        if !declared_objectives.contains(reference.as_str()) {
            errors.push(ConformanceError::DanglingObjectiveRef {
                owner: block.id.clone(),
                reference: reference.clone(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::parse_manifest;

    const CONFORMANT: &str = r#"<courseStructure xmlns="x">
        <course id="https://example.com/course/1">
            <title><langstring lang="en">C</langstring></title>
        </course>
        <objectives>
            <objective id="https://example.com/obj/1"><title><langstring lang="en">O</langstring></title></objective>
        </objectives>
        <block id="https://example.com/block/1">
            <objectives><objective idref="https://example.com/obj/1"/></objectives>
            <au id="https://example.com/au/1" moveOn="Passed"><url>a1.html</url></au>
        </block>
        <au id="https://example.com/au/2" moveOn="Completed"><url>a2.html</url></au>
    </courseStructure>"#;

    fn validate(xml: &str) -> Vec<ConformanceError> {
        validate_course_structure(&parse_manifest(xml).expect("parse"))
    }

    #[test]
    fn a_conformant_course_has_no_errors() {
        assert_eq!(validate(CONFORMANT), Vec::new());
    }

    #[test]
    fn a_non_iri_course_id_is_rejected() {
        let xml = r#"<courseStructure xmlns="x"><course id="course1"/>
            <au id="https://x/au/1" moveOn="Passed"><url>a.html</url></au></courseStructure>"#;
        assert!(validate(xml).contains(&ConformanceError::CourseIdNotIri("course1".into())));
    }

    #[test]
    fn a_non_iri_au_id_is_rejected() {
        let xml = r#"<courseStructure xmlns="x"><course id="https://x/c"/>
            <au id="au1" moveOn="Passed"><url>a.html</url></au></courseStructure>"#;
        assert!(validate(xml).contains(&ConformanceError::AuIdNotIri("au1".into())));
    }

    #[test]
    fn duplicate_au_ids_are_rejected() {
        let xml = r#"<courseStructure xmlns="x"><course id="https://x/c"/>
            <au id="https://x/au/1" moveOn="Passed"><url>a.html</url></au>
            <au id="https://x/au/1" moveOn="Completed"><url>b.html</url></au></courseStructure>"#;
        assert!(validate(xml).contains(&ConformanceError::DuplicateAuId("https://x/au/1".into())));
    }

    #[test]
    fn a_course_with_no_aus_is_rejected() {
        let xml = r#"<courseStructure xmlns="x"><course id="https://x/c"/></courseStructure>"#;
        assert!(validate(xml).contains(&ConformanceError::NoAssignableUnits));
    }

    #[test]
    fn a_dangling_objective_reference_is_rejected() {
        let xml = r#"<courseStructure xmlns="x"><course id="https://x/c"/>
            <au id="https://x/au/1" moveOn="Passed">
                <objectives><objective idref="https://x/obj/missing"/></objectives>
                <url>a.html</url>
            </au></courseStructure>"#;
        assert!(validate(xml).iter().any(|e| matches!(
            e,
            ConformanceError::DanglingObjectiveRef { reference, .. } if reference == "https://x/obj/missing"
        )));
    }
}
