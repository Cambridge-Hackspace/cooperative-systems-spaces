//! The cmi5 subsystem service: package import, content storage, and the admin
//! management operations.
//!
//! This is the server-side half of the cmi5 feature. The pure specification
//! logic (manifest parsing, statement validation, moveOn) lives in the `cmi5`
//! crate; here we drive it: unpack an uploaded package to the filesystem content
//! store, persist its course/block/AU tree, list/get/delete courses, and bind an
//! AU to a training step so a verified pass grants physical tool access.
//!
//! Keeping all cmi5 database access in this module (rather than spread across the
//! shared `database.rs`) is deliberate: it is what lets the feature read as one
//! cohesive unit. The one cross-feature reach is the grant itself, which in a
//! later stage goes through the existing training-completion path so the
//! tool-access invariant that `tool_access_agrees` guards stays authoritative.

use std::io::{Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;
use zip::write::{SimpleFileOptions, ZipWriter};
use zip::ZipArchive;

// Leading `::` names the external `cmi5` crate unambiguously, distinct from this
// module (`crate::cmi5`) and `crate::api::cmi5`.
use ::cmi5::{
    append_query, build_launch_query, categories, evaluate_move_on, parse_manifest,
    validate_cmi5_statement, verbs, Account, Activity, Agent, Context, ContextActivities,
    LangString, LaunchData, LaunchMode, LaunchParams, ManifestError, MoveOn, Node,
    SessionExpectation, SessionState, Statement, StatementObject, Verb, Violation,
};

use crate::config::Cmi5Config;
use crate::database::DatabaseManager;
use crate::models::{
    AssignCmi5AuStep, Cmi5AssignableUnit, Cmi5Block, Cmi5Course, Cmi5Registration,
    NewCmi5AssignableUnit, NewCmi5Block, NewCmi5Course, NewCmi5LaunchToken, NewCmi5Registration,
    NewCmi5StateDocument, NewCmi5Statement, NewTrainingRecord, TrainingStep,
};
use crate::schema::{
    cmi5_assignable_units, cmi5_blocks, cmi5_courses, cmi5_launch_tokens, cmi5_registrations,
    cmi5_state_documents, cmi5_statements, training_steps,
};
use crate::tokens::{generate_token, hash_token};

/// What can go wrong in a cmi5 service operation. Named per cause so the API
/// layer can map each to the right status and the tests can assert the specific
/// rejection (notably the zip-slip and requires-assessment defenses).
#[derive(Debug, thiserror::Error)]
pub enum Cmi5Error {
    #[error("the cmi5 subsystem is disabled")]
    Disabled,
    #[error("package is {size} bytes, over the {max}-byte limit")]
    TooLarge { size: usize, max: usize },
    #[error("not a readable zip archive: {0}")]
    Zip(String),
    #[error("package has no cmi5.xml at its root")]
    NoManifest,
    #[error("invalid cmi5.xml: {0}")]
    Manifest(#[from] ManifestError),
    #[error("package entry '{0}' escapes the content directory")]
    ZipSlip(String),
    #[error("filesystem error: {0}")]
    Io(String),
    #[error("database pool error: {0}")]
    Pool(String),
    #[error("database error: {0}")]
    Db(#[from] diesel::result::Error),
    #[error("no such cmi5 course")]
    CourseNotFound,
    #[error("no such assignable unit")]
    AuNotFound,
    #[error("no such training step")]
    StepNotFound,
    #[error("a cmi5 module cannot satisfy a step that requires an assessment")]
    StepRequiresAssessment,
    #[error("an AU with moveOn=NotApplicable can never satisfy, so it cannot gate a tool")]
    MoveOnNotApplicable,
    #[error("the fetch token is unknown, already used, or expired")]
    FetchTokenInvalid,
    #[error("serialization error: {0}")]
    Json(String),
    #[error("malformed xAPI statement: {0}")]
    BadStatement(String),
    #[error("statement rejected: {0}")]
    Rejected(Violation),
}

impl From<crate::database::DatabaseError> for Cmi5Error {
    fn from(e: crate::database::DatabaseError) -> Self {
        match e {
            crate::database::DatabaseError::Diesel(d) => Cmi5Error::Db(d),
            // Pool/timeout/migration/other are all genuine server-side database
            // faults; fold them onto the existing pool arm rather than adding a
            // new blanket-500 site.
            other => Cmi5Error::Pool(other.to_string()),
        }
    }
}

/// The cmi5 service. Holds a database handle and the content-store settings
/// captured at startup. The `enabled` gate is checked against live config by the
/// handlers, so toggling it takes effect without a restart.
#[derive(Clone)]
pub struct Cmi5Service {
    db: Arc<DatabaseManager>,
    content_dir: PathBuf,
    max_package_bytes: usize,
    fetch_ttl: chrono::Duration,
    session_ttl: chrono::Duration,
}

/// The server-side truth about a launched session, resolved from a session
/// credential. This is what the LRS routes authorize every statement against.
#[derive(Debug, Clone)]
pub struct Cmi5SessionContext {
    pub registration_id: Uuid,
    pub user_id: Uuid,
    pub au_id: Uuid,
    /// The AU's activity IRI: the only activity this session may write about.
    pub activity_id: String,
    /// Normal / Browse / Review. Only Normal may satisfy moveOn.
    pub launch_mode: String,
}

/// The result of minting a launch: the URL for the SPA to open, and the
/// registration it belongs to.
#[derive(Debug, Clone)]
pub struct LaunchResult {
    pub launch_url: String,
    pub registration_id: Uuid,
}

/// Returned when an accepted statement satisfied an AU and a training-step grant
/// was written. The LRS handler uses it to audit and to broadcast the new
/// tool-access state to edge devices.
#[derive(Debug, Clone)]
pub struct GrantInfo {
    pub user_id: Uuid,
    pub au_id: Uuid,
    pub registration_id: Uuid,
    pub training_step_id: Uuid,
    pub tool_id: Uuid,
    pub score: Option<i32>,
}

impl Cmi5Service {
    pub fn new(db: Arc<DatabaseManager>, config: &Cmi5Config) -> Self {
        Self {
            db,
            content_dir: PathBuf::from(&config.content_dir),
            max_package_bytes: config.max_package_bytes,
            fetch_ttl: chrono::Duration::seconds(config.fetch_ttl_secs as i64),
            session_ttl: chrono::Duration::seconds(config.session_ttl_secs as i64),
        }
    }

    /// The directory a course's content is served from.
    pub fn course_content_dir(&self, content_path: &str) -> PathBuf {
        self.content_dir.join(content_path)
    }

    fn conn(
        &self,
    ) -> Result<
        diesel::r2d2::PooledConnection<diesel::r2d2::ConnectionManager<PgConnection>>,
        Cmi5Error,
    > {
        self.db
            .pool()
            .get()
            .map_err(|e| Cmi5Error::Pool(e.to_string()))
    }

    /// Import an uploaded `.zip` package: validate size, parse and validate the
    /// manifest, extract content to the store (zip-slip guarded), and persist the
    /// course/block/AU tree. The row id and the content directory share a
    /// freshly minted UUID so they cannot drift.
    pub fn import_package(
        &self,
        zip_bytes: &[u8],
        imported_by: Uuid,
    ) -> Result<Cmi5Course, Cmi5Error> {
        if zip_bytes.len() > self.max_package_bytes {
            return Err(Cmi5Error::TooLarge {
                size: zip_bytes.len(),
                max: self.max_package_bytes,
            });
        }

        let mut archive =
            ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| Cmi5Error::Zip(e.to_string()))?;

        // Read and validate the manifest before writing anything to disk.
        let manifest_xml = {
            let mut f = archive
                .by_name("cmi5.xml")
                .map_err(|_| Cmi5Error::NoManifest)?;
            let mut s = String::new();
            f.read_to_string(&mut s)
                .map_err(|e| Cmi5Error::Io(e.to_string()))?;
            s
        };
        let structure = parse_manifest(&manifest_xml)?;

        // The id names both the row and the content directory.
        let course_id = Uuid::new_v4();
        let dest = self.content_dir.join(course_id.to_string());
        extract_all(&mut archive, &dest)?;

        // Persist the tree. On any failure, remove the extracted files so a
        // failed import leaves nothing behind.
        let mut conn = match self.conn() {
            Ok(c) => c,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dest);
                return Err(e);
            }
        };
        let manifest_for_row = manifest_xml.clone();
        let result = conn.transaction::<Cmi5Course, diesel::result::Error, _>(|conn| {
            let new_course = NewCmi5Course {
                id: course_id,
                course_iri: structure.course.id.clone(),
                title: first_lang(&structure.course.title),
                description: first_lang(&structure.course.description),
                content_path: course_id.to_string(),
                manifest_xml: manifest_for_row,
                imported_by: Some(imported_by),
            };
            let course: Cmi5Course = diesel::insert_into(cmi5_courses::table)
                .values(&new_course)
                .get_result(conn)?;
            insert_nodes(conn, course_id, None, &structure.nodes)?;
            Ok(course)
        });

        match result {
            Ok(course) => Ok(course),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dest);
                Err(Cmi5Error::Db(e))
            }
        }
    }

    /// Every course that has not been deleted, newest first.
    pub fn list_courses(&self) -> Result<Vec<Cmi5Course>, Cmi5Error> {
        let mut conn = self.conn()?;
        let courses = cmi5_courses::table
            .filter(cmi5_courses::deleted_at.is_null())
            .order(cmi5_courses::created_at.desc())
            .select(Cmi5Course::as_select())
            .load(&mut conn)?;
        Ok(courses)
    }

    /// A single non-deleted course by id.
    pub fn get_course(&self, id: Uuid) -> Result<Cmi5Course, Cmi5Error> {
        let mut conn = self.conn()?;
        cmi5_courses::table
            .filter(cmi5_courses::id.eq(id))
            .filter(cmi5_courses::deleted_at.is_null())
            .select(Cmi5Course::as_select())
            .first(&mut conn)
            .optional()?
            .ok_or(Cmi5Error::CourseNotFound)
    }

    /// The AUs of a course, in document order.
    pub fn list_aus(&self, course_id: Uuid) -> Result<Vec<Cmi5AssignableUnit>, Cmi5Error> {
        let mut conn = self.conn()?;
        let aus = cmi5_assignable_units::table
            .filter(cmi5_assignable_units::course_id.eq(course_id))
            .order(cmi5_assignable_units::position.asc())
            .select(Cmi5AssignableUnit::as_select())
            .load(&mut conn)?;
        Ok(aus)
    }

    /// Soft-delete a course and remove its content directory. The row is kept
    /// (audit/history) but marked deleted; the files are pruned since a deleted
    /// course can no longer be launched or exported.
    pub fn delete_course(&self, id: Uuid) -> Result<(), Cmi5Error> {
        let mut conn = self.conn()?;
        let course = self.get_course(id)?;
        let affected = diesel::update(
            cmi5_courses::table
                .filter(cmi5_courses::id.eq(id))
                .filter(cmi5_courses::deleted_at.is_null()),
        )
        .set(cmi5_courses::deleted_at.eq(Some(Utc::now())))
        .execute(&mut conn)?;
        if affected == 0 {
            return Err(Cmi5Error::CourseNotFound);
        }
        let dir = self.course_content_dir(&course.content_path);
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            // A missing directory is fine; anything else is worth a line but not
            // worth failing a delete whose database half already succeeded.
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("cmi5: could not remove content dir {:?}: {}", dir, e);
            }
        }
        Ok(())
    }

    /// Bind (or, with `None`, unbind) an AU to a training step.
    ///
    /// Two fail-closed refusals guard the access gate: a step that
    /// `requires_assessment` cannot be satisfied by a browser course (a click-
    /// through is not a practical), and an AU whose moveOn is `NotApplicable`
    /// can never satisfy, so binding it would promise access the learner can
    /// never actually earn.
    pub fn assign_au_step(
        &self,
        au_id: Uuid,
        step_id: Option<Uuid>,
    ) -> Result<Cmi5AssignableUnit, Cmi5Error> {
        let mut conn = self.conn()?;
        let au: Cmi5AssignableUnit = cmi5_assignable_units::table
            .filter(cmi5_assignable_units::id.eq(au_id))
            .select(Cmi5AssignableUnit::as_select())
            .first(&mut conn)
            .optional()?
            .ok_or(Cmi5Error::AuNotFound)?;

        if let Some(sid) = step_id {
            let step: TrainingStep = training_steps::table
                .filter(training_steps::id.eq(sid))
                .select(TrainingStep::as_select())
                .first(&mut conn)
                .optional()?
                .ok_or(Cmi5Error::StepNotFound)?;
            if step.requires_assessment {
                return Err(Cmi5Error::StepRequiresAssessment);
            }
            if au.move_on == MoveOn::NotApplicable.as_str() {
                return Err(Cmi5Error::MoveOnNotApplicable);
            }
        }

        let updated: Cmi5AssignableUnit = diesel::update(
            cmi5_assignable_units::table.filter(cmi5_assignable_units::id.eq(au_id)),
        )
        .set(&AssignCmi5AuStep {
            training_step_id: step_id,
            updated_at: Utc::now(),
        })
        .get_result(&mut conn)?;
        Ok(updated)
    }

    /// Mint a launch for `user_id` against `au_id`.
    ///
    /// Creates the registration and a one-time fetch token, writes the
    /// `LMS.LaunchData` state document and the LMS-issued `launched` statement,
    /// and returns the fully-formed launch URL. The actor is derived here from
    /// the authenticated user, never from the request, so a learner cannot launch
    /// as anyone else. Launch mode is always `Normal` (Browse/Review are not
    /// offered — they must never lead to a tool grant).
    pub fn create_launch(
        &self,
        au_id: Uuid,
        user_id: Uuid,
        site_url: &str,
    ) -> Result<LaunchResult, Cmi5Error> {
        let mut conn = self.conn()?;

        let au: Cmi5AssignableUnit = cmi5_assignable_units::table
            .filter(cmi5_assignable_units::id.eq(au_id))
            .select(Cmi5AssignableUnit::as_select())
            .first(&mut conn)
            .optional()?
            .ok_or(Cmi5Error::AuNotFound)?;
        let course: Cmi5Course = cmi5_courses::table
            .filter(cmi5_courses::id.eq(au.course_id))
            .filter(cmi5_courses::deleted_at.is_null())
            .select(Cmi5Course::as_select())
            .first(&mut conn)
            .optional()?
            .ok_or(Cmi5Error::CourseNotFound)?;

        let registration_id = Uuid::new_v4();
        let actor_name = user_id.to_string();
        let now = Utc::now();
        let (fetch_plaintext, fetch_hash) = generate_token();

        // The actor and the mandated launch parameters.
        let actor = Agent {
            object_type: Some("Agent".to_string()),
            name: None,
            mbox: None,
            account: Some(Account {
                home_page: site_url.to_string(),
                name: actor_name.clone(),
            }),
        };
        let endpoint = format!("{site_url}/api/cmi5/lrs");
        let fetch = format!("{site_url}/api/cmi5/fetch?token={fetch_plaintext}");
        let move_on = MoveOn::parse(&au.move_on)?;

        // LMS.LaunchData for the content to read back through the State API.
        let launch_data = LaunchData::new(
            registration_id,
            LaunchMode::Normal,
            move_on,
            au.mastery_score,
        );
        let launch_data_json =
            serde_json::to_value(&launch_data).map_err(|e| Cmi5Error::Json(e.to_string()))?;

        // The LMS-issued `launched` statement.
        let statement_id = Uuid::new_v4();
        let launched = Statement {
            id: Some(statement_id),
            actor: actor.clone(),
            verb: Verb {
                id: verbs::LAUNCHED.to_string(),
                display: None,
            },
            object: StatementObject::Activity(Activity {
                object_type: Some("Activity".to_string()),
                id: au.au_iri.clone(),
                definition: None,
            }),
            result: None,
            context: Some(Context {
                registration: Some(registration_id),
                context_activities: Some(ContextActivities {
                    category: Some(vec![Activity {
                        object_type: None,
                        id: categories::CMI5.to_string(),
                        definition: None,
                    }]),
                    parent: None,
                    grouping: None,
                    other: None,
                }),
                extensions: None,
            }),
            timestamp: Some(now),
        };
        let launched_json =
            serde_json::to_value(&launched).map_err(|e| Cmi5Error::Json(e.to_string()))?;

        // Build the launch URL: content base + AU url + the cmi5 query.
        let query = build_launch_query(&LaunchParams {
            endpoint: &endpoint,
            fetch: &fetch,
            actor: &actor,
            registration: registration_id,
            activity_id: &au.au_iri,
        })
        .map_err(|e| Cmi5Error::Json(e.to_string()))?;
        let content_base = format!(
            "{site_url}/cmi5-content/{}/{}",
            course.content_path,
            au.launch_url.trim_start_matches('/')
        );
        // Opaque launchParameters, if any, precede the cmi5 params.
        let with_params = match &au.launch_parameters {
            Some(p) if !p.is_empty() => append_query(&content_base, p),
            _ => content_base,
        };
        let launch_url = append_query(&with_params, &query);

        conn.transaction::<(), diesel::result::Error, _>(|conn| {
            diesel::insert_into(cmi5_registrations::table)
                .values(&NewCmi5Registration {
                    id: registration_id,
                    user_id,
                    au_id,
                    actor_account_name: actor_name.clone(),
                    launch_mode: LaunchMode::Normal.as_str().to_string(),
                })
                .execute(conn)?;
            diesel::insert_into(cmi5_launch_tokens::table)
                .values(&NewCmi5LaunchToken {
                    registration_id,
                    fetch_token_hash: fetch_hash,
                    expires_at: now + self.fetch_ttl,
                    session_expires_at: now + self.session_ttl,
                })
                .execute(conn)?;
            diesel::insert_into(cmi5_state_documents::table)
                .values(&NewCmi5StateDocument {
                    registration_id,
                    activity_iri: au.au_iri.clone(),
                    agent_account_name: actor_name.clone(),
                    state_id: "LMS.LaunchData".to_string(),
                    document: launch_data_json,
                    etag: Uuid::new_v4().to_string(),
                })
                .execute(conn)?;
            diesel::insert_into(cmi5_statements::table)
                .values(&NewCmi5Statement {
                    registration_id,
                    statement_id,
                    verb_iri: verbs::LAUNCHED.to_string(),
                    statement: launched_json,
                })
                .execute(conn)?;
            Ok(())
        })?;

        Ok(LaunchResult {
            launch_url,
            registration_id,
        })
    }

    /// Exchange a one-time fetch token for a session credential.
    ///
    /// The claim is atomic and single-use, copied from the device-invite path:
    /// the `WHERE fetch_consumed_at IS NULL AND expires_at > now()` plus the
    /// affected-row check — not the preceding read — is what makes a second fetch
    /// fail under concurrency. Returns the plaintext session token, whose hash is
    /// what the LRS extractor later resolves.
    pub fn consume_fetch(&self, fetch_plaintext: &str) -> Result<String, Cmi5Error> {
        let mut conn = self.conn()?;
        let fetch_hash = hash_token(fetch_plaintext);
        let (session_plaintext, session_hash) = generate_token();
        let now = Utc::now();

        let affected = diesel::update(
            cmi5_launch_tokens::table
                .filter(cmi5_launch_tokens::fetch_token_hash.eq(&fetch_hash))
                .filter(cmi5_launch_tokens::fetch_consumed_at.is_null())
                .filter(cmi5_launch_tokens::expires_at.gt(now)),
        )
        .set((
            cmi5_launch_tokens::fetch_consumed_at.eq(Some(now)),
            cmi5_launch_tokens::session_token_hash.eq(Some(session_hash)),
        ))
        .execute(&mut conn)?;

        if affected == 1 {
            Ok(session_plaintext)
        } else {
            Err(Cmi5Error::FetchTokenInvalid)
        }
    }

    /// Resolve a session credential to the server-side session truth, or `None`
    /// if the token is unknown or the session has expired. This is what the LRS
    /// extractor calls; it never trusts anything the content sent.
    pub fn resolve_session(
        &self,
        session_plaintext: &str,
    ) -> Result<Option<Cmi5SessionContext>, Cmi5Error> {
        let mut conn = self.conn()?;
        let session_hash = hash_token(session_plaintext);
        let now = Utc::now();

        let row: Option<(Uuid, Uuid, Uuid, String, String)> = cmi5_launch_tokens::table
            .inner_join(
                cmi5_registrations::table
                    .on(cmi5_registrations::id.eq(cmi5_launch_tokens::registration_id)),
            )
            .inner_join(
                cmi5_assignable_units::table
                    .on(cmi5_assignable_units::id.eq(cmi5_registrations::au_id)),
            )
            .filter(cmi5_launch_tokens::session_token_hash.eq(&session_hash))
            .filter(cmi5_launch_tokens::session_expires_at.gt(now))
            .select((
                cmi5_registrations::id,
                cmi5_registrations::user_id,
                cmi5_registrations::au_id,
                cmi5_assignable_units::au_iri,
                cmi5_registrations::launch_mode,
            ))
            .first(&mut conn)
            .optional()?;

        Ok(row.map(
            |(registration_id, user_id, au_id, activity_id, launch_mode)| Cmi5SessionContext {
                registration_id,
                user_id,
                au_id,
                activity_id,
                launch_mode,
            },
        ))
    }

    /// Record one content-issued xAPI statement, and grant the mapped training
    /// step if it satisfies the AU.
    ///
    /// This is the core of the security boundary, and every check that decides
    /// whether the statement may count runs against `session` — the server-side
    /// truth — never against anything the content chose. In order:
    ///
    /// 1. parse and validate against the session (actor/registration/activity
    ///    binding, cmi5 category, verb legality, masteryScore) via the pure crate;
    /// 2. replay the session's prior statements through the sequence machine and
    ///    apply this one, enforcing initialized-first / terminated-last / no
    ///    double-outcome;
    /// 3. store it (a duplicate `statement_id` is an idempotent no-op, not a
    ///    second grant);
    /// 4. if the accumulated outcome satisfies the AU's moveOn — and the launch
    ///    was Normal, the AU is mapped to a step, and the registration is not
    ///    already satisfied — write the grant through the shared
    ///    `create_training_record` path and mark the registration satisfied.
    ///
    /// Returns `Some(GrantInfo)` exactly when a grant was written, so the handler
    /// can audit it and broadcast the new tool-access state.
    pub fn record_statement(
        &self,
        session: &Cmi5SessionContext,
        site_url: &str,
        statement_id_hint: Option<Uuid>,
        raw: serde_json::Value,
    ) -> Result<Option<GrantInfo>, Cmi5Error> {
        let stmt: Statement = serde_json::from_value(raw.clone())
            .map_err(|e| Cmi5Error::BadStatement(e.to_string()))?;

        let mut conn = self.conn()?;

        let au: Cmi5AssignableUnit = cmi5_assignable_units::table
            .filter(cmi5_assignable_units::id.eq(session.au_id))
            .select(Cmi5AssignableUnit::as_select())
            .first(&mut conn)
            .optional()?
            .ok_or(Cmi5Error::AuNotFound)?;

        // 1. Validate against the session. Identity/binding first, so a forged
        //    statement fails as "wrong actor/activity", not by leaking its score.
        let expect = SessionExpectation {
            actor_home_page: site_url.to_string(),
            actor_account_name: session.user_id.to_string(),
            registration: session.registration_id,
            activity_id: session.activity_id.clone(),
            mastery_score: au.mastery_score,
        };
        validate_cmi5_statement(&stmt, &expect).map_err(Cmi5Error::Rejected)?;

        // Idempotency: a statement id already stored is a replay -> no-op accept.
        let statement_id = statement_id_hint.or(stmt.id).unwrap_or_else(Uuid::new_v4);
        let already: Option<Uuid> = cmi5_statements::table
            .filter(cmi5_statements::statement_id.eq(statement_id))
            .select(cmi5_statements::statement_id)
            .first(&mut conn)
            .optional()?;
        if already.is_some() {
            return Ok(None);
        }

        // 2. Rebuild the session state from prior statements, then apply this one.
        let prior: Vec<serde_json::Value> = cmi5_statements::table
            .filter(cmi5_statements::registration_id.eq(session.registration_id))
            .order(cmi5_statements::stored.asc())
            .select(cmi5_statements::statement)
            .load(&mut conn)?;
        let mut machine = SessionState::new();
        for value in &prior {
            // Only AU-issued verbs advance the sequence machine; the LMS-issued
            // `launched`/`satisfied` are skipped. Prior statements were accepted
            // already, so a replay error here is not the caller's to answer for.
            if let Ok(s) = serde_json::from_value::<Statement>(value.clone()) {
                let _ = machine.apply(&s);
            }
        }
        machine.apply(&stmt).map_err(Cmi5Error::Rejected)?;

        // 3. Store it.
        diesel::insert_into(cmi5_statements::table)
            .values(&NewCmi5Statement {
                registration_id: session.registration_id,
                statement_id,
                verb_iri: stmt.verb_id().to_string(),
                statement: raw,
            })
            .execute(&mut conn)?;

        // 4. Grant on satisfaction.
        let move_on = MoveOn::parse(&au.move_on)?;
        let creditable = session.launch_mode == LaunchMode::Normal.as_str();
        if !creditable || !evaluate_move_on(move_on, &machine.outcome()) {
            return Ok(None);
        }
        let Some(step_id) = au.training_step_id else {
            return Ok(None);
        };
        let registration: Cmi5Registration = cmi5_registrations::table
            .filter(cmi5_registrations::id.eq(session.registration_id))
            .select(Cmi5Registration::as_select())
            .first(&mut conn)?;
        if registration.satisfied_at.is_some() {
            return Ok(None);
        }

        let score = statement_score(&stmt);
        let grant = self.grant_step(step_id, session, &au, score)?;

        let now = Utc::now();
        let outcome = machine.outcome();
        diesel::update(
            cmi5_registrations::table.filter(cmi5_registrations::id.eq(session.registration_id)),
        )
        .set((
            cmi5_registrations::satisfied_at.eq(Some(now)),
            cmi5_registrations::passed_at.eq(outcome.passed.then_some(now)),
            cmi5_registrations::completed_at.eq(outcome.completed.then_some(now)),
            cmi5_registrations::updated_at.eq(now),
        ))
        .execute(&mut conn)?;

        Ok(Some(grant))
    }

    /// Write the tool-access grant for a satisfied AU, through the *shared*
    /// training-completion path so the web and edge access checks stay in
    /// agreement (`tool_access_agrees`). `create_training_record` upserts the
    /// `user_training_progress` row to Completed with the step's expiry — the
    /// exact row `can_access_tool` reads — and records a training_records entry.
    fn grant_step(
        &self,
        step_id: Uuid,
        session: &Cmi5SessionContext,
        au: &Cmi5AssignableUnit,
        score: Option<i32>,
    ) -> Result<GrantInfo, Cmi5Error> {
        let step = self
            .db
            .get_training_step_by_id(step_id)?
            .ok_or(Cmi5Error::StepNotFound)?;

        let record = NewTrainingRecord {
            tool_id: step.tool_id,
            training_step_id: Some(step_id),
            trainee_user_id: session.user_id,
            // Self-directed, system-verified completion: the learner is both the
            // subject and, for the record, the actor. The note names the cmi5
            // module so history reads as "completed the cmi5 course", not "was
            // signed off by a trainer".
            trainer_user_id: session.user_id,
            training_date: Utc::now().date_naive(),
            completion_status: "completed".to_string(),
            minutes_trained: None,
            skills_covered: None,
            notes: Some(format!("Completed via cmi5 module {}", au.au_iri)),
            next_steps: None,
        };
        self.db.create_training_record(&record)?;

        Ok(GrantInfo {
            user_id: session.user_id,
            au_id: au.id,
            registration_id: session.registration_id,
            training_step_id: step_id,
            tool_id: step.tool_id,
            score,
        })
    }

    /// Read a State API document, if present.
    pub fn get_state_document(
        &self,
        registration_id: Uuid,
        activity_iri: &str,
        agent_account_name: &str,
        state_id: &str,
    ) -> Result<Option<serde_json::Value>, Cmi5Error> {
        let mut conn = self.conn()?;
        let doc: Option<serde_json::Value> = cmi5_state_documents::table
            .filter(cmi5_state_documents::registration_id.eq(registration_id))
            .filter(cmi5_state_documents::activity_iri.eq(activity_iri))
            .filter(cmi5_state_documents::agent_account_name.eq(agent_account_name))
            .filter(cmi5_state_documents::state_id.eq(state_id))
            .select(cmi5_state_documents::document)
            .first(&mut conn)
            .optional()?;
        Ok(doc)
    }

    /// Create or replace a State API document.
    pub fn put_state_document(
        &self,
        registration_id: Uuid,
        activity_iri: &str,
        agent_account_name: &str,
        state_id: &str,
        document: serde_json::Value,
    ) -> Result<(), Cmi5Error> {
        let mut conn = self.conn()?;
        let now = Utc::now();
        diesel::insert_into(cmi5_state_documents::table)
            .values(&NewCmi5StateDocument {
                registration_id,
                activity_iri: activity_iri.to_string(),
                agent_account_name: agent_account_name.to_string(),
                state_id: state_id.to_string(),
                document: document.clone(),
                etag: Uuid::new_v4().to_string(),
            })
            .on_conflict((
                cmi5_state_documents::registration_id,
                cmi5_state_documents::activity_iri,
                cmi5_state_documents::agent_account_name,
                cmi5_state_documents::state_id,
            ))
            .do_update()
            .set((
                cmi5_state_documents::document.eq(document),
                cmi5_state_documents::etag.eq(Uuid::new_v4().to_string()),
                cmi5_state_documents::updated_at.eq(now),
            ))
            .execute(&mut conn)?;
        Ok(())
    }

    /// Delete a State API document. Missing is not an error.
    pub fn delete_state_document(
        &self,
        registration_id: Uuid,
        activity_iri: &str,
        agent_account_name: &str,
        state_id: &str,
    ) -> Result<(), Cmi5Error> {
        let mut conn = self.conn()?;
        diesel::delete(
            cmi5_state_documents::table
                .filter(cmi5_state_documents::registration_id.eq(registration_id))
                .filter(cmi5_state_documents::activity_iri.eq(activity_iri))
                .filter(cmi5_state_documents::agent_account_name.eq(agent_account_name))
                .filter(cmi5_state_documents::state_id.eq(state_id)),
        )
        .execute(&mut conn)?;
        Ok(())
    }

    /// Read a stored statement by id, scoped to a registration.
    pub fn get_statement(
        &self,
        registration_id: Uuid,
        statement_id: Uuid,
    ) -> Result<Option<serde_json::Value>, Cmi5Error> {
        let mut conn = self.conn()?;
        let stmt: Option<serde_json::Value> = cmi5_statements::table
            .filter(cmi5_statements::registration_id.eq(registration_id))
            .filter(cmi5_statements::statement_id.eq(statement_id))
            .select(cmi5_statements::statement)
            .first(&mut conn)
            .optional()?;
        Ok(stmt)
    }

    /// Repackage a course's content directory back into a `.zip`.
    ///
    /// The directory holds exactly what was imported — `cmi5.xml` and the content
    /// files, extracted verbatim — so re-zipping it reproduces a package that
    /// re-imports to the same course tree (the round-trip the e2e stage checks).
    /// The name is the course title/id; the caller streams the bytes.
    pub fn export_package(&self, course_id: Uuid) -> Result<Vec<u8>, Cmi5Error> {
        let course = self.get_course(course_id)?;
        let dir = self.course_content_dir(&course.content_path);

        let mut buf = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut buf));
            add_dir_to_zip(&mut writer, &dir, &dir)?;
            writer.finish().map_err(|e| Cmi5Error::Zip(e.to_string()))?;
        }
        Ok(buf)
    }
}

/// The 0–100 assessment score a statement carries, from `result.score.scaled`.
fn statement_score(stmt: &Statement) -> Option<i32> {
    stmt.result
        .as_ref()
        .and_then(|r| r.score.as_ref())
        .and_then(|s| s.scaled)
        .map(|scaled| (scaled * 100.0).round().clamp(0.0, 100.0) as i32)
}

/// Extract every file entry into `dest`, refusing any entry whose path would
/// escape `dest`. `enclosed_name` returns `None` for absolute paths and `..`
/// traversal, which is the zip-slip guard; the join then stays inside `dest`.
///
/// A free function (not a method) so it can be exercised without a database.
fn extract_all<R: Read + Seek>(archive: &mut ZipArchive<R>, dest: &Path) -> Result<(), Cmi5Error> {
    std::fs::create_dir_all(dest).map_err(|e| Cmi5Error::Io(e.to_string()))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Cmi5Error::Zip(e.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| Cmi5Error::ZipSlip(entry.name().to_string()))?;
        let out_path = dest.join(&rel);
        if let Some(parent) = out_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Cmi5Error::Io(e.to_string()))?;
        }
        let mut out = std::fs::File::create(&out_path).map_err(|e| Cmi5Error::Io(e.to_string()))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| Cmi5Error::Io(e.to_string()))?;
    }
    Ok(())
}

/// Recursively add every file under `current` to `writer`, named by its path
/// relative to `base` (forward slashes), for export. Directories are walked;
/// empty ones are simply not represented, which a cmi5 package does not need.
fn add_dir_to_zip<W: Write + Seek>(
    writer: &mut ZipWriter<W>,
    base: &Path,
    current: &Path,
) -> Result<(), Cmi5Error> {
    let options = SimpleFileOptions::default();
    let entries = std::fs::read_dir(current).map_err(|e| Cmi5Error::Io(e.to_string()))?;
    for entry in entries {
        let path = entry.map_err(|e| Cmi5Error::Io(e.to_string()))?.path();
        if path.is_dir() {
            add_dir_to_zip(writer, base, &path)?;
        } else {
            let rel = path
                .strip_prefix(base)
                .map_err(|e| Cmi5Error::Io(e.to_string()))?
                .to_string_lossy()
                .replace('\\', "/");
            writer
                .start_file(rel, options)
                .map_err(|e| Cmi5Error::Zip(e.to_string()))?;
            let data = std::fs::read(&path).map_err(|e| Cmi5Error::Io(e.to_string()))?;
            writer
                .write_all(&data)
                .map_err(|e| Cmi5Error::Io(e.to_string()))?;
        }
    }
    Ok(())
}

/// The first localized string's value, if any.
fn first_lang(strings: &[LangString]) -> Option<String> {
    strings.first().map(|s| s.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::{SimpleFileOptions, ZipWriter};

    /// Build an in-memory zip from (name, contents) pairs, names written
    /// verbatim so a traversal name survives to the reader.
    fn zip_with(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = ZipWriter::new(Cursor::new(&mut buf));
            let opts = SimpleFileOptions::default();
            for (name, body) in entries {
                w.start_file(*name, opts).expect("start_file");
                w.write_all(body).expect("write");
            }
            w.finish().expect("finish");
        }
        buf
    }

    #[test]
    fn extract_all_writes_normal_entries() {
        let bytes = zip_with(&[("cmi5.xml", b"<x/>"), ("content/index.html", b"hi")]);
        let dir = std::env::temp_dir().join(format!("cmi5-extract-ok-{}", Uuid::new_v4()));
        let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).expect("open");
        extract_all(&mut archive, &dir).expect("extract");

        assert_eq!(
            std::fs::read_to_string(dir.join("content/index.html")).unwrap(),
            "hi"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extract_all_rejects_a_zip_slip_entry() {
        // An entry that climbs out of the destination. enclosed_name() must
        // refuse it; nothing may be written outside `dest`.
        let bytes = zip_with(&[("../escape.txt", b"pwned")]);
        let dir = std::env::temp_dir().join(format!("cmi5-extract-slip-{}", Uuid::new_v4()));
        let sibling = dir.parent().unwrap().join("escape.txt");
        let _ = std::fs::remove_file(&sibling);

        let mut archive = ZipArchive::new(Cursor::new(bytes.as_slice())).expect("open");
        let err = extract_all(&mut archive, &dir).expect_err("must reject traversal");
        assert!(
            matches!(err, Cmi5Error::ZipSlip(_)),
            "expected ZipSlip, got {err:?}"
        );
        assert!(
            !sibling.exists(),
            "the traversal entry escaped the destination to {sibling:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Persist a slice of course-tree nodes under an optional parent block,
/// recursing into nested blocks. `position` is the node's index among its
/// siblings, preserving document order.
fn insert_nodes(
    conn: &mut PgConnection,
    course_id: Uuid,
    block_id: Option<Uuid>,
    nodes: &[Node],
) -> Result<(), diesel::result::Error> {
    for (index, node) in nodes.iter().enumerate() {
        match node {
            Node::Au(au) => {
                let new_au = NewCmi5AssignableUnit {
                    course_id,
                    block_id,
                    au_iri: au.id.clone(),
                    title: first_lang(&au.title),
                    launch_url: au.url.clone(),
                    launch_parameters: au.launch_parameters.clone(),
                    launch_method: au.launch_method.map(|m| m.as_str().to_string()),
                    move_on: au.move_on.as_str().to_string(),
                    mastery_score: au.mastery_score,
                    position: index as i32,
                    training_step_id: None,
                };
                diesel::insert_into(cmi5_assignable_units::table)
                    .values(&new_au)
                    .execute(conn)?;
            }
            Node::Block(block) => {
                let new_block = NewCmi5Block {
                    course_id,
                    parent_block_id: block_id,
                    block_iri: Some(block.id.clone()),
                    title: first_lang(&block.title),
                    position: index as i32,
                };
                let inserted: Cmi5Block = diesel::insert_into(cmi5_blocks::table)
                    .values(&new_block)
                    .get_result(conn)?;
                insert_nodes(conn, course_id, Some(inserted.id), &block.children)?;
            }
        }
    }
    Ok(())
}
