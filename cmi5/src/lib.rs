//! Pure cmi5 / xAPI domain logic.
//!
//! This crate owns everything about the cmi5 specification that does not need a
//! database, an HTTP server, or the application's `AppState`:
//!
//! - [`manifest`] — the `cmi5.xml` course-structure model, plus [`manifest::parse_manifest`]
//!   and [`manifest::to_manifest_xml`] (both directions; export needs to write).
//! - [`statement`] — the xAPI statement model and the verb / cmi5-category IRI
//!   constants the LMS and the content exchange.
//! - [`validation`] — [`validation::validate_cmi5_statement`], the per-session
//!   [`validation::SessionState`] sequence machine, and
//!   [`validation::evaluate_move_on`], which decides when an AU is satisfied.
//! - [`launch`] — [`launch::build_launch_query`] and the `LMS.LaunchData`
//!   document the content reads back through the State API.
//!
//! The server crate turns these into rows, routes, and tool-access grants; none
//! of that leaks in here, which is what keeps the spec logic unit-testable on
//! its own (including on the FreeBSD workstation tier, which has no database).

pub mod launch;
pub mod manifest;
pub mod statement;
pub mod validation;

pub use launch::{LaunchData, LaunchMode, build_launch_query};
pub use manifest::{
    AssignableUnit, Block, Course, CourseStructure, LangString, LaunchMethod, ManifestError,
    MoveOn, Node, Objective, parse_manifest, to_manifest_xml,
};
pub use statement::{
    Account, Activity, Agent, Context, ContextActivities, Score, Statement, StatementObject,
    XResult, verbs,
};
pub use validation::{
    SessionExpectation, SessionOutcome, SessionState, Violation, evaluate_move_on,
    validate_cmi5_statement,
};
