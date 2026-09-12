//! ToolPass -> CSS loader (step 2 of #38): `staged.sqlite` -> Postgres.
//!
//! Reads the intermediate SQLite produced by the transform (step 1, built with
//! the ToolPass data outside this repo) and writes it into the CSS database.
//!
//! Properties:
//!   * **Idempotent / re-runnable** — every entity is keyed on a natural id
//!     (email, external_id, (user,tool), card code, ledger external_reference),
//!     so a second run is a no-op / upsert, never a duplicate.
//!   * **Transactional** — the whole load is one transaction; `--dry-run` does
//!     all the work and rolls back, printing the counts it would have committed.
//!   * **No Stripe** — plain Diesel inserts (no `StripeClient`), and it takes
//!     only a `--database-url`, never the app config, so it cannot read or use a
//!     Stripe key. Membership/Stripe columns on `users` are left NULL.
//!
//! Scope: users, tools, cards, tool-access grants (waivers), rate tiers +
//! assignments, and the full membership ledger. Usage sessions are a separate
//! pass (they need a `tool_usage_sessions` idempotency column).
//!
//! Usage: `toolpass-load --sqlite <staged.sqlite> --database-url <url> [--dry-run]`

use std::collections::HashMap;
use std::error::Error;
use std::str::FromStr;

use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use css_server::auth::PasswordHashUtil;
use css_server::database::MIGRATIONS;
use css_server::models::{
    CardStatus, LedgerEntryType, NewMembershipLedgerEntry, NewTool, NewToolRateTier,
    NewToolTierAssignment, NewTrainingWaiver, NewUser, NewUserCard, ToolCategory, ToolStatus,
    UserRole,
};
use css_server::schema::{
    membership_ledger, tool_rate_tiers, tool_tier_assignments, tools, training_waivers, user_cards,
    users,
};

const MIGRATION_EMAIL: &str = "toolpass-migration@invalid.local";
const MIGRATION_USERNAME: &str = "toolpass_migration";
const LEDGER_CHUNK: usize = 2000;

// ── in-memory staging rows (read from SQLite before touching Postgres) ────────

struct SUser {
    tp: String,
    full_name: String,
    email: String,
    username: String,
    role: UserRole,
}
struct STool {
    tp: String,
    name: String,
    description: Option<String>,
    external_id: Option<String>,
    category: ToolCategory,
    status: ToolStatus,
}
struct SCard {
    user_tp: String,
    code: String,
    status: CardStatus,
}
struct STier {
    tool_tp: String,
    name: String,
    rate_per_min: Option<BigDecimal>,
}
struct SAssign {
    user_tp: String,
    tool_tp: String,
    tier_name: String,
}
struct SWaiver {
    user_tp: String,
    tool_tp: String,
    reason: String,
    waived_at: Option<DateTime<Utc>>,
}
struct SLedger {
    user_tp: String,
    amount: BigDecimal,
    currency: String,
    entry_type: LedgerEntryType,
    occurred_at: DateTime<Utc>,
    description: Option<String>,
    ext_ref: String,
}

struct Staged {
    users: Vec<SUser>,
    tools: Vec<STool>,
    cards: Vec<SCard>,
    tiers: Vec<STier>,
    assignments: Vec<SAssign>,
    waivers: Vec<SWaiver>,
    ledger: Vec<SLedger>,
}

fn role_of(s: &str) -> UserRole {
    match s {
        "admin" => UserRole::Admin,
        "staff" => UserRole::Staff,
        "member" => UserRole::Member,
        "newbie" => UserRole::Newbie,
        _ => UserRole::Unknown,
    }
}
fn card_status_of(s: &str) -> CardStatus {
    match s {
        "active" => CardStatus::Active,
        "released" => CardStatus::Released,
        _ => CardStatus::Disabled,
    }
}
fn category_of(s: &str) -> ToolCategory {
    match s {
        "saw" => ToolCategory::Saw,
        "powertool" => ToolCategory::PowerTool,
        "hand_tools" => ToolCategory::HandTools,
        "measuring" => ToolCategory::Measuring,
        "safety" => ToolCategory::Safety,
        "electronics" => ToolCategory::Electronics,
        "woodworking" => ToolCategory::Woodworking,
        "metalworking" => ToolCategory::Metalworking,
        "3d_printing" => ToolCategory::ThreeDPrinting,
        "laser_cutting" => ToolCategory::LaserCutting,
        _ => ToolCategory::Other,
    }
}
fn status_of(s: &str) -> ToolStatus {
    match s {
        "in_use" => ToolStatus::InUse,
        "maintenance" => ToolStatus::Maintenance,
        "broken" => ToolStatus::Broken,
        "repair" => ToolStatus::Repair,
        "retired" => ToolStatus::Retired,
        _ => ToolStatus::Idle,
    }
}
fn entry_type_of(s: &str) -> LedgerEntryType {
    match s {
        "cash_payment" => LedgerEntryType::CashPayment,
        "adjustment" => LedgerEntryType::Adjustment,
        _ => LedgerEntryType::ToolUsage,
    }
}
fn ts(s: &str) -> Result<DateTime<Utc>, Box<dyn Error>> {
    Ok(DateTime::parse_from_rfc3339(s)?.with_timezone(&Utc))
}
fn bd(s: &str) -> Result<BigDecimal, Box<dyn Error>> {
    Ok(BigDecimal::from_str(s)?)
}

fn read_staged(path: &str) -> Result<Staged, Box<dyn Error>> {
    let sq =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;

    let mut users = Vec::new();
    let mut st = sq.prepare("SELECT tp_id,full_name,email,username,role FROM stg_users")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        users.push(SUser {
            tp: r.get(0)?,
            full_name: r.get(1)?,
            email: r.get(2)?,
            username: r.get(3)?,
            role: role_of(&r.get::<_, String>(4)?),
        });
    }

    let mut tools = Vec::new();
    let mut st =
        sq.prepare("SELECT tp_id,name,description,external_id,category,status FROM stg_tools")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        tools.push(STool {
            tp: r.get(0)?,
            name: r.get(1)?,
            description: r.get(2)?,
            external_id: r.get(3)?,
            category: category_of(&r.get::<_, String>(4)?),
            status: status_of(&r.get::<_, String>(5)?),
        });
    }

    let mut cards = Vec::new();
    let mut st = sq.prepare("SELECT user_tp_id,code,status FROM stg_cards")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        cards.push(SCard {
            user_tp: r.get(0)?,
            code: r.get(1)?,
            status: card_status_of(&r.get::<_, String>(2)?),
        });
    }

    let mut tiers = Vec::new();
    let mut st = sq.prepare("SELECT tool_tp_id,name,rate_per_min FROM stg_tool_tiers")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        let rate: Option<String> = r.get(2)?;
        tiers.push(STier {
            tool_tp: r.get(0)?,
            name: r.get(1)?,
            rate_per_min: match rate {
                Some(s) => Some(bd(&s)?),
                None => None,
            },
        });
    }

    let mut assignments = Vec::new();
    let mut st = sq.prepare("SELECT user_tp_id,tool_tp_id,tier_name FROM stg_tier_assignments")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        assignments.push(SAssign {
            user_tp: r.get(0)?,
            tool_tp: r.get(1)?,
            tier_name: r.get(2)?,
        });
    }

    let mut waivers = Vec::new();
    let mut st = sq.prepare("SELECT user_tp_id,tool_tp_id,reason,waived_at FROM stg_waivers")?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        let w: Option<String> = r.get(3)?;
        waivers.push(SWaiver {
            user_tp: r.get(0)?,
            tool_tp: r.get(1)?,
            reason: r.get(2)?,
            waived_at: match w {
                Some(s) => Some(ts(&s)?),
                None => None,
            },
        });
    }

    let mut ledger = Vec::new();
    let mut st = sq.prepare(
        "SELECT user_tp_id,amount,currency,entry_type,occurred_at,description,ext_ref FROM stg_ledger",
    )?;
    let mut rows = st.query([])?;
    while let Some(r) = rows.next()? {
        ledger.push(SLedger {
            user_tp: r.get(0)?,
            amount: bd(&r.get::<_, String>(1)?)?,
            currency: r.get(2)?,
            entry_type: entry_type_of(&r.get::<_, String>(3)?),
            occurred_at: ts(&r.get::<_, String>(4)?)?,
            description: r.get(5)?,
            ext_ref: r.get(6)?,
        });
    }

    Ok(Staged {
        users,
        tools,
        cards,
        tiers,
        assignments,
        waivers,
        ledger,
    })
}

#[derive(Default, Debug)]
struct Counts {
    users: usize,
    tools: usize,
    cards: usize,
    tiers: usize,
    assignments: usize,
    waivers: usize,
    ledger: usize,
    ledger_members_skipped: usize,
}

/// All Postgres writes, in FK order. Diesel errors only (SQLite is already read).
fn load(conn: &mut PgConnection, s: &Staged) -> Result<Counts, diesel::result::Error> {
    let mut c = Counts::default();

    // Migration system user: owns migrated tools and is the actor on waivers /
    // tier assignments. A random password it can never be used with.
    let mig_hash = PasswordHashUtil::hash(&Uuid::new_v4().to_string())
        .map_err(|e| diesel::result::Error::QueryBuilderError(Box::new(e)))?;
    diesel::insert_into(users::table)
        .values(&NewUser::with_role(
            MIGRATION_USERNAME.to_string(),
            MIGRATION_EMAIL.to_string(),
            mig_hash,
            "ToolPass Migration".to_string(),
            UserRole::Admin,
        ))
        .on_conflict(users::email)
        .do_nothing()
        .execute(conn)?;
    let mig_id: Uuid = users::table
        .filter(users::email.eq(MIGRATION_EMAIL))
        .select(users::id)
        .first(conn)?;

    // Users — insert-if-absent by email, then resolve id (rebuilds the map on a
    // re-run too). Random password; members set a real one via the reset flow.
    let mut user_id: HashMap<String, Uuid> = HashMap::new();
    for u in &s.users {
        let hash = PasswordHashUtil::hash(&Uuid::new_v4().to_string())
            .map_err(|e| diesel::result::Error::QueryBuilderError(Box::new(e)))?;
        let inserted = diesel::insert_into(users::table)
            .values(&NewUser::with_role(
                u.username.clone(),
                u.email.clone(),
                hash,
                u.full_name.clone(),
                u.role.clone(),
            ))
            .on_conflict(users::email)
            .do_nothing()
            .execute(conn)?;
        c.users += inserted;
        let id: Uuid = users::table
            .filter(users::email.eq(&u.email))
            .select(users::id)
            .first(conn)?;
        user_id.insert(u.tp.clone(), id);
    }

    // Tool default rate = the "Default Rate" tier's rate, if the tool has one.
    let mut tool_default_rate: HashMap<String, Option<BigDecimal>> = HashMap::new();
    for t in &s.tiers {
        if t.name == "Default Rate" {
            tool_default_rate.insert(t.tool_tp.clone(), t.rate_per_min.clone());
        }
    }

    // Tools — find-or-insert by external_id (else name); migration user owns them.
    let mut tool_id: HashMap<String, Uuid> = HashMap::new();
    for t in &s.tools {
        let existing: Option<Uuid> = match &t.external_id {
            Some(x) => tools::table
                .filter(tools::external_id.eq(x))
                .select(tools::id)
                .first(conn)
                .optional()?,
            None => tools::table
                .filter(tools::name.eq(&t.name))
                .select(tools::id)
                .first(conn)
                .optional()?,
        };
        let id = match existing {
            Some(id) => id,
            None => {
                let new_tool = NewTool {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    category: t.category.clone(),
                    status: Some(t.status.clone()),
                    barcode: None,
                    serial_number: None,
                    location: None,
                    purchase_date: None,
                    purchase_price: None,
                    maintenance_notes: None,
                    requires_training: Some(true),
                    created_by: mig_id,
                    external_id: t.external_id.clone(),
                    external_api_key: None,
                    place_id: None,
                    schedule_id: None,
                    usage_flat_fee: None,
                    usage_rate_per_min: tool_default_rate.get(&t.tp).cloned().flatten(),
                    usage_max_session_minutes: None,
                };
                let id: Uuid = diesel::insert_into(tools::table)
                    .values(&new_tool)
                    .returning(tools::id)
                    .get_result(conn)?;
                c.tools += 1;
                id
            }
        };
        tool_id.insert(t.tp.clone(), id);
    }

    // Rate tiers — find-or-insert by (tool_id, name).
    let mut tier_id: HashMap<(String, String), Uuid> = HashMap::new();
    for t in &s.tiers {
        let Some(&tid) = tool_id.get(&t.tool_tp) else {
            continue;
        };
        let existing: Option<Uuid> = tool_rate_tiers::table
            .filter(tool_rate_tiers::tool_id.eq(tid))
            .filter(tool_rate_tiers::name.eq(&t.name))
            .select(tool_rate_tiers::id)
            .first(conn)
            .optional()?;
        let id = match existing {
            Some(id) => id,
            None => {
                let id: Uuid = diesel::insert_into(tool_rate_tiers::table)
                    .values(&NewToolRateTier {
                        tool_id: tid,
                        name: t.name.clone(),
                        flat_fee: None,
                        rate_per_min: t.rate_per_min.clone(),
                        max_session_minutes: None,
                    })
                    .returning(tool_rate_tiers::id)
                    .get_result(conn)?;
                c.tiers += 1;
                id
            }
        };
        tier_id.insert((t.tool_tp.clone(), t.name.clone()), id);
    }

    // Waivers — the legacy access grants; upsert on (user, tool).
    for w in &s.waivers {
        let (Some(&uid), Some(&tid)) = (user_id.get(&w.user_tp), tool_id.get(&w.tool_tp)) else {
            continue;
        };
        diesel::insert_into(training_waivers::table)
            .values(&NewTrainingWaiver {
                user_id: uid,
                tool_id: tid,
                reason: w.reason.clone(),
                waived_by: Some(mig_id),
                waived_at: w.waived_at,
                expires_at: None,
            })
            .on_conflict((training_waivers::user_id, training_waivers::tool_id))
            .do_update()
            .set((
                training_waivers::reason.eq(&w.reason),
                training_waivers::waived_by.eq(Some(mig_id)),
            ))
            .execute(conn)?;
        c.waivers += 1;
    }

    // Tier assignments — upsert on (user, tool).
    for a in &s.assignments {
        let (Some(&uid), Some(&tid)) = (user_id.get(&a.user_tp), tool_id.get(&a.tool_tp)) else {
            continue;
        };
        let Some(&trid) = tier_id.get(&(a.tool_tp.clone(), a.tier_name.clone())) else {
            continue;
        };
        diesel::insert_into(tool_tier_assignments::table)
            .values(&NewToolTierAssignment {
                user_id: uid,
                tool_id: tid,
                tier_id: trid,
                assigned_by: Some(mig_id),
            })
            .on_conflict((
                tool_tier_assignments::user_id,
                tool_tier_assignments::tool_id,
            ))
            .do_update()
            .set((
                tool_tier_assignments::tier_id.eq(trid),
                tool_tier_assignments::assigned_by.eq(Some(mig_id)),
            ))
            .execute(conn)?;
        c.assignments += 1;
    }

    // Cards — insert-if-absent by (user, code). A code may recur across members
    // as a released reissue, so key on the pair, not the code alone.
    for card in &s.cards {
        let Some(&uid) = user_id.get(&card.user_tp) else {
            continue;
        };
        let exists: Option<Uuid> = user_cards::table
            .filter(user_cards::user_id.eq(uid))
            .filter(user_cards::code.eq(&card.code))
            .select(user_cards::id)
            .first(conn)
            .optional()?;
        if exists.is_none() {
            diesel::insert_into(user_cards::table)
                .values(&NewUserCard {
                    user_id: uid,
                    code: card.code.clone(),
                    status: Some(card.status.clone()),
                })
                .execute(conn)?;
            c.cards += 1;
        }
    }

    // Ledger — coarse idempotency per member (skip a member whose migrated
    // entries are already present), then batch-insert their entries.
    let mut by_member: HashMap<&str, Vec<&SLedger>> = HashMap::new();
    for e in &s.ledger {
        by_member.entry(&e.user_tp).or_default().push(e);
    }
    for (tp, entries) in &by_member {
        let Some(&uid) = user_id.get(*tp) else {
            continue;
        };
        let already: i64 = membership_ledger::table
            .filter(membership_ledger::external_reference.like(format!("toolpass:txn:{}:%", tp)))
            .count()
            .get_result(conn)?;
        if already > 0 {
            c.ledger_members_skipped += 1;
            continue;
        }
        let batch: Vec<NewMembershipLedgerEntry> = entries
            .iter()
            .map(|e| NewMembershipLedgerEntry {
                user_id: uid,
                entry_type: e.entry_type.clone(),
                amount: e.amount.clone(),
                currency: e.currency.clone(),
                occurred_at: e.occurred_at,
                description: e.description.clone(),
                external_reference: Some(e.ext_ref.clone()),
                created_by: None,
            })
            .collect();
        for chunk in batch.chunks(LEDGER_CHUNK) {
            diesel::insert_into(membership_ledger::table)
                .values(chunk)
                .execute(conn)?;
            c.ledger += chunk.len();
        }
    }

    Ok(c)
}

fn arg(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_mapping_covers_the_toolpass_types_and_defaults_safe() {
        assert_eq!(role_of("member"), UserRole::Member);
        assert_eq!(role_of("admin"), UserRole::Admin);
        assert_eq!(role_of("staff"), UserRole::Staff);
        // An unrecognised role must NOT silently become a privileged one.
        assert_eq!(role_of("something-else"), UserRole::Unknown);
    }

    #[test]
    fn card_status_defaults_to_disabled_not_active() {
        assert_eq!(card_status_of("active"), CardStatus::Active);
        assert_eq!(card_status_of("released"), CardStatus::Released);
        // Deny-biased: anything unexpected is disabled, never active.
        assert_eq!(card_status_of("weird"), CardStatus::Disabled);
    }

    #[test]
    fn tool_category_and_status_map_the_known_values() {
        assert_eq!(category_of("laser_cutting"), ToolCategory::LaserCutting);
        assert_eq!(category_of("saw"), ToolCategory::Saw);
        assert_eq!(category_of("powertool"), ToolCategory::PowerTool);
        assert_eq!(category_of("mystery"), ToolCategory::Other);
        assert_eq!(status_of("maintenance"), ToolStatus::Maintenance);
        assert_eq!(status_of("anything"), ToolStatus::Idle);
    }

    #[test]
    fn ledger_type_never_maps_to_stripe() {
        assert_eq!(entry_type_of("cash_payment").as_str(), "cash_payment");
        assert_eq!(entry_type_of("adjustment").as_str(), "adjustment");
        assert_eq!(entry_type_of("tool_usage").as_str(), "tool_usage");
        // The default is a non-Stripe type; the loader must never mint a
        // stripe_payment/refund from migrated data.
        let s = entry_type_of("whatever").as_str();
        assert!(!s.starts_with("stripe"), "got {s}");
    }

    #[test]
    fn money_and_timestamp_parse() {
        assert_eq!(
            bd("-49.17").unwrap(),
            BigDecimal::from_str("-49.17").unwrap()
        );
        assert_eq!(bd("0").unwrap(), BigDecimal::from_str("0").unwrap());
        let t = ts("2021-12-12T22:00:00Z").unwrap();
        assert_eq!(t.to_rfc3339(), "2021-12-12T22:00:00+00:00");
        assert!(ts("not-a-date").is_err());
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let sqlite = arg("--sqlite").ok_or("--sqlite <path> is required")?;
    let db_url = arg("--database-url")
        .or_else(|| std::env::var("CSS_LOAD_DATABASE_URL").ok())
        .ok_or("--database-url <url> (or CSS_LOAD_DATABASE_URL) is required")?;
    let dry_run = std::env::args().any(|a| a == "--dry-run");

    eprintln!("reading staged SQLite: {sqlite}");
    let staged = read_staged(&sqlite)?;
    eprintln!(
        "staged: {} users, {} tools, {} cards, {} tiers, {} assignments, {} waivers, {} ledger",
        staged.users.len(),
        staged.tools.len(),
        staged.cards.len(),
        staged.tiers.len(),
        staged.assignments.len(),
        staged.waivers.len(),
        staged.ledger.len(),
    );
    let admins = staged
        .users
        .iter()
        .filter(|u| u.role == UserRole::Admin)
        .count();
    eprintln!("note: {admins} member(s) map to role=admin (ToolPass Group Admins)");

    let mut conn = PgConnection::establish(&db_url)?;
    conn.run_pending_migrations(MIGRATIONS)
        .map_err(|e| format!("migrations failed: {e}"))?;

    // One transaction; --dry-run rolls back via a sentinel error after counting.
    let outcome = conn.transaction::<Counts, diesel::result::Error, _>(|conn| {
        let counts = load(conn, &staged)?;
        println!(
            "{}counts: {:?}",
            if dry_run { "[dry-run] " } else { "" },
            counts
        );
        if dry_run {
            return Err(diesel::result::Error::RollbackTransaction);
        }
        Ok(counts)
    });

    match outcome {
        Ok(_) => {
            println!("committed.");
            Ok(())
        }
        Err(diesel::result::Error::RollbackTransaction) => {
            println!("dry-run: rolled back, nothing written.");
            Ok(())
        }
        Err(e) => Err(Box::new(e)),
    }
}
