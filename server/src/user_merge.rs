//! Merging one user into another (#118).
//!
//! Two records, one person. The survivor keeps its identity (username,
//! password, profile); everything that *referenced* the absorbed account is
//! re-pointed to the survivor; the absorbed row is then deleted and a
//! `user_merges` row keeps what it was. Nothing is chosen silently: anything
//! that cannot be merged mechanically becomes a [`MergeWarning`] the
//! administrator must acknowledge by code before the commit runs, and the
//! commit recomputes the plan inside its transaction and refuses if the set of
//! warnings has changed since the preview.
//!
//! Why re-point everything rather than delete-and-cascade: `audit_logs.actor_id`
//! and `audit_logs.user_id` are `ON DELETE SET NULL`, so a plain delete would
//! anonymise everything the person ever did; seven other references are
//! RESTRICT and would refuse the delete outright; and the six `(user_id, X)`
//! unique pairs would collide. Each of those shapes is handled by name below,
//! and `checks/tests/user_merge_covers_every_user_reference.rs` holds this
//! file's table lists to the migrations: a new foreign key to `users` that this
//! file does not know about fails the build.

use std::collections::{BTreeMap, BTreeSet};

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Jsonb, Text, Uuid as SqlUuid};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::{DatabaseError, DatabaseManager};
use crate::models::{NewUserMerge, User};
use crate::rbac::RoleGraph;

/// `(table, column)` pairs re-pointed with a plain `UPDATE ... SET col =
/// survivor WHERE col = absorbed`. Every FK to `users` that carries no
/// uniqueness over the user, whatever its ON DELETE clause.
pub const PLAIN: &[(&str, &str)] = &[
    ("alert_acknowledgements", "user_id"),
    ("audit_logs", "user_id"),
    ("audit_logs", "actor_id"),
    ("cmi5_courses", "imported_by"),
    ("cmi5_registrations", "user_id"),
    ("door_access_events", "user_id"),
    ("door_checkins", "user_id"),
    ("doors", "created_by"),
    ("home_links", "created_by"),
    ("membership_ledger", "user_id"),
    ("membership_ledger", "created_by"),
    ("power_circuits", "locked_out_by"),
    ("profile_config_versions", "created_by"),
    ("schedules", "created_by"),
    ("space_device_auth_requests", "created_by"),
    ("tool_events", "user_id"),
    ("tool_events", "actor_id"),
    ("tool_tier_assignments", "assigned_by"),
    ("tool_training_types", "created_by"),
    ("tool_usage_sessions", "user_id"),
    ("tools", "created_by"),
    ("training_instructors", "certified_by"),
    ("training_steps", "created_by"),
    ("training_waivers", "waived_by"),
    ("user_cards", "user_id"),
    ("user_merges", "survivor_id"),
    ("user_merges", "actor_id"),
    ("user_mfa_recovery_codes", "user_id"),
    ("user_mfa_totp", "user_id"),
    ("user_mfa_webauthn", "user_id"),
    ("user_stripe_customers", "user_id"),
    ("user_tool_training", "trainer_id"),
    ("user_training_progress", "instructor_id"),
    ("webhook_auth_headers", "created_by"),
    ("webhooks", "created_by"),
];

/// `(table, key column)` pairs holding `UNIQUE (user_id, key)`. Rows whose
/// key the survivor does not already hold are moved; rows that collide keep
/// the SURVIVOR's and drop the absorbed one -- and the collision is a warning,
/// never a silent choice.
pub const UNIQUE_PAIRS: &[(&str, &str)] = &[
    ("tool_tier_assignments", "tool_id"),
    ("training_instructors", "training_step_id"),
    ("training_waivers", "tool_id"),
    ("user_tool_training", "training_type_id"),
    ("user_training_progress", "training_step_id"),
];

/// References to `users` handled by hand in [`execute`], with the reason.
pub const SPECIAL: &[(&str, &str, &str)] = &[
    (
        "user_roles",
        "user_id",
        "set union on the (user_id, role_id) primary key",
    ),
    (
        "user_emails",
        "user_id",
        "moved as secondaries; the absorbed primary is demoted",
    ),
];

/// References to `users` that are deliberately NOT moved: the rows die with
/// the absorbed account (ON DELETE CASCADE) because they are credentials in
/// flight for an identity that is ending.
pub const DROPPED: &[(&str, &str, &str)] = &[
    (
        "password_reset_tokens",
        "user_id",
        "a live reset link for the absorbed login must not reset the survivor",
    ),
    (
        "email_verification_tokens",
        "user_id",
        "re-issued against the moved address when the member asks",
    ),
];

/// References by UUID-as-text, with no foreign key, rewritten by value.
pub const TEXT_REFS: &[(&str, &str)] = &[
    ("cmi5_registrations", "actor_account_name"),
    ("cmi5_state_documents", "agent_account_name"),
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MergeWarning {
    /// Stable code the administrator acknowledges by.
    pub code: String,
    pub detail: String,
}

impl MergeWarning {
    fn new(code: &str, detail: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MergeParty {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub full_name: String,
}

impl From<&User> for MergeParty {
    fn from(u: &User) -> Self {
        Self {
            id: u.id,
            username: u.username.clone(),
            email: u.email.clone(),
            full_name: u.full_name.clone(),
        }
    }
}

/// What a merge would do. The preview returns this; the commit recomputes it.
#[derive(Debug, Clone, Serialize)]
pub struct MergePlan {
    pub survivor: MergeParty,
    pub absorbed: MergeParty,
    /// Rows that would be re-pointed, keyed `table.column`.
    pub moves: BTreeMap<String, i64>,
    pub warnings: Vec<MergeWarning>,
}

impl MergePlan {
    pub fn warning_codes(&self) -> BTreeSet<String> {
        self.warnings.iter().map(|w| w.code.clone()).collect()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MergeOutcome {
    pub merge_id: Uuid,
    pub survivor_id: Uuid,
    pub absorbed_id: Uuid,
    pub moved: BTreeMap<String, i64>,
    pub warnings: Vec<MergeWarning>,
}

#[derive(Debug)]
pub enum MergeError {
    /// The acknowledged set does not equal the plan's warning set. Carries
    /// the plan as it stands now, so the caller can show what must be
    /// acknowledged.
    Unacknowledged(MergePlan),
    Db(diesel::result::Error),
}

impl From<diesel::result::Error> for MergeError {
    fn from(e: diesel::result::Error) -> Self {
        MergeError::Db(e)
    }
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct TextRow {
    #[diesel(sql_type = Text)]
    v: String,
}

fn count(conn: &mut PgConnection, sql: &str, a: Uuid) -> QueryResult<i64> {
    let row: CountRow = diesel::sql_query(sql)
        .bind::<SqlUuid, _>(a)
        .get_result(conn)?;
    Ok(row.n)
}

fn count2(conn: &mut PgConnection, sql: &str, a: Uuid, b: Uuid) -> QueryResult<i64> {
    let row: CountRow = diesel::sql_query(sql)
        .bind::<SqlUuid, _>(a)
        .bind::<SqlUuid, _>(b)
        .get_result(conn)?;
    Ok(row.n)
}

/// Identifiers here come only from the constant tables above, never from a
/// request, so interpolating them is the same as writing the query by hand.
fn plain_count_sql(table: &str, col: &str) -> String {
    format!("SELECT count(*) AS n FROM {table} WHERE {col} = $1")
}

fn plain_update_sql(table: &str, col: &str) -> String {
    format!("UPDATE {table} SET {col} = $1 WHERE {col} = $2")
}

/// Rows of the absorbed user whose key the survivor does NOT already hold.
fn pair_movable_sql(table: &str, key: &str) -> String {
    format!(
        "SELECT count(*) AS n FROM {table} a WHERE a.user_id = $2 \
         AND NOT EXISTS (SELECT 1 FROM {table} s WHERE s.user_id = $1 AND s.{key} = a.{key})"
    )
}

fn pair_colliding_sql(table: &str, key: &str) -> String {
    format!(
        "SELECT count(*) AS n FROM {table} a WHERE a.user_id = $2 \
         AND EXISTS (SELECT 1 FROM {table} s WHERE s.user_id = $1 AND s.{key} = a.{key})"
    )
}

fn pair_move_sql(table: &str, key: &str) -> String {
    format!(
        "UPDATE {table} a SET user_id = $1 WHERE a.user_id = $2 \
         AND NOT EXISTS (SELECT 1 FROM {table} s WHERE s.user_id = $1 AND s.{key} = a.{key})"
    )
}

fn pair_drop_sql(table: &str) -> String {
    format!("DELETE FROM {table} WHERE user_id = $1")
}

/// Compute the plan: counts of what moves, and every warning.
pub fn plan(
    conn: &mut PgConnection,
    rbac: &RoleGraph,
    survivor: &User,
    absorbed: &User,
) -> QueryResult<MergePlan> {
    let s = survivor.id;
    let a = absorbed.id;
    let mut moves = BTreeMap::new();
    let mut warnings = Vec::new();

    for (table, col) in PLAIN {
        let n = count(conn, &plain_count_sql(table, col), a)?;
        if n > 0 {
            moves.insert(format!("{table}.{col}"), n);
        }
    }
    for (table, key) in UNIQUE_PAIRS {
        let movable = count2(conn, &pair_movable_sql(table, key), s, a)?;
        if movable > 0 {
            moves.insert(format!("{table}.user_id"), movable);
        }
        let colliding = count2(conn, &pair_colliding_sql(table, key), s, a)?;
        if colliding > 0 {
            warnings.push(MergeWarning::new(
                &format!("duplicate_{table}"),
                format!(
                    "{colliding} row(s) in {table} exist for the same {key} on both accounts; \
                     the survivor's are kept and the absorbed account's are discarded"
                ),
            ));
        }
    }
    for (table, col) in TEXT_REFS {
        let n = count(
            conn,
            &format!("SELECT count(*) AS n FROM {table} WHERE {col} = $1::text"),
            a,
        )?;
        if n > 0 {
            moves.insert(format!("{table}.{col}"), n);
        }
    }

    // Roles: union. Warn when the union raises the survivor's level -- that
    // is an access grant, made by merging rather than by assigning.
    let survivor_roles = crate::rbac::roles_for_user(conn, s)?;
    let absorbed_roles = crate::rbac::roles_for_user(conn, a)?;
    let new_roles: Vec<Uuid> = absorbed_roles
        .iter()
        .filter(|r| !survivor_roles.contains(r))
        .copied()
        .collect();
    if !new_roles.is_empty() {
        moves.insert("user_roles.user_id".to_string(), new_roles.len() as i64);
    }
    let before = rbac.effective_level(&survivor_roles);
    let union: Vec<Uuid> = survivor_roles
        .iter()
        .chain(absorbed_roles.iter())
        .copied()
        .collect();
    let after = rbac.effective_level(&union);
    if after > before {
        warnings.push(MergeWarning::new(
            "roles_raise_level",
            format!(
                "the absorbed account's roles raise the survivor's effective level from {before} to {after}"
            ),
        ));
    }

    // Addresses: every one moves; the absorbed primary becomes a secondary.
    let emails = count(
        conn,
        "SELECT count(*) AS n FROM user_emails WHERE user_id = $1",
        a,
    )?;
    if emails > 0 {
        moves.insert("user_emails.user_id".to_string(), emails);
    }

    // Access rules by user id: rewritten; an allow on one and a deny on the
    // other for the same resource is a decision, not a rewrite.
    let rules = count(
        conn,
        "SELECT count(*) AS n FROM access_rules WHERE kind = 'user' AND value = $1::text",
        a,
    )?;
    if rules > 0 {
        moves.insert("access_rules.value".to_string(), rules);
    }
    let conflicting = count2(
        conn,
        "SELECT count(*) AS n FROM access_rules a \
         JOIN access_rules s ON s.resource_id = a.resource_id AND s.kind = 'user' AND s.value = $1::text \
         WHERE a.kind = 'user' AND a.value = $2::text AND a.effect <> s.effect",
        s,
        a,
    )?;
    if conflicting > 0 {
        warnings.push(MergeWarning::new(
            "access_rule_conflict",
            format!(
                "{conflicting} resource(s) allow one account and deny the other; both rules are kept and deny wins"
            ),
        ));
    }

    // Always-true losses, stated rather than assumed.
    warnings.push(MergeWarning::new(
        "absorbed_login_lost",
        format!(
            "the username {:?} and its password stop working; the survivor's credentials remain",
            absorbed.username
        ),
    ));
    if survivor.full_name != absorbed.full_name {
        warnings.push(MergeWarning::new(
            "full_name_differs",
            format!(
                "names differ ({:?} vs {:?}); the survivor's is kept",
                survivor.full_name, absorbed.full_name
            ),
        ));
    }
    if !absorbed.is_active && survivor.is_active {
        warnings.push(MergeWarning::new(
            "absorbed_inactive",
            "the absorbed account is deactivated; its history joins an active account",
        ));
    }
    if absorbed.is_active && !survivor.is_active {
        warnings.push(MergeWarning::new(
            "survivor_inactive",
            "the survivor is deactivated; the active account's history joins an account that cannot sign in",
        ));
    }

    // Profile: survivor's keys win; absorbed-only keys are added.
    let conflicts = profile_conflicts(&survivor.profile, &absorbed.profile);
    if !conflicts.is_empty() {
        warnings.push(MergeWarning::new(
            "profile_conflicts",
            format!(
                "profile fields differ and the survivor's are kept: {}",
                conflicts.join(", ")
            ),
        ));
    }
    if survivor.mailing_list_opt_out_at.is_some() != absorbed.mailing_list_opt_out_at.is_some() {
        warnings.push(MergeWarning::new(
            "opt_out_mismatch",
            "one account opted out of the mailing list and the other did not; the survivor's choice is kept",
        ));
    }
    match (survivor.membership_next_due_at, absorbed.membership_next_due_at) {
        (Some(sd), Some(ad)) if sd != ad => warnings.push(MergeWarning::new(
            "next_due_mismatch",
            format!("both accounts are enrolled with different renewal dates ({sd} vs {ad}); the survivor's is kept"),
        )),
        (None, Some(ad)) => warnings.push(MergeWarning::new(
            "next_due_adopted",
            format!("only the absorbed account is enrolled in dues; its renewal date ({ad}) is adopted"),
        )),
        _ => {}
    }

    // Things that need a query.
    let last_login_sql = "SELECT coalesce(max(created_at)::text, '') AS v FROM audit_logs \
                          WHERE event_type = 'user_login' AND user_id = $1";
    let s_login: TextRow = diesel::sql_query(last_login_sql)
        .bind::<SqlUuid, _>(s)
        .get_result(conn)?;
    let a_login: TextRow = diesel::sql_query(last_login_sql)
        .bind::<SqlUuid, _>(a)
        .get_result(conn)?;
    if !a_login.v.is_empty() && a_login.v > s_login.v {
        warnings.push(MergeWarning::new(
            "absorbed_logged_in_more_recently",
            format!(
                "the absorbed account signed in more recently ({}) than the survivor ({}); check the survivor is the right way round",
                a_login.v,
                if s_login.v.is_empty() { "never" } else { &s_login.v }
            ),
        ));
    }
    let open_sql =
        "SELECT count(*) AS n FROM tool_usage_sessions WHERE status = 'open' AND user_id = $1";
    if count(conn, open_sql, s)? > 0 && count(conn, open_sql, a)? > 0 {
        warnings.push(MergeWarning::new(
            "open_sessions_both",
            "both accounts have a tool session open right now; both move to the survivor",
        ));
    }
    let tiers = count2(
        conn,
        "SELECT count(*) AS n FROM tool_tier_assignments a \
         JOIN tool_tier_assignments s ON s.user_id = $1 AND s.tool_id = a.tool_id \
         WHERE a.user_id = $2 AND a.tier_id <> s.tier_id",
        s,
        a,
    )?;
    if tiers > 0 {
        warnings.push(MergeWarning::new(
            "tier_differs",
            format!("{tiers} tool(s) have a different rate tier on each account; the survivor's tier is kept"),
        ));
    }
    if count(
        conn,
        "SELECT count(*) AS n FROM user_stripe_customers WHERE subscription_id IS NOT NULL AND user_id = $1",
        a,
    )? > 0
    {
        warnings.push(MergeWarning::new(
            "absorbed_active_subscription",
            "the absorbed account has a live Stripe subscription; it moves to the survivor, who may then be billed twice",
        ));
    }
    let codes_sql =
        "SELECT count(*) AS n FROM user_mfa_recovery_codes WHERE used_at IS NULL AND user_id = $1";
    if count(conn, codes_sql, s)? > 0 && count(conn, codes_sql, a)? > 0 {
        warnings.push(MergeWarning::new(
            "recovery_codes_union",
            "both accounts hold unused recovery codes; both sheets will open the survivor until regenerated",
        ));
    }

    Ok(MergePlan {
        survivor: survivor.into(),
        absorbed: absorbed.into(),
        moves,
        warnings,
    })
}

/// Keys present on both profiles with different values.
fn profile_conflicts(survivor: &serde_json::Value, absorbed: &serde_json::Value) -> Vec<String> {
    let (Some(s), Some(a)) = (survivor.as_object(), absorbed.as_object()) else {
        return Vec::new();
    };
    a.iter()
        .filter(|(k, v)| s.get(*k).is_some_and(|sv| sv != *v))
        .map(|(k, _)| k.clone())
        .collect()
}

/// The survivor's profile with the absorbed account's keys added where the
/// survivor had none.
fn merged_profile(survivor: &serde_json::Value, absorbed: &serde_json::Value) -> serde_json::Value {
    let mut out = survivor.clone();
    if let (Some(o), Some(a)) = (out.as_object_mut(), absorbed.as_object()) {
        for (k, v) in a {
            o.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    out
}

/// Run the merge. Recomputes the plan inside the transaction and refuses with
/// [`MergeError::Unacknowledged`] unless `acknowledged` equals its warning
/// codes exactly -- a warning that appeared since the preview (someone logged
/// in, a rule was added) is a warning the administrator has not seen.
pub fn execute(
    conn: &mut PgConnection,
    rbac: &RoleGraph,
    survivor: &User,
    absorbed: &User,
    acknowledged: &BTreeSet<String>,
    actor: Uuid,
) -> Result<MergeOutcome, MergeError> {
    conn.transaction::<MergeOutcome, MergeError, _>(|conn| {
        let plan = plan(conn, rbac, survivor, absorbed)?;
        if &plan.warning_codes() != acknowledged {
            return Err(MergeError::Unacknowledged(plan));
        }
        let s = survivor.id;
        let a = absorbed.id;
        let mut moved: BTreeMap<String, i64> = BTreeMap::new();

        // Roles first, so the union exists before anything consults it.
        let n = diesel::sql_query(
            "INSERT INTO user_roles (user_id, role_id) SELECT $1, role_id FROM user_roles \
             WHERE user_id = $2 ON CONFLICT DO NOTHING",
        )
        .bind::<SqlUuid, _>(s)
        .bind::<SqlUuid, _>(a)
        .execute(conn)?;
        if n > 0 {
            moved.insert("user_roles.user_id".to_string(), n as i64);
        }
        diesel::sql_query("DELETE FROM user_roles WHERE user_id = $1")
            .bind::<SqlUuid, _>(a)
            .execute(conn)?;

        for (table, key) in UNIQUE_PAIRS {
            let n = diesel::sql_query(pair_move_sql(table, key))
                .bind::<SqlUuid, _>(s)
                .bind::<SqlUuid, _>(a)
                .execute(conn)?;
            if n > 0 {
                moved.insert(format!("{table}.user_id"), n as i64);
            }
            diesel::sql_query(pair_drop_sql(table))
                .bind::<SqlUuid, _>(a)
                .execute(conn)?;
        }

        for (table, col) in PLAIN {
            let n = diesel::sql_query(plain_update_sql(table, col))
                .bind::<SqlUuid, _>(s)
                .bind::<SqlUuid, _>(a)
                .execute(conn)?;
            if n > 0 {
                moved.insert(format!("{table}.{col}"), n as i64);
            }
        }

        for (table, col) in TEXT_REFS {
            let n = diesel::sql_query(format!(
                "UPDATE {table} SET {col} = $1::text WHERE {col} = $2::text"
            ))
            .bind::<SqlUuid, _>(s)
            .bind::<SqlUuid, _>(a)
            .execute(conn)?;
            if n > 0 {
                moved.insert(format!("{table}.{col}"), n as i64);
            }
        }

        // Access rules: rewrite where the survivor has no identical rule;
        // drop the exact duplicates. A conflicting effect is kept on both
        // sides (deny wins in the engine), as the warning said.
        let n = diesel::sql_query(
            "UPDATE access_rules a SET value = $1::text WHERE a.kind = 'user' AND a.value = $2::text \
             AND NOT EXISTS (SELECT 1 FROM access_rules s WHERE s.resource_id = a.resource_id \
             AND s.kind = 'user' AND s.value = $1::text AND s.effect = a.effect)",
        )
        .bind::<SqlUuid, _>(s)
        .bind::<SqlUuid, _>(a)
        .execute(conn)?;
        if n > 0 {
            moved.insert("access_rules.value".to_string(), n as i64);
        }
        diesel::sql_query("DELETE FROM access_rules WHERE kind = 'user' AND value = $1::text")
            .bind::<SqlUuid, _>(a)
            .execute(conn)?;

        // Addresses: all become the survivor's secondaries. The absorbed
        // primary is demoted in the same statement, so the partial unique
        // index (one primary per user) never sees two.
        let n = diesel::sql_query(
            "UPDATE user_emails SET user_id = $1, is_primary = FALSE WHERE user_id = $2",
        )
        .bind::<SqlUuid, _>(s)
        .bind::<SqlUuid, _>(a)
        .execute(conn)?;
        if n > 0 {
            moved.insert("user_emails.user_id".to_string(), n as i64);
        }

        // The survivor's own row: profile union, adopted renewal date, and a
        // session-revocation bump (its roles may have changed).
        let profile = merged_profile(&survivor.profile, &absorbed.profile);
        diesel::sql_query(
            "UPDATE users SET profile = $1, \
             membership_next_due_at = coalesce(membership_next_due_at, $2), \
             token_version = token_version + 1, updated_at = now() WHERE id = $3",
        )
        .bind::<Jsonb, _>(profile)
        .bind::<diesel::sql_types::Nullable<diesel::sql_types::Timestamptz>, _>(
            absorbed.membership_next_due_at,
        )
        .bind::<SqlUuid, _>(s)
        .execute(conn)?;

        // The absorbed row goes. Everything that referenced it has been moved;
        // what is left to cascade is DROPPED above, by design.
        let deleted = diesel::sql_query("DELETE FROM users WHERE id = $1")
            .bind::<SqlUuid, _>(a)
            .execute(conn)?;
        if deleted != 1 {
            return Err(MergeError::Db(diesel::result::Error::NotFound));
        }

        let mut snapshot = serde_json::to_value(absorbed).unwrap_or(serde_json::Value::Null);
        if let Some(o) = snapshot.as_object_mut() {
            o.remove("password_hash");
        }
        let merge_id: Uuid = diesel::insert_into(crate::schema::user_merges::table)
            .values(NewUserMerge {
                survivor_id: Some(s),
                absorbed_id: a,
                absorbed_username: absorbed.username.clone(),
                absorbed_email: absorbed.email.clone(),
                absorbed_snapshot: snapshot,
                moved: serde_json::to_value(&moved).unwrap_or(serde_json::Value::Null),
                warnings: serde_json::to_value(&plan.warnings).unwrap_or(serde_json::Value::Null),
                actor_id: Some(actor),
            })
            .returning(crate::schema::user_merges::id)
            .get_result(conn)?;

        Ok(MergeOutcome {
            merge_id,
            survivor_id: s,
            absorbed_id: a,
            moved,
            warnings: plan.warnings,
        })
    })
}

impl DatabaseManager {
    pub fn preview_user_merge(
        &self,
        survivor: &User,
        absorbed: &User,
    ) -> Result<MergePlan, DatabaseError> {
        let mut conn = self.get_connection()?;
        plan(&mut conn, &self.rbac(), survivor, absorbed).map_err(DatabaseError::Diesel)
    }

    pub fn merge_users(
        &self,
        survivor: &User,
        absorbed: &User,
        acknowledged: &BTreeSet<String>,
        actor: Uuid,
    ) -> Result<Result<MergeOutcome, MergePlan>, DatabaseError> {
        let mut conn = self.get_connection()?;
        match execute(
            &mut conn,
            &self.rbac(),
            survivor,
            absorbed,
            acknowledged,
            actor,
        ) {
            Ok(outcome) => Ok(Ok(outcome)),
            Err(MergeError::Unacknowledged(plan)) => Ok(Err(plan)),
            Err(MergeError::Db(e)) => Err(DatabaseError::Diesel(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn survivor_profile_wins_and_absorbed_only_keys_are_added() {
        let s = serde_json::json!({"card_id": "S1", "phone": "1"});
        let a = serde_json::json!({"card_id": "A1", "bio": "hi"});
        assert_eq!(
            merged_profile(&s, &a),
            serde_json::json!({"card_id": "S1", "phone": "1", "bio": "hi"})
        );
        assert_eq!(profile_conflicts(&s, &a), vec!["card_id".to_string()]);
    }

    #[test]
    fn an_identical_value_is_not_a_conflict() {
        let s = serde_json::json!({"card_id": "X"});
        let a = serde_json::json!({"card_id": "X"});
        assert!(profile_conflicts(&s, &a).is_empty());
    }

    #[test]
    fn the_table_lists_do_not_overlap() {
        // A pair handled twice would be re-pointed twice -- harmless for a
        // plain update, wrong for a unique pair whose rows were just dropped.
        let mut seen = BTreeSet::new();
        for (t, c) in PLAIN {
            assert!(seen.insert((t, c)), "{t}.{c} listed twice in PLAIN");
        }
        for (t, _) in UNIQUE_PAIRS {
            assert!(
                seen.insert((t, &"user_id")),
                "{t}.user_id is both PLAIN and a UNIQUE_PAIR"
            );
        }
        for (t, c, _) in SPECIAL.iter().chain(DROPPED.iter()) {
            assert!(
                seen.insert((t, c)),
                "{t}.{c} is listed in more than one group"
            );
        }
    }
}
