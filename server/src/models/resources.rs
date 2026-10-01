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
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = resources)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct NewResource {
    /// The id is shared with the subtype row that is inserted alongside it, so it
    /// is supplied rather than defaulted.
    pub id: Uuid,
    pub kind: String,
}
