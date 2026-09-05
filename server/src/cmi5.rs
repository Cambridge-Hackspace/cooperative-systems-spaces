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

use std::io::{Cursor, Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;
use zip::ZipArchive;

// Leading `::` names the external `cmi5` crate unambiguously, distinct from this
// module (`crate::cmi5`) and `crate::api::cmi5`.
use ::cmi5::{
    append_query, build_launch_query, categories, parse_manifest, verbs, Account, Activity, Agent,
    Context, ContextActivities, LangString, LaunchData, LaunchMode, LaunchParams, ManifestError,
    MoveOn, Node, Statement, StatementObject, Verb,
};

use crate::config::Cmi5Config;
use crate::database::DatabaseManager;
use crate::models::{
    AssignCmi5AuStep, Cmi5AssignableUnit, Cmi5Block, Cmi5Course, NewCmi5AssignableUnit,
    NewCmi5Block, NewCmi5Course, NewCmi5LaunchToken, NewCmi5Registration, NewCmi5StateDocument,
    NewCmi5Statement, TrainingStep,
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
