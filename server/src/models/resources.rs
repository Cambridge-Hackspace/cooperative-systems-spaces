use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::schema::resources;

// ---------------------------------------------------------------------------
// resources -- the supertype of every access-controlled thing (#101).
//
// A door and a tool are the same kind of thing: a resource access is controlled
// over. This is the class-table-inheritance parent -- each door/tool row shares
// its id with a `resources` row (the subtype tables carry a shared-PK foreign
// key), so a `resource_id` and a `tool_id`/`door_id` are the same uuid. Slice 1a
// carries only identity + the `kind` discriminator + audit timestamps; the
// common columns (name/description/location) are lifted here in slice 1b.
// ---------------------------------------------------------------------------

/// The two subtypes a resource can be. Stored as `resources.kind` text, matching
/// the SQL `CHECK (kind IN ('door','tool'))`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Door,
    Tool,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Door => "door",
            ResourceKind::Tool => "tool",
        }
    }

    /// The session profile a resource of this kind gets unless an administrator
    /// says otherwise (#182): a door's session is the pulse, a tool's lasts
    /// until it is stopped. A default by kind, not a rule -- a door that holds
    /// a room's power until a stop is `until_stopped`, and nothing about the
    /// kind forbids it.
    pub fn default_session_profile(self) -> &'static str {
        match self {
            ResourceKind::Door => session_profile::MOMENTARY,
            ResourceKind::Tool => session_profile::UNTIL_STOPPED,
        }
    }
}

/// How a session on a resource ends (#182). Stored as `resources.session_profile`
/// text, matching the SQL `CHECK`; `checks/tests/text_check_vocabularies_match.rs`
/// holds the two to each other.
///
/// `momentary`: a yes is the strike pulse and the session is over when the pulse
/// is -- the door as we know it. `until_stopped`: the session lasts until a stop
/// (a badge, a stop button, a sensor, the sweep) -- the tool as we know it, and
/// the room: a door whose power bindings stay leased until somebody stops it.
pub mod session_profile {
    pub const MOMENTARY: &str = "momentary";
    pub const UNTIL_STOPPED: &str = "until_stopped";
    pub const ALL: [&str; 2] = [MOMENTARY, UNTIL_STOPPED];
}

/// Who ended a resource session (#190). Held to the CHECK on
/// `resource_sessions.closed_by` by `checks/tests/text_check_vocabularies_match.rs`.
pub mod session_closed_by {
    /// A device: the stop button, the badge, the sensor -- a `/stop`.
    pub const STOP: &str = "stop";
    /// The session sweep: abandoned past its cap.
    pub const SWEEP: &str = "sweep";
    /// An administrator, from Resource Wiring.
    pub const ADMIN: &str = "admin";
    pub const ALL: [&str; 3] = [STOP, SWEEP, ADMIN];
}

/// An access session on a resource of any kind (#190): what a tool has always
/// had as `tools.status = in_use`, now a record a door can have too. One open
/// per resource; `closed_at` is `None` while it is.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = crate::schema::resource_sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ResourceSession {
    pub id: Uuid,
    pub resource_id: Uuid,
    pub user_id: Option<Uuid>,
    pub device_id: Option<Uuid>,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub closed_by: Option<String>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::resource_sessions)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewResourceSession {
    pub resource_id: Uuid,
    pub user_id: Option<Uuid>,
    pub device_id: Option<Uuid>,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable, Serialize)]
#[diesel(table_name = resources)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct Resource {
    pub id: Uuid,
    /// `"door"` or `"tool"`; see [`ResourceKind`].
    pub kind: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// `momentary` | `until_stopped`; see [`session_profile`]. Appended last:
    /// positional Queryable.
    pub session_profile: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = resources)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewResource {
    /// The id is shared with the subtype row that is inserted alongside it, so it
    /// is supplied rather than defaulted.
    pub id: Uuid,
    pub kind: String,
    pub session_profile: String,
}

impl NewResource {
    /// A resource of `kind` with the session profile its kind defaults to.
    pub fn of_kind(id: Uuid, kind: ResourceKind) -> Self {
        Self {
            id,
            kind: kind.as_str().to_string(),
            session_profile: kind.default_session_profile().to_string(),
        }
    }
}
