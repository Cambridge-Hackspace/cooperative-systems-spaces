use anyhow::Result;
use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool, PoolError, PooledConnection};
use diesel::{Connection, ExpressionMethods, QueryDsl, RunQueryDsl, SelectableHelper};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use std::fmt;
use tracing::{debug, error, info, warn};

use crate::config::DatabaseConfig;
use crate::models::{
    CardResolution, CardStatus, NewTrainingWaiver, NewUser, NewUserCard, TrainingWaiver,
    UpdateUser, User, UserCard,
};
use crate::schema::{training_waivers, user_cards, users};

pub const MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations");

/// Type alias for our database connection pool
pub type DbPool = Pool<ConnectionManager<PgConnection>>;

/// Type alias for a pooled database connection
pub type DbConnection = PooledConnection<ConnectionManager<PgConnection>>;

/// Custom error type for database operations
#[derive(Debug)]
pub enum DatabaseError {
    /// Connection pool error
    Pool(PoolError),
    /// Diesel database error
    Diesel(diesel::result::Error),
    /// Migration error
    Migration(Box<dyn std::error::Error + Send + Sync>),
    /// Connection timeout
    ConnectionTimeout,
    /// Generic database error
    Other(String),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DatabaseError::Pool(err) => write!(f, "Database pool error: {}", err),
            DatabaseError::Diesel(err) => write!(f, "Database query error: {}", err),
            DatabaseError::Migration(err) => write!(f, "Database migration error: {}", err),
            DatabaseError::ConnectionTimeout => write!(f, "Database connection timeout"),
            DatabaseError::Other(msg) => write!(f, "Database error: {}", msg),
        }
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            DatabaseError::Pool(err) => Some(err),
            DatabaseError::Diesel(err) => Some(err),
            DatabaseError::Migration(err) => Some(err.as_ref()),
            DatabaseError::ConnectionTimeout => None,
            DatabaseError::Other(_) => None,
        }
    }
}

impl From<PoolError> for DatabaseError {
    fn from(err: PoolError) -> Self {
        DatabaseError::Pool(err)
    }
}

impl From<diesel::result::Error> for DatabaseError {
    fn from(err: diesel::result::Error) -> Self {
        DatabaseError::Diesel(err)
    }
}

#[derive(Clone)]
pub struct DatabaseManager {
    pool: DbPool,
    /// Optional sink for newly-created audit logs, consumed by the webhook
    /// dispatcher. Set once at startup via [`set_webhook_sender`].
    webhook_tx: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<crate::models::AuditLog>>,
    /// A second, independent sink for the same audit events, consumed by the
    /// Groups.io mailing-list sync. Kept separate from `webhook_tx` on purpose:
    /// the two dispatchers must not be able to starve or block one another, and
    /// each is registered (or absent) on its own.
    groupsio_tx: std::sync::OnceLock<tokio::sync::mpsc::UnboundedSender<crate::models::AuditLog>>,
    /// Cached RBAC role graph (roles + permission matrix + inheritance), the
    /// authorization decisions read from. Loaded once at startup by
    /// [`reload_rbac`] after migrations seed it; held behind an `RwLock` so a
    /// future role/permission edit can rebuild it in place (Phase 3) without a
    /// restart. Defaults to an empty graph, which grants nothing -- so a manager
    /// that was never loaded denies rather than silently permits.
    rbac_graph: std::sync::Arc<std::sync::RwLock<std::sync::Arc<crate::rbac::RoleGraph>>>,
}

#[cfg(any(test, feature = "test-support"))]
impl DatabaseManager {
    /// A manager whose pool is created **without opening any connection**.
    ///
    /// Every query through it fails fast with a pool error, which handlers map
    /// to a 500. It exists so the request-rejection surface — authentication,
    /// routing, method dispatch, body decoding — can be tested without a live
    /// PostgreSQL, which is most of the server's security-relevant behavior
    /// and none of which reaches the database. `AuthUser::from_request_parts`
    /// checks the header, the `Bearer` prefix and the JWT signature *before*
    /// its single `find_user_by_id`, so every negative case is reachable here.
    ///
    /// This is a rig, not a lowered bar. Production [`DatabaseManager::new`] is
    /// untouched: it keeps its eager connectivity probe and its blocking
    /// `min_idle` pool build. What is being routed around is an *environment*
    /// without a database, not a defect in the code.
    ///
    /// **500 is the universal "you reached the dead pool" signal**, and it is
    /// distinct in both status and body shape from every legitimate rejection.
    /// Tests built on this must therefore:
    ///
    /// * assert with `assert_eq!` on an exact status, never `is_client_error()`;
    /// * treat an unexpected 500 as a failure of the test's own premise rather
    ///   than as a result — a row whose true offline answer is 500 belongs to
    ///   the container tier, not here;
    /// * include a liveness case asserting that one known DB-reaching endpoint
    ///   *does* return 500, so that wiring a real database into this fixture
    ///   fails loudly instead of silently reinterpreting every negative result.
    ///
    /// Port 1 gives an immediate `ECONNREFUSED` rather than a routable
    /// blackhole; `min_idle(0)` stops the background reaper churning; the short
    /// timeout means a DB-reaching handler answers in milliseconds rather than
    /// after the configured thirty seconds.
    pub fn disconnected() -> Self {
        let manager = ConnectionManager::<PgConnection>::new("postgres://127.0.0.1:1/none");
        let pool = Pool::builder()
            .max_size(1)
            .min_idle(Some(0))
            .connection_timeout(std::time::Duration::from_millis(50))
            .build_unchecked(manager);
        Self {
            pool,
            webhook_tx: std::sync::OnceLock::new(),
            groupsio_tx: std::sync::OnceLock::new(),
            rbac_graph: std::sync::Arc::new(std::sync::RwLock::new(std::sync::Arc::new(
                crate::rbac::RoleGraph::default(),
            ))),
        }
    }
}

impl DatabaseManager {
    /// Create a new database manager with connection pool
    pub fn new(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let database_url = config.get_url();

        info!("Initializing database connection pool");
        debug!("Database URL: {}", mask_password(&database_url));

        // Test database connectivity first
        info!("Testing database connectivity...");
        let _test_conn = PgConnection::establish(&database_url).map_err(|e| {
            error!("Failed to establish test database connection: {}", e);
            DatabaseError::Other(format!("Connection failed: {}", e))
        })?;
        info!("Database connectivity test successful");

        // Create connection manager
        let manager = ConnectionManager::<PgConnection>::new(&database_url);

        // Build the connection pool
        let pool = Pool::builder()
            .max_size(config.max_connections)
            .min_idle(Some(config.min_connections))
            .connection_timeout(std::time::Duration::from_secs(
                config.connect_timeout_seconds,
            ))
            .idle_timeout(Some(std::time::Duration::from_secs(
                config.idle_timeout_seconds,
            )))
            .build(manager)
            .map_err(|e| {
                error!("Failed to create database connection pool: {}", e);
                DatabaseError::Pool(e)
            })?;

        info!(
            "Database connection pool created successfully (min: {}, max: {})",
            config.min_connections, config.max_connections
        );

        Ok(Self {
            pool,
            webhook_tx: std::sync::OnceLock::new(),
            groupsio_tx: std::sync::OnceLock::new(),
            rbac_graph: std::sync::Arc::new(std::sync::RwLock::new(std::sync::Arc::new(
                crate::rbac::RoleGraph::default(),
            ))),
        })
    }

    /// Rebuild the cached RBAC role graph from the database. Called once at
    /// startup after migrations have seeded the roles, and again (Phase 3) after
    /// any role/permission/inheritance edit. Fails loudly if the RBAC tables are
    /// missing or unreadable -- an unseeded graph would deny every gated route.
    pub fn reload_rbac(&self) -> Result<(), DatabaseError> {
        let mut conn = self.get_connection()?;
        let graph = crate::rbac::RoleGraph::load(&mut conn).map_err(DatabaseError::Diesel)?;
        *self
            .rbac_graph
            .write()
            .expect("rbac_graph lock is never held across a panic") = std::sync::Arc::new(graph);
        Ok(())
    }

    /// A snapshot of the cached RBAC role graph.
    pub fn rbac(&self) -> std::sync::Arc<crate::rbac::RoleGraph> {
        self.rbac_graph
            .read()
            .expect("rbac_graph lock is never held across a panic")
            .clone()
    }

    /// Does the role named `role_name` (through inheritance) hold `key`? The
    /// enforcement entry point for the extractors and in-handler gates. Reads
    /// the cached graph, so it is infallible and allocation-cheap.
    pub fn role_has_permission(&self, role_name: &str, key: &str) -> bool {
        self.rbac().has_permission_for_role_name(role_name, key)
    }

    /// Does the user hold `key` through any of their assigned roles (expanded by
    /// inheritance)? The multi-role enforcement entry point: resolves the user's
    /// `user_roles` set against the cached graph. Fallible because it reads the
    /// assignment table per call -- the extractors re-load it every request so a
    /// role change takes effect without re-issuing the token.
    pub fn user_has_permission(
        &self,
        user_id: uuid::Uuid,
        key: &str,
    ) -> Result<bool, DatabaseError> {
        let mut conn = self.get_connection()?;
        let role_ids =
            crate::rbac::roles_for_user(&mut conn, user_id).map_err(DatabaseError::Diesel)?;
        Ok(self.rbac().has_permission(&role_ids, key))
    }

    /// A user's assigned role names and their effective permissions, both sorted.
    /// Backs `GET /api/auth/me`, so the frontend can gate on permissions and show
    /// the roles a user holds. Resolves through `user_roles` -- the same source
    /// enforcement reads -- so what `me` reports and what the gates enforce agree.
    pub fn user_roles_and_permissions(
        &self,
        user_id: uuid::Uuid,
    ) -> Result<(Vec<String>, Vec<String>), DatabaseError> {
        use crate::schema::roles;
        let mut conn = self.get_connection()?;
        // Two single-table reads rather than a join: the RBAC tables are
        // deliberately not registered for cross-table queries in schema.rs, and
        // the role set per user is tiny.
        let ids = crate::rbac::roles_for_user(&mut conn, user_id).map_err(DatabaseError::Diesel)?;
        let mut names: Vec<String> = roles::table
            .filter(roles::id.eq_any(&ids))
            .select(roles::name)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        names.sort();
        let mut permissions: Vec<String> = self
            .rbac()
            .effective_permissions(&ids)
            .into_iter()
            .collect();
        permissions.sort();
        Ok((names, permissions))
    }

    // ===== RBAC administration (#65 Phase 3c) =====================================
    //
    // Writes to the roles / role_permissions / role_inheritance tables change the
    // cached graph, so each calls `reload_rbac` after committing. Writes to
    // `user_roles` (assignment) do not touch the graph -- only which roles a user
    // holds -- so they skip the reload. Policy (protecting system roles, cycle
    // rejection, last-admin protection) lives in the handlers so it can answer a
    // 4xx; these methods just perform the write.

    /// One role by id, or `None`.
    pub fn get_role(
        &self,
        role_id: uuid::Uuid,
    ) -> Result<Option<crate::rbac::Role>, DatabaseError> {
        use crate::schema::roles;
        let mut conn = self.get_connection()?;
        roles::table
            .find(role_id)
            .select(crate::rbac::Role::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// The role ids assigned to a user (for last-admin simulation in the UI).
    pub fn user_role_ids(&self, user_id: uuid::Uuid) -> Result<Vec<uuid::Uuid>, DatabaseError> {
        let mut conn = self.get_connection()?;
        crate::rbac::roles_for_user(&mut conn, user_id).map_err(DatabaseError::Diesel)
    }

    /// A user's assigned roles as `(id, name)`, ordered by name -- for the roster
    /// UI to show which roles a user holds. Two single-table reads (the RBAC
    /// tables are not registered for cross-table queries).
    pub fn user_assigned_roles(
        &self,
        user_id: uuid::Uuid,
    ) -> Result<Vec<(uuid::Uuid, String)>, DatabaseError> {
        use crate::schema::roles;
        let mut conn = self.get_connection()?;
        let ids = crate::rbac::roles_for_user(&mut conn, user_id).map_err(DatabaseError::Diesel)?;
        let mut out: Vec<(uuid::Uuid, String)> = roles::table
            .filter(roles::id.eq_any(&ids))
            .select((roles::id, roles::name))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        out.sort_by(|a, b| a.1.cmp(&b.1));
        Ok(out)
    }

    /// The permission catalog keys (for validating grants).
    pub fn permission_keys(&self) -> Result<Vec<String>, DatabaseError> {
        use crate::schema::permissions;
        let mut conn = self.get_connection()?;
        permissions::table
            .select(permissions::key)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Create a custom (non-system) role.
    pub fn create_role(
        &self,
        name: &str,
        description: &str,
        level: i16,
    ) -> Result<crate::rbac::Role, DatabaseError> {
        use crate::schema::roles;
        let mut conn = self.get_connection()?;
        let role = diesel::insert_into(roles::table)
            .values((
                roles::name.eq(name),
                roles::description.eq(description),
                roles::level.eq(level),
            ))
            .returning(crate::rbac::Role::as_returning())
            .get_result::<crate::rbac::Role>(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        self.reload_rbac()?;
        Ok(role)
    }

    /// Update a role's description and/or level (name and `is_system` are fixed).
    pub fn update_role(
        &self,
        role_id: uuid::Uuid,
        description: Option<String>,
        level: Option<i16>,
    ) -> Result<crate::rbac::Role, DatabaseError> {
        use crate::schema::roles;
        let mut conn = self.get_connection()?;
        let role = conn
            .transaction::<crate::rbac::Role, diesel::result::Error, _>(|conn| {
                if let Some(d) = &description {
                    diesel::update(roles::table.find(role_id))
                        .set(roles::description.eq(d))
                        .execute(conn)?;
                }
                if let Some(l) = level {
                    diesel::update(roles::table.find(role_id))
                        .set(roles::level.eq(l))
                        .execute(conn)?;
                }
                diesel::update(roles::table.find(role_id))
                    .set(roles::updated_at.eq(chrono::Utc::now()))
                    .execute(conn)?;
                roles::table
                    .find(role_id)
                    .select(crate::rbac::Role::as_select())
                    .first(conn)
            })
            .map_err(DatabaseError::Diesel)?;
        self.reload_rbac()?;
        Ok(role)
    }

    /// Delete a role. FK cascades remove its grants, inheritance edges, and user
    /// assignments. The handler refuses this for system roles.
    pub fn delete_role(&self, role_id: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::roles;
        let mut conn = self.get_connection()?;
        let n = diesel::delete(roles::table.find(role_id))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        self.reload_rbac()?;
        Ok(n)
    }

    /// Replace a role's direct permission grants with `keys`.
    pub fn set_role_permissions(
        &self,
        role_id: uuid::Uuid,
        keys: &[String],
    ) -> Result<(), DatabaseError> {
        use crate::schema::role_permissions;
        let mut conn = self.get_connection()?;
        conn.transaction::<(), diesel::result::Error, _>(|conn| {
            diesel::delete(role_permissions::table.filter(role_permissions::role_id.eq(role_id)))
                .execute(conn)?;
            for key in keys {
                diesel::insert_into(role_permissions::table)
                    .values((
                        role_permissions::role_id.eq(role_id),
                        role_permissions::permission_key.eq(key),
                    ))
                    .on_conflict((role_permissions::role_id, role_permissions::permission_key))
                    .do_nothing()
                    .execute(conn)?;
            }
            Ok(())
        })
        .map_err(DatabaseError::Diesel)?;
        self.reload_rbac()?;
        Ok(())
    }

    /// Replace a role's inheritance edges (the roles it inherits from). The
    /// handler validates acyclicity before calling.
    pub fn set_role_inheritance(
        &self,
        role_id: uuid::Uuid,
        parents: &[uuid::Uuid],
    ) -> Result<(), DatabaseError> {
        use crate::schema::role_inheritance;
        let mut conn = self.get_connection()?;
        conn.transaction::<(), diesel::result::Error, _>(|conn| {
            diesel::delete(role_inheritance::table.filter(role_inheritance::role_id.eq(role_id)))
                .execute(conn)?;
            for parent in parents {
                diesel::insert_into(role_inheritance::table)
                    .values((
                        role_inheritance::role_id.eq(role_id),
                        role_inheritance::inherits_role_id.eq(parent),
                    ))
                    .on_conflict((
                        role_inheritance::role_id,
                        role_inheritance::inherits_role_id,
                    ))
                    .do_nothing()
                    .execute(conn)?;
            }
            Ok(())
        })
        .map_err(DatabaseError::Diesel)?;
        self.reload_rbac()?;
        Ok(())
    }

    /// Assign a role to a user (idempotent). Does not change the graph, so no
    /// reload. This is an *additional* assignment: it does not touch the user's
    /// primary tier (see `set_user_primary_role`).
    pub fn assign_user_role(
        &self,
        user_id: uuid::Uuid,
        role_id: uuid::Uuid,
    ) -> Result<(), DatabaseError> {
        use crate::schema::user_roles;
        let mut conn = self.get_connection()?;
        diesel::insert_into(user_roles::table)
            .values((
                user_roles::user_id.eq(user_id),
                user_roles::role_id.eq(role_id),
            ))
            .on_conflict((user_roles::user_id, user_roles::role_id))
            .do_nothing()
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    /// Remove a role assignment from a user. Returns the number of rows removed.
    pub fn unassign_user_role(
        &self,
        user_id: uuid::Uuid,
        role_id: uuid::Uuid,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::user_roles;
        let mut conn = self.get_connection()?;
        diesel::delete(
            user_roles::table
                .filter(user_roles::user_id.eq(user_id))
                .filter(user_roles::role_id.eq(role_id)),
        )
        .execute(&mut conn)
        .map_err(DatabaseError::Diesel)
    }

    /// A full read of the RBAC configuration for the admin API: every role, the
    /// permission catalog, the role x permission grants, and the inheritance
    /// edges. Returned as raw rows for the handler to assemble; read straight
    /// from the tables (not the cached graph) so descriptions and `is_system`
    /// come through and the view reflects the database exactly.
    #[allow(clippy::type_complexity)]
    pub fn rbac_snapshot(
        &self,
    ) -> Result<
        (
            Vec<crate::rbac::Role>,
            Vec<(String, String)>,         // permissions: (key, description)
            Vec<(uuid::Uuid, String)>,     // role_permissions: (role_id, permission_key)
            Vec<(uuid::Uuid, uuid::Uuid)>, // role_inheritance: (role_id, inherits_role_id)
        ),
        DatabaseError,
    > {
        use crate::schema::{permissions, role_inheritance, role_permissions, roles};
        let mut conn = self.get_connection()?;
        let role_rows: Vec<crate::rbac::Role> = roles::table
            .select(crate::rbac::Role::as_select())
            .order(roles::level.asc())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        let perm_rows: Vec<(String, String)> = permissions::table
            .select((permissions::key, permissions::description))
            .order(permissions::key.asc())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        let grant_rows: Vec<(uuid::Uuid, String)> = role_permissions::table
            .select((role_permissions::role_id, role_permissions::permission_key))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        let edge_rows: Vec<(uuid::Uuid, uuid::Uuid)> = role_inheritance::table
            .select((
                role_inheritance::role_id,
                role_inheritance::inherits_role_id,
            ))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok((role_rows, perm_rows, grant_rows, edge_rows))
    }

    /// Register the channel the webhook dispatcher listens on. Called once at
    /// startup after the dispatcher is created. Subsequent calls are ignored.
    pub fn set_webhook_sender(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::models::AuditLog>,
    ) {
        let _ = self.webhook_tx.set(tx);
    }

    /// Register the channel the Groups.io sync listens on. Called once at
    /// startup after the sync service is created. Subsequent calls are ignored.
    pub fn set_groupsio_sender(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::models::AuditLog>,
    ) {
        let _ = self.groupsio_tx.set(tx);
    }

    /// Get a connection from the pool
    pub fn get_connection(&self) -> Result<DbConnection, DatabaseError> {
        self.pool.get().map_err(|e| {
            warn!("Failed to get connection from pool: {}", e);
            DatabaseError::Pool(e)
        })
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> &DbPool {
        &self.pool
    }

    /// Run database migrations
    pub fn run_migrations(&self) -> Result<(), DatabaseError> {
        info!("Running database migrations...");

        let mut conn = self.get_connection()?;

        conn.run_pending_migrations(MIGRATIONS).map_err(|e| {
            error!("Failed to run database migrations: {}", e);
            DatabaseError::Migration(e)
        })?;

        info!("Database migrations completed successfully");
        Ok(())
    }

    /// Check database health
    pub fn health_check(&self) -> Result<(), DatabaseError> {
        debug!("Performing database health check");

        let mut conn = self.get_connection()?;

        // Run a simple query to verify the connection is working
        diesel::sql_query("SELECT 1")
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        debug!("Database health check passed");
        Ok(())
    }

    /// Get connection pool status
    pub fn pool_status(&self) -> PoolStatus {
        let state = self.pool.state();
        PoolStatus {
            connections: state.connections,
            idle_connections: state.idle_connections,
        }
    }
}

/// Connection pool status information
#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub connections: u32,
    pub idle_connections: u32,
}

impl fmt::Display for PoolStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Pool status: {} total connections, {} idle",
            self.connections, self.idle_connections
        )
    }
}

/// Mask password in database URL for logging
fn mask_password(url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(url) {
        let mut masked = parsed.clone();
        if parsed.password().is_some() {
            let _ = masked.set_password(Some("***"));
        }
        masked.to_string()
    } else {
        // If URL parsing fails, just mask anything that looks like a password
        if let Some(at_pos) = url.find('@') {
            if let Some(colon_pos) = url[..at_pos].rfind(':') {
                let mut masked = url.to_string();
                if let Some(schema_pos) = url.find("://") {
                    let start = schema_pos + 3;
                    if colon_pos > start {
                        masked.replace_range(colon_pos + 1..at_pos, "***");
                    }
                }
                masked
            } else {
                url.to_string()
            }
        } else {
            url.to_string()
        }
    }
}

/// Initialize database connection pool and optionally run migrations
pub async fn initialize_database(
    config: &DatabaseConfig,
) -> Result<DatabaseManager, DatabaseError> {
    let db_manager = DatabaseManager::new(config)?;

    // Run migrations if auto_migrate is enabled
    if config.auto_migrate {
        db_manager.run_migrations()?;
    }

    // Perform initial health check
    db_manager.health_check()?;

    // Load the RBAC role graph now that migrations have seeded it. Done here,
    // after health_check, so a broken schema surfaces as a startup failure
    // rather than as every gated route silently denying at runtime.
    db_manager.reload_rbac()?;

    let status = db_manager.pool_status();
    info!("Database initialization complete. {}", status);

    Ok(db_manager)
}

/// Replace a user's role assignments with the single role named `role_name`.
///
/// Phase 3 keeps `users.role` (the primary role) and `user_roles` in lockstep:
/// authorization resolves through `user_roles`, so every `users.role` write
/// funnels through here inside the same transaction. Multi-role assignment
/// (adding roles beyond the primary) writes `user_roles` directly and is layered
/// on separately; until then a user has exactly one assignment, matching their
/// primary role. An unseeded role name clears the assignments rather than
/// failing -- consistent with the resolver, which grants an unknown role nothing.
fn sync_user_roles_to(
    conn: &mut PgConnection,
    user_id: uuid::Uuid,
    role_name: &str,
) -> Result<(), diesel::result::Error> {
    use crate::schema::{roles, user_roles};
    diesel::delete(user_roles::table.filter(user_roles::user_id.eq(user_id))).execute(conn)?;
    let role_id: Option<uuid::Uuid> = roles::table
        .filter(roles::name.eq(role_name))
        .select(roles::id)
        .first::<uuid::Uuid>(conn)
        .optional()?;
    if let Some(role_id) = role_id {
        diesel::insert_into(user_roles::table)
            .values((
                user_roles::user_id.eq(user_id),
                user_roles::role_id.eq(role_id),
            ))
            .on_conflict((user_roles::user_id, user_roles::role_id))
            .do_nothing()
            .execute(conn)?;
    }
    Ok(())
}

/// User-related database operations
impl DatabaseManager {
    /// Create a new user.
    ///
    /// Runs in a transaction so the `user_roles` assignment lands with the row:
    /// authorization resolves through `user_roles`, so a user inserted without a
    /// matching assignment would be denied every gated route. Keeps the single
    /// `users.role` and `user_roles` in lockstep (one row, the primary role);
    /// multi-role assignment is layered on later.
    pub fn create_user(&self, new_user: &NewUser, role: &str) -> Result<User, DatabaseError> {
        let mut conn = self.get_connection()?;
        conn.transaction::<User, diesel::result::Error, _>(|conn| {
            let user = diesel::insert_into(users::table)
                .values(new_user)
                .returning(User::as_returning())
                .get_result::<User>(conn)?;
            sync_user_roles_to(conn, user.id, role)?;
            Ok(user)
        })
        .map_err(DatabaseError::Diesel)
    }

    /// The user's primary (display) role: the highest-level role they hold, or
    /// `guest` when they hold none. The `users.role` enum is retired; this is the
    /// single-role view derived from `user_roles`.
    pub fn user_primary_role(&self, user_id: uuid::Uuid) -> Result<String, DatabaseError> {
        use crate::schema::roles;
        let mut conn = self.get_connection()?;
        let ids = crate::rbac::roles_for_user(&mut conn, user_id).map_err(DatabaseError::Diesel)?;
        let name: Option<String> = roles::table
            .filter(roles::id.eq_any(&ids))
            .order(roles::level.desc())
            .select(roles::name)
            .first::<String>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        Ok(name.unwrap_or_else(|| crate::models::role::GUEST.to_string()))
    }

    /// A user's effective tier level (max over their assigned roles' inheritance
    /// closures), for "this tier or higher" gates now that `users.role`/`rank()`
    /// are gone. 0 when the user holds no roles.
    pub fn user_effective_level(&self, user_id: uuid::Uuid) -> Result<i16, DatabaseError> {
        let mut conn = self.get_connection()?;
        let ids = crate::rbac::roles_for_user(&mut conn, user_id).map_err(DatabaseError::Diesel)?;
        Ok(self.rbac().effective_level(&ids))
    }

    /// Effective tier levels for many users in one pass (for door allow-list
    /// expansion, which walks the whole active roster). Users absent from the
    /// result hold no roles (treat as level 0).
    pub fn effective_levels_for(
        &self,
        user_ids: &[uuid::Uuid],
    ) -> Result<std::collections::HashMap<uuid::Uuid, i16>, DatabaseError> {
        use crate::schema::user_roles;
        let mut conn = self.get_connection()?;
        let rows: Vec<(uuid::Uuid, uuid::Uuid)> = user_roles::table
            .filter(user_roles::user_id.eq_any(user_ids))
            .select((user_roles::user_id, user_roles::role_id))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        let mut by_user: std::collections::HashMap<uuid::Uuid, Vec<uuid::Uuid>> =
            std::collections::HashMap::new();
        for (uid, rid) in rows {
            by_user.entry(uid).or_default().push(rid);
        }
        let graph = self.rbac();
        Ok(by_user
            .into_iter()
            .map(|(uid, ids)| (uid, graph.effective_level(&ids)))
            .collect())
    }

    /// Set a user's single tier role (guest/historical/active/staff/admin),
    /// preserving any additional non-tier roles they were granted. Replaces the
    /// legacy `users.role` write.
    pub fn set_user_primary_role(
        &self,
        user_id: uuid::Uuid,
        role_name: &str,
    ) -> Result<(), DatabaseError> {
        use crate::schema::{roles, user_roles};
        let mut conn = self.get_connection()?;
        conn.transaction::<(), diesel::result::Error, _>(|conn| {
            let tier_ids: Vec<uuid::Uuid> = roles::table
                .filter(roles::name.eq_any(crate::models::role::TIERS))
                .select(roles::id)
                .load(conn)?;
            diesel::delete(
                user_roles::table
                    .filter(user_roles::user_id.eq(user_id))
                    .filter(user_roles::role_id.eq_any(&tier_ids)),
            )
            .execute(conn)?;
            if let Some(role_id) = roles::table
                .filter(roles::name.eq(role_name))
                .select(roles::id)
                .first::<uuid::Uuid>(conn)
                .optional()?
            {
                diesel::insert_into(user_roles::table)
                    .values((
                        user_roles::user_id.eq(user_id),
                        user_roles::role_id.eq(role_id),
                    ))
                    .on_conflict((user_roles::user_id, user_roles::role_id))
                    .do_nothing()
                    .execute(conn)?;
            }
            Ok(())
        })
        .map_err(DatabaseError::Diesel)
    }

    /// Update a tool
    pub fn update_tool(
        &self,
        tool_id: uuid::Uuid,
        payload: &crate::api::tools::UpdateToolRequest,
    ) -> Result<crate::models::Tool, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        // Create a modified payload that includes the updated_at timestamp
        let update_payload = payload.clone();

        // Use the AsChangeset implementation to only update non-None fields
        diesel::update(tools.filter(id.eq(tool_id)))
            .set((&update_payload, updated_at.eq(chrono::Utc::now())))
            .returning(crate::models::Tool::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update tool status
    pub fn update_tool_status(
        &self,
        tool_id: uuid::Uuid,
        new_status: &crate::models::ToolStatus,
    ) -> Result<crate::models::Tool, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(tools.filter(id.eq(tool_id)))
            .set((status.eq(new_status), updated_at.eq(chrono::Utc::now())))
            .returning(crate::models::Tool::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Find user by ID
    pub fn find_user_by_id(&self, user_id: uuid::Uuid) -> Result<Option<User>, DatabaseError> {
        let mut conn = self.get_connection()?;

        let user = users::table
            .select(User::as_select())
            .filter(users::id.eq(user_id))
            .first::<User>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        Ok(user)
    }

    /// Find a user by username
    pub fn find_user_by_username(&self, username: &str) -> Result<Option<User>, DatabaseError> {
        let mut conn = self.get_connection()?;

        let user = users::table
            .select(User::as_select())
            .filter(users::username.eq(username))
            .first::<User>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        Ok(user)
    }

    /// Find a user by email address
    pub fn find_user_by_email(&self, email: &str) -> Result<Option<User>, DatabaseError> {
        let mut conn = self.get_connection()?;

        let user = users::table
            .select(User::as_select())
            .filter(users::email.eq(email))
            .first::<User>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        Ok(user)
    }

    /// Find a user by a value in their profile JSON field
    /// Uses PostgreSQL JSONB operators to query efficiently
    pub fn find_user_by_profile_field(
        &self,
        field_name: &str,
        field_value: &str,
    ) -> Result<Option<User>, DatabaseError> {
        use diesel::dsl::sql;
        use diesel::sql_types::{Bool, Text};

        let mut conn = self.get_connection()?;

        // Match either a scalar text value (`profile->>'field' = $1`) or a
        // text element inside an array (`profile->'field' ? $1`). This lets a
        // single profile field hold either one identifier (e.g. a card ID) or
        // a list of identifiers.
        let val = field_value.to_string();
        let predicate = sql::<Bool>(&format!("(profile->>'{}' = ", field_name))
            .bind::<Text, _>(val.clone())
            .sql(&format!(") OR (profile->'{}' ? ", field_name))
            .bind::<Text, _>(val)
            .sql(")");

        let user = users::table
            .select(User::as_select())
            .filter(predicate)
            .first::<User>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;

        Ok(user)
    }

    /// Resolve a scanned card code to a member, regardless of card status.
    ///
    /// Checks first-class `user_cards` first, then falls back to the legacy
    /// single value in `users.profile[profile_field]` (treated as active). A
    /// live (active/disabled) card — unique per code — wins over any released
    /// row sharing that code; a released-only code resolves to its most recent
    /// released row so a presentation is still attributable to its old owner.
    /// Find the card a presented code belongs to.
    ///
    /// Selects on the blind index when this deployment has keys, because the
    /// sealed value is non-deterministic and cannot be matched directly. Rows
    /// the backfill has not reached yet still carry plaintext and are found the
    /// old way, so a swipe works throughout the migration rather than only
    /// after it -- a member turned away at a laser because an operator had not
    /// finished a backfill is not an acceptable intermediate state.
    pub fn resolve_card(
        &self,
        profile_field: &str,
        code: &str,
        cipher: Option<&css_lib::card_crypto::CardCipher>,
    ) -> Result<CardResolution, DatabaseError> {
        let mut conn = self.get_connection()?;

        // With the plaintext `code` column dropped (#108) a card row is
        // reachable only by its blind index, which needs the cipher. A
        // deployment without keys is refused at startup, so `None` here means
        // only the legacy profile-field scheme below can still answer -- the
        // card table cannot be searched at all. Prefer a live (non-released)
        // row, then fall back to the most recent released one, exactly as
        // before; the only thing gone is the plaintext-fallback arm.
        let card = match cipher {
            Some(c) => {
                let bidx = c.blind_index(code);
                let live = user_cards::table
                    .filter(user_cards::code_bidx.eq(bidx.clone()))
                    .filter(user_cards::status.ne(CardStatus::Released))
                    .select(UserCard::as_select())
                    .first::<UserCard>(&mut conn)
                    .optional()
                    .map_err(DatabaseError::Diesel)?;
                match live {
                    Some(c) => Some(c),
                    None => user_cards::table
                        .filter(user_cards::code_bidx.eq(bidx))
                        .filter(user_cards::status.eq(CardStatus::Released))
                        .order(user_cards::created_at.desc())
                        .select(UserCard::as_select())
                        .first::<UserCard>(&mut conn)
                        .optional()
                        .map_err(DatabaseError::Diesel)?,
                }
            }
            None => None,
        };

        if let Some(card) = card {
            let user = users::table
                .find(card.user_id)
                .select(User::as_select())
                .first::<User>(&mut conn)
                .map_err(DatabaseError::Diesel)?;
            return Ok(if card.status.grants_access() {
                CardResolution::Active {
                    user,
                    card: Some(card),
                }
            } else {
                CardResolution::Revoked { user, card }
            });
        }

        // Legacy fallback: a single value in the profile JSONB field.
        match self.find_user_by_profile_field(profile_field, code)? {
            Some(user) => Ok(CardResolution::Active { user, card: None }),
            None => Ok(CardResolution::Unknown),
        }
    }

    /// Record that a card was just used (best-effort).
    pub fn touch_card_last_used(&self, card_id: uuid::Uuid) -> Result<(), DatabaseError> {
        let mut conn = self.get_connection()?;
        diesel::update(user_cards::table.find(card_id))
            .set((
                user_cards::last_used_at.eq(diesel::dsl::now),
                user_cards::updated_at.eq(diesel::dsl::now),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    /// All cards belonging to a member, newest first.
    pub fn list_user_cards(&self, user_id: uuid::Uuid) -> Result<Vec<UserCard>, DatabaseError> {
        let mut conn = self.get_connection()?;
        user_cards::table
            .filter(user_cards::user_id.eq(user_id))
            .order(user_cards::created_at.desc())
            .select(UserCard::as_select())
            .load::<UserCard>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Issue a new active card to a member. Fails with a unique-violation if the
    /// code already belongs to a live (active/disabled) card.
    /// `cipher` is `None` on a deployment that has not been given card keys
    /// yet; the card is then stored in plaintext exactly as before, which is
    /// what lets the migration be rolled out without a flag day.
    pub fn create_card(
        &self,
        user_id: uuid::Uuid,
        code: &str,
        cipher: Option<&css_lib::card_crypto::CardCipher>,
    ) -> Result<UserCard, DatabaseError> {
        let mut conn = self.get_connection()?;
        // The plaintext `code` column is gone (#108): a card can be stored only
        // sealed, which needs the cipher. Startup refuses to run without keys,
        // so a `None` here cannot happen in a real deployment -- but a caller
        // that reaches it anyway gets an error, never a row with neither a
        // plaintext nor a blind index that nothing could ever resolve.
        let cipher = cipher.ok_or_else(|| {
            DatabaseError::Other(
                "card encryption keys are required to issue a card (#108)".to_string(),
            )
        })?;
        let sealed = cipher
            .seal(code)
            .map_err(|e| DatabaseError::Other(format!("could not seal card: {e}")))?;
        // Computed at issue rather than per sync: the KDF is deliberately slow,
        // and deriving it for every member on every device poll would cost tens
        // of seconds of CPU each time.
        let wire_digest = cipher
            .wire_digest(code)
            .map_err(|e| DatabaseError::Other(format!("could not digest card: {e}")))?;
        let new_card = NewUserCard {
            user_id,
            code_encrypted: Some(sealed.ciphertext.clone()),
            code_nonce: Some(sealed.nonce.clone()),
            code_bidx: Some(sealed.blind_index.clone()),
            code_wire_digest: Some(wire_digest),
            status: Some(CardStatus::Active),
        };
        diesel::insert_into(user_cards::table)
            .values(&new_card)
            .returning(UserCard::as_returning())
            .get_result::<UserCard>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Disable a card (revoke access, keep it bound to the member).
    pub fn disable_card(
        &self,
        card_id: uuid::Uuid,
        reason: Option<&str>,
    ) -> Result<UserCard, DatabaseError> {
        let mut conn = self.get_connection()?;
        diesel::update(user_cards::table.find(card_id))
            .set((
                user_cards::status.eq(CardStatus::Disabled),
                user_cards::disabled_at.eq(diesel::dsl::now),
                user_cards::disabled_reason.eq(reason.map(|s| s.to_string())),
                user_cards::updated_at.eq(diesel::dsl::now),
            ))
            .returning(UserCard::as_returning())
            .get_result::<UserCard>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Release a card back to the pool (revoke access; code may be reissued).
    pub fn release_card(&self, card_id: uuid::Uuid) -> Result<UserCard, DatabaseError> {
        let mut conn = self.get_connection()?;
        diesel::update(user_cards::table.find(card_id))
            .set((
                user_cards::status.eq(CardStatus::Released),
                user_cards::released_at.eq(diesel::dsl::now),
                user_cards::updated_at.eq(diesel::dsl::now),
            ))
            .returning(UserCard::as_returning())
            .get_result::<UserCard>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update user information
    pub fn update_user(
        &self,
        user_id: uuid::Uuid,
        updates: &UpdateUser,
    ) -> Result<User, DatabaseError> {
        let mut conn = self.get_connection()?;

        // Set updated_at to current time if not explicitly provided
        let mut updates = updates.clone();
        if updates.updated_at.is_none() {
            updates.updated_at = Some(chrono::Utc::now().naive_utc());
        }

        // Role is no longer a `users` column; the tier role lives in `user_roles`
        // and is set via `set_user_primary_role`. This only touches profile/
        // status/etc. fields.
        diesel::update(users::table.filter(users::id.eq(user_id)))
            .set(&updates)
            .returning(User::as_returning())
            .get_result::<User>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update user profile only
    pub fn update_user_profile(
        &self,
        user_id: uuid::Uuid,
        profile: &serde_json::Value,
    ) -> Result<User, DatabaseError> {
        let mut conn = self.get_connection()?;

        let updated_user = diesel::update(users::table.filter(users::id.eq(user_id)))
            .set((
                users::profile.eq(profile),
                users::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(User::as_returning())
            .get_result::<User>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(updated_user)
    }

    /// Delete a user
    /// Delete user (soft delete by setting is_active = false)
    pub fn deactivate_user(&self, user_id: uuid::Uuid) -> Result<User, DatabaseError> {
        let mut conn = self.get_connection()?;

        diesel::update(users::table.filter(users::id.eq(user_id)))
            .set((
                users::is_active.eq(false),
                users::updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .returning(User::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Hard delete user (permanently remove from database)
    pub fn delete_user(&self, user_id: uuid::Uuid) -> Result<(), DatabaseError> {
        let mut conn = self.get_connection()?;

        // The row count is the answer, not a detail to discard.
        // Deleting nothing reported success.
        let affected = diesel::delete(users::table.filter(users::id.eq(user_id)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    /// List users with pagination
    pub fn list_users(&self, limit: i64, offset: i64) -> Result<Vec<User>, DatabaseError> {
        let mut conn = self.get_connection()?;

        let users = users::table
            .select(User::as_select())
            .filter(users::is_active.eq(true))
            .order(users::created_at.desc())
            .limit(limit)
            .offset(offset)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(users)
    }

    /// List users with pagination (alias for compatibility)
    pub fn list_users_paginated(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>, DatabaseError> {
        self.list_users(limit, offset)
    }

    /// Count total users
    pub fn count_users(&self) -> Result<i64, DatabaseError> {
        let mut conn = self.get_connection()?;

        let count = users::table
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(count)
    }

    /// List all users
    pub fn get_all_users(&self) -> Result<Vec<User>, DatabaseError> {
        let mut conn = self.get_connection()?;

        let users = users::table
            .select(User::as_select())
            .order(users::created_at.desc())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(users)
    }

    /// Get all active users (for training roster)
    pub fn get_all_active_users(&self) -> Result<Vec<User>, DatabaseError> {
        let mut conn = self.get_connection()?;

        let users = users::table
            .select(User::as_select())
            .filter(users::is_active.eq(true))
            .order(users::created_at.desc())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(users)
    }

    /// Check if a user is a trainer for a specific tool

    /// Check if a user is a trainer for any tool

    /// Count active users
    pub fn count_active_users(&self) -> Result<i64, DatabaseError> {
        let mut conn = self.get_connection()?;

        let count = users::table
            .filter(users::is_active.eq(true))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(count)
    }

    /// Get audit logs with pagination and filtering
    pub fn get_audit_logs(
        &self,
        offset: i64,
        limit: i64,
        event_type_filter: Option<String>,
    ) -> Result<Vec<crate::models::AuditLog>, diesel::result::Error> {
        use crate::schema::audit_logs::dsl::*;

        let mut conn = self
            .pool
            .get()
            .map_err(|_| diesel::result::Error::NotFound)?;

        let mut query = audit_logs.order(created_at.desc()).into_boxed();

        if let Some(event_filter) = event_type_filter {
            query = query.filter(event_type.eq(event_filter));
        }

        query
            .limit(limit)
            .offset(offset)
            .load::<crate::models::AuditLog>(&mut conn)
    }

    /// Create a new audit log entry
    pub fn create_audit_log(
        &self,
        new_audit_log: &crate::models::NewAuditLog,
    ) -> Result<crate::models::AuditLog, DatabaseError> {
        use crate::schema::audit_logs;

        let mut conn = self.get_connection()?;

        let created: crate::models::AuditLog = diesel::insert_into(audit_logs::table)
            .values(new_audit_log)
            .returning(crate::models::AuditLog::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // Fan the event out to the webhook dispatcher, if one is registered.
        // Never fails the audit write: a closed/absent channel is ignored.
        if let Some(tx) = self.webhook_tx.get() {
            let _ = tx.send(created.clone());
        }
        // ...and independently to the Groups.io sync, same contract. A send that
        // fails means the sender is registered but its consumer task is gone -- a
        // real fault worth logging, unlike an absent sender (the module is simply
        // disabled).
        if let Some(tx) = self.groupsio_tx.get() {
            if tx.send(created.clone()).is_err() {
                tracing::error!(
                    "groupsio_tx send failed (consumer gone) for {}",
                    created.event_type
                );
            }
        }

        Ok(created)
    }

    /// Get tools with filtering
    pub fn get_tools(
        &self,
        query: crate::api::tools::ToolQuery,
    ) -> Result<Vec<crate::models::Tool>, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        let mut diesel_query = tools.into_boxed();

        if let Some(cat) = query.category {
            diesel_query = diesel_query.filter(category.eq(cat));
        }

        if let Some(stat) = query.status {
            diesel_query = diesel_query.filter(status.eq(stat));
        }

        if let Some(training_required) = query.requires_training {
            diesel_query = diesel_query.filter(requires_training.eq(training_required));
        }

        // Apply pagination
        let page = query.page.unwrap_or(1);
        let per_page = std::cmp::min(query.per_page.unwrap_or(50), 100);
        let offset = (page - 1) * per_page;

        diesel_query
            .order(created_at.desc())
            .limit(per_page as i64)
            .offset(offset as i64)
            .load::<crate::models::Tool>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get a tool by ID
    pub fn get_tool_by_id(
        &self,
        tool_id: uuid::Uuid,
    ) -> Result<Option<crate::models::Tool>, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        tools
            .find(tool_id)
            .first::<crate::models::Tool>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Get a tool by external_id (for ToolGuard integration)
    pub fn get_tool_by_external_id(
        &self,
        external_id_value: &str,
    ) -> Result<Option<crate::models::Tool>, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        tools
            .filter(external_id.eq(external_id_value))
            .first::<crate::models::Tool>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Create a new tool
    pub fn create_tool(
        &self,
        new_tool: &crate::models::NewTool,
    ) -> Result<crate::models::Tool, DatabaseError> {
        use crate::schema::tools;

        let mut conn = self.get_connection()?;

        diesel::insert_into(tools::table)
            .values(new_tool)
            .returning(crate::models::Tool::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get all tools with InUse status (for boot-reset)
    pub fn get_inuse_tools(&self) -> Result<Vec<crate::models::Tool>, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        tools
            .into_boxed()
            .filter(status.eq(crate::models::ToolStatus::InUse))
            .load::<crate::models::Tool>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Delete a tool
    pub fn delete_tool(&self, tool_id: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        // The row count is the answer, not a detail to discard.
        // Deleting nothing reported success.
        let affected = diesel::delete(tools.find(tool_id))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    /// Create a tool event
    pub fn create_tool_event(
        &self,
        new_event: &crate::models::NewToolEvent,
    ) -> Result<crate::models::ToolEvent, DatabaseError> {
        use crate::schema::tool_events;

        let mut conn = self.get_connection()?;

        diesel::insert_into(tool_events::table)
            .values(new_event)
            .returning(crate::models::ToolEvent::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }
    /// Get tool events
    pub fn get_tool_events(
        &self,
        _tool_id: uuid::Uuid,
    ) -> Result<Vec<crate::models::ToolEvent>, DatabaseError> {
        use crate::schema::tool_events::dsl::*;

        let mut conn = self.get_connection()?;

        tool_events
            .filter(tool_id.eq(_tool_id))
            .order(created_at.desc())
            .load::<crate::models::ToolEvent>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Check if user has valid training for a tool
    pub fn user_has_valid_training(
        &self,
        user_id: uuid::Uuid,
        tool_id: uuid::Uuid,
    ) -> Result<bool, DatabaseError> {
        use crate::schema::{tool_training_types, user_tool_training};

        let mut conn = self.get_connection()?;

        let training_count: i64 = user_tool_training::table
            .inner_join(tool_training_types::table)
            .filter(user_tool_training::user_id.eq(user_id))
            .filter(tool_training_types::tool_id.eq(tool_id))
            .filter(
                user_tool_training::expires_at
                    .is_null()
                    .or(user_tool_training::expires_at.gt(chrono::Utc::now())),
            )
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(training_count > 0)
    }

    /// Check if a tool has any training types defined
    pub fn tool_has_training_types(&self, tool_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::tool_training_types;

        let mut conn = self.get_connection()?;

        let training_type_count: i64 = tool_training_types::table
            .filter(tool_training_types::tool_id.eq(tool_id))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(training_type_count > 0)
    }

    /// Check if a tool has any training steps defined
    pub fn tool_has_training_steps(&self, tool_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::training_steps;

        let mut conn = self.get_connection()?;

        let step_count: i64 = training_steps::table
            .filter(training_steps::tool_id.eq(tool_id))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(step_count > 0)
    }

    /// Check if a user has completed all required training steps for a tool
    /// Returns true if all steps are completed, false otherwise
    pub fn user_has_completed_all_training_steps(
        &self,
        user_id: uuid::Uuid,
        tool_id: uuid::Uuid,
    ) -> Result<bool, DatabaseError> {
        use crate::schema::{training_steps, user_training_progress};

        let mut conn = self.get_connection()?;

        // Get all training steps for the tool
        let all_steps: Vec<uuid::Uuid> = training_steps::table
            .filter(training_steps::tool_id.eq(tool_id))
            .select(training_steps::id)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // If no steps exist, consider it as "no training required"
        if all_steps.is_empty() {
            return Ok(true);
        }

        // For each step, check if the user has completed it
        for step_id in &all_steps {
            let progress = user_training_progress::table
                .filter(user_training_progress::user_id.eq(user_id))
                .filter(user_training_progress::training_step_id.eq(step_id))
                .select((
                    user_training_progress::status,
                    user_training_progress::expires_at,
                ))
                .first::<(
                    crate::models::TrainingStatus,
                    Option<chrono::DateTime<chrono::Utc>>,
                )>(&mut conn)
                .optional()
                .map_err(DatabaseError::Diesel)?;

            match progress {
                Some((status, expires_at)) => {
                    // Check if completed
                    if status != crate::models::TrainingStatus::Completed {
                        return Ok(false);
                    }

                    // Also check if it's expired
                    if let Some(expiry) = expires_at {
                        if expiry < chrono::Utc::now() {
                            return Ok(false); // Training expired
                        }
                    }
                }
                None => {
                    // User hasn't started this step
                    return Ok(false);
                }
            }
        }

        // All steps completed and valid
        Ok(true)
    }

    /// Whether the user has an active (unexpired) training waiver for the tool.
    pub fn user_has_active_waiver(
        &self,
        user_id: uuid::Uuid,
        tool_id: uuid::Uuid,
    ) -> Result<bool, DatabaseError> {
        let mut conn = self.get_connection()?;
        let count: i64 = training_waivers::table
            .filter(training_waivers::user_id.eq(user_id))
            .filter(training_waivers::tool_id.eq(tool_id))
            .filter(
                training_waivers::expires_at
                    .is_null()
                    .or(training_waivers::expires_at.gt(diesel::dsl::now)),
            )
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(count > 0)
    }

    /// The single source of truth for "may this user use this tool", used by
    /// both the online tool-on path and the edge sync builder.
    ///
    /// - Ungated (no training steps AND not `requires_training`) → open to all.
    /// - Otherwise authorized iff an active waiver exists, or (when the tool has
    ///   steps) the user has completed all of them. A `requires_training` tool
    ///   with no steps and no waiver is therefore closed — the case that lets a
    ///   migrated ToolPass grant preserve its access restriction.
    pub fn user_is_authorized_for_tool(
        &self,
        user_id: uuid::Uuid,
        tool_id: uuid::Uuid,
        requires_training: bool,
    ) -> Result<bool, DatabaseError> {
        // Emergency lockout is a hard override, independent of who is asking:
        // a tool whose own firmware self-tripped, or whose circuit is in
        // lockout, is denied outright (#44). Checked before every other arm --
        // including the free-tool early return below -- so a locked circuit's
        // step-less tool is still refused. Both the web self-check and the edge
        // allow-list resolve through this one rule, so they cannot disagree.
        if self.tool_is_locked_out(tool_id)? {
            return Ok(false);
        }
        let has_steps = self.tool_has_training_steps(tool_id)?;
        if !has_steps && !requires_training {
            return Ok(true);
        }
        if self.user_has_active_waiver(user_id, tool_id)? {
            return Ok(true);
        }
        if has_steps {
            return self.user_has_completed_all_training_steps(user_id, tool_id);
        }
        // requires_training, no steps, no waiver.
        Ok(false)
    }

    /// Grant (or update, keyed on (user, tool)) a training waiver.
    pub fn upsert_waiver(
        &self,
        waiver: &NewTrainingWaiver,
    ) -> Result<TrainingWaiver, DatabaseError> {
        let mut conn = self.get_connection()?;
        diesel::insert_into(training_waivers::table)
            .values(waiver)
            .on_conflict((training_waivers::user_id, training_waivers::tool_id))
            .do_update()
            .set((
                training_waivers::reason.eq(&waiver.reason),
                training_waivers::waived_by.eq(waiver.waived_by),
                training_waivers::expires_at.eq(waiver.expires_at),
                training_waivers::updated_at.eq(diesel::dsl::now),
            ))
            .returning(TrainingWaiver::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// All waivers held by a member, newest first.
    pub fn list_waivers_for_user(
        &self,
        user_id: uuid::Uuid,
    ) -> Result<Vec<TrainingWaiver>, DatabaseError> {
        let mut conn = self.get_connection()?;
        training_waivers::table
            .filter(training_waivers::user_id.eq(user_id))
            .order(training_waivers::created_at.desc())
            .select(TrainingWaiver::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Revoke (delete) a waiver by id; returns the deleted row for auditing.
    pub fn revoke_waiver(&self, id: uuid::Uuid) -> Result<TrainingWaiver, DatabaseError> {
        let mut conn = self.get_connection()?;
        diesel::delete(training_waivers::table.find(id))
            .returning(TrainingWaiver::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ==================== TOOL RATE TIERS (#34) ====================

    /// Resolve the effective billing rate for a member on a tool: the member's
    /// assigned tier if any, else the tool's own default (`usage_*`). `max`
    /// falls back tier -> tool. The one place tier resolution lives, so the web
    /// self-check, the edge allow-list, and the session hold cannot disagree.
    /// (An assignment applies regardless of the tier's `active` flag; `active`
    /// only governs whether the tier is offered for new assignments.)
    pub fn resolve_effective_billing(
        &self,
        uid: uuid::Uuid,
        tool: &crate::models::Tool,
    ) -> Result<crate::tool_billing::EffectiveRate, DatabaseError> {
        use crate::schema::{tool_rate_tiers as t, tool_tier_assignments as a};
        let mut conn = self.get_connection()?;
        let assigned = a::table
            .inner_join(t::table)
            .filter(a::user_id.eq(uid))
            .filter(a::tool_id.eq(tool.id))
            .select((t::id, t::flat_fee, t::rate_per_min, t::max_session_minutes))
            .first::<(
                uuid::Uuid,
                Option<bigdecimal::BigDecimal>,
                Option<bigdecimal::BigDecimal>,
                Option<i32>,
            )>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        Ok(match assigned {
            Some((tier_id, flat, rate, max)) => crate::tool_billing::EffectiveRate {
                flat_fee: flat,
                rate_per_min: rate,
                max_session_minutes: max.or(tool.usage_max_session_minutes),
                tier_id: Some(tier_id),
            },
            None => crate::tool_billing::EffectiveRate {
                flat_fee: tool.usage_flat_fee.clone(),
                rate_per_min: tool.usage_rate_per_min.clone(),
                max_session_minutes: tool.usage_max_session_minutes,
                tier_id: None,
            },
        })
    }

    pub fn list_tiers_for_tool(
        &self,
        tid: uuid::Uuid,
    ) -> Result<Vec<crate::models::ToolRateTier>, DatabaseError> {
        use crate::schema::tool_rate_tiers::dsl::*;
        let mut conn = self.get_connection()?;
        tool_rate_tiers
            .filter(tool_id.eq(tid))
            .order(name.asc())
            .select(crate::models::ToolRateTier::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_tier(&self, tid: uuid::Uuid) -> Result<crate::models::ToolRateTier, DatabaseError> {
        use crate::schema::tool_rate_tiers::dsl::*;
        let mut conn = self.get_connection()?;
        tool_rate_tiers
            .find(tid)
            .select(crate::models::ToolRateTier::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_tier(
        &self,
        new_tier: &crate::models::NewToolRateTier,
    ) -> Result<crate::models::ToolRateTier, DatabaseError> {
        use crate::schema::tool_rate_tiers;
        let mut conn = self.get_connection()?;
        diesel::insert_into(tool_rate_tiers::table)
            .values(new_tier)
            .returning(crate::models::ToolRateTier::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_tier(
        &self,
        tid: uuid::Uuid,
        changes: &crate::models::UpdateToolRateTier,
    ) -> Result<crate::models::ToolRateTier, DatabaseError> {
        use crate::schema::tool_rate_tiers::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(tool_rate_tiers.find(tid))
            .set(changes)
            .returning(crate::models::ToolRateTier::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_tier(&self, tid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::tool_rate_tiers::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(tool_rate_tiers.find(tid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_tier_assignments_for_tool(
        &self,
        tid: uuid::Uuid,
    ) -> Result<Vec<crate::models::ToolTierAssignment>, DatabaseError> {
        use crate::schema::tool_tier_assignments::dsl::*;
        let mut conn = self.get_connection()?;
        tool_tier_assignments
            .filter(tool_id.eq(tid))
            .select(crate::models::ToolTierAssignment::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Assign (or re-assign, keyed on (user, tool)) a member to a rate tier.
    pub fn upsert_tier_assignment(
        &self,
        assignment: &crate::models::NewToolTierAssignment,
    ) -> Result<crate::models::ToolTierAssignment, DatabaseError> {
        use crate::schema::tool_tier_assignments::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::insert_into(tool_tier_assignments)
            .values(assignment)
            .on_conflict((user_id, tool_id))
            .do_update()
            .set((
                tier_id.eq(assignment.tier_id),
                assigned_by.eq(assignment.assigned_by),
                updated_at.eq(chrono::Utc::now()),
            ))
            .returning(crate::models::ToolTierAssignment::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Clear a member's tier on a tool (back to the tool default). Returns the
    /// number of rows removed (0 = none was set).
    pub fn clear_tier_assignment(
        &self,
        uid: uuid::Uuid,
        tid: uuid::Uuid,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::tool_tier_assignments::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(
            tool_tier_assignments
                .filter(user_id.eq(uid))
                .filter(tool_id.eq(tid)),
        )
        .execute(&mut conn)
        .map_err(DatabaseError::Diesel)
    }

    /// Every (user, tool) tier assignment joined to its tier's rate, for
    /// building the allow-list once without an N+1 per-(user,tool) lookup.
    #[allow(clippy::type_complexity)]
    pub fn all_tier_rates(
        &self,
    ) -> Result<
        Vec<(
            uuid::Uuid,
            uuid::Uuid,
            Option<bigdecimal::BigDecimal>,
            Option<bigdecimal::BigDecimal>,
            Option<i32>,
        )>,
        DatabaseError,
    > {
        use crate::schema::{tool_rate_tiers as t, tool_tier_assignments as a};
        let mut conn = self.get_connection()?;
        a::table
            .inner_join(t::table)
            .select((
                a::user_id,
                a::tool_id,
                t::flat_fee,
                t::rate_per_min,
                t::max_session_minutes,
            ))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ==================== TRAINING SYSTEM DATABASE METHODS ====================

    /// Create a new training step
    pub fn create_training_step(
        &self,
        new_step: &crate::models::NewTrainingStep,
    ) -> Result<crate::models::TrainingStep, DatabaseError> {
        use crate::schema::training_steps;

        let mut conn = self.get_connection()?;

        diesel::insert_into(training_steps::table)
            .values(new_step)
            .returning(crate::models::TrainingStep::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get training steps with optional filtering
    pub fn get_training_steps(
        &self,
        query: &crate::api::training::TrainingQuery,
    ) -> Result<Vec<crate::models::TrainingStep>, DatabaseError> {
        use crate::schema::training_steps::dsl::*;

        let mut conn = self.get_connection()?;
        let mut diesel_query = training_steps.into_boxed();

        if let Some(tool_id_filter) = query.tool_id {
            diesel_query = diesel_query.filter(tool_id.eq(tool_id_filter));
        }

        let steps = diesel_query
            .order(step_number.asc())
            .select(crate::models::TrainingStep::as_select())
            .load::<crate::models::TrainingStep>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(steps)
    }

    /// Get a specific training step by ID
    pub fn get_training_step_by_id(
        &self,
        step_id: uuid::Uuid,
    ) -> Result<Option<crate::models::TrainingStep>, DatabaseError> {
        use crate::schema::training_steps::dsl::*;

        let mut conn = self.get_connection()?;

        let step = training_steps
            .filter(id.eq(step_id))
            .select(crate::models::TrainingStep::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;

        Ok(step)
    }

    /// Update a training step
    pub fn update_training_step(
        &self,
        step_id: uuid::Uuid,
        update_step: &crate::models::UpdateTrainingStep,
    ) -> Result<crate::models::TrainingStep, DatabaseError> {
        use crate::schema::training_steps::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(training_steps.filter(id.eq(step_id)))
            .set((update_step, updated_at.eq(chrono::Utc::now())))
            .returning(crate::models::TrainingStep::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update training step position/order
    pub fn update_training_step_position(
        &self,
        step_id: uuid::Uuid,
        new_step_number: i32,
    ) -> Result<(), DatabaseError> {
        use crate::schema::training_steps::dsl::*;

        let mut conn = self.get_connection()?;

        // The row count is the answer, not a detail to discard.
        // Renumbering a step that does not exist reported success.
        let affected = diesel::update(training_steps.filter(id.eq(step_id)))
            .set((
                step_number.eq(new_step_number),
                updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    /// Check if any users have progress for this training step
    pub fn has_user_progress_for_step(&self, step_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        let mut conn = self.get_connection()?;

        let count: i64 = crate::schema::user_training_progress::table
            .filter(crate::schema::user_training_progress::training_step_id.eq(step_id))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(count > 0)
    }

    /// Delete a training step
    pub fn delete_training_step(&self, step_id: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::training_steps::dsl::*;

        let mut conn = self.get_connection()?;

        // The row count is the answer, not a detail to discard.
        // Deleting nothing reported success.
        let affected = diesel::delete(training_steps.filter(id.eq(step_id)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    /// Add a training prerequisite
    pub fn add_training_prerequisite(
        &self,
        new_prereq: &crate::models::NewTrainingPrerequisite,
    ) -> Result<crate::models::TrainingPrerequisite, DatabaseError> {
        use crate::schema::training_prerequisites;

        let mut conn = self.get_connection()?;

        diesel::insert_into(training_prerequisites::table)
            .values(new_prereq)
            .returning(crate::models::TrainingPrerequisite::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get training prerequisites for a step
    pub fn get_training_prerequisites(
        &self,
        step_id: uuid::Uuid,
    ) -> Result<Vec<crate::models::TrainingStep>, DatabaseError> {
        use crate::schema::{training_prerequisites, training_steps};

        let mut conn = self.get_connection()?;

        let prerequisites = training_prerequisites::table
            .inner_join(
                training_steps::table
                    .on(training_steps::id.eq(training_prerequisites::prerequisite_step_id)),
            )
            .filter(training_prerequisites::training_step_id.eq(step_id))
            .select(crate::models::TrainingStep::as_select())
            .load::<crate::models::TrainingStep>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(prerequisites)
    }

    /// Remove a training prerequisite
    pub fn remove_training_prerequisite(&self, prereq_id: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::training_prerequisites::dsl::*;

        let mut conn = self.get_connection()?;

        // The row count is the answer, not a detail to discard.
        // Reached with a `training_steps` id where this deletes from
        // `training_prerequisites`, so it matched nothing on every call and
        // answered 200 -- the prerequisite stayed on screen.
        let affected = diesel::delete(training_prerequisites.filter(id.eq(prereq_id)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    /// Get user training progress with filtering
    pub fn get_user_training_progress(
        &self,
        user_id_param: uuid::Uuid,
        query: &crate::api::training::TrainingQuery,
    ) -> Result<Vec<crate::models::UserTrainingProgress>, DatabaseError> {
        use crate::schema::user_training_progress::dsl::*;

        let mut conn = self.get_connection()?;
        let mut diesel_query = user_training_progress.into_boxed();

        diesel_query = diesel_query.filter(user_id.eq(user_id_param));

        if let Some(status_filter) = &query.status {
            diesel_query = diesel_query.filter(status.eq(status_filter));
        }

        if let Some(instructor_filter) = query.instructor_id {
            diesel_query = diesel_query.filter(instructor_id.eq(instructor_filter));
        }

        let progress = diesel_query
            .select(crate::models::UserTrainingProgress::as_select())
            .load::<crate::models::UserTrainingProgress>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(progress)
    }

    /// Get specific user training progress for a step
    pub fn get_user_training_progress_for_step(
        &self,
        user_id_param: uuid::Uuid,
        step_id: uuid::Uuid,
    ) -> Result<Option<crate::models::UserTrainingProgress>, DatabaseError> {
        use crate::schema::user_training_progress::dsl::*;

        let mut conn = self.get_connection()?;

        let progress = user_training_progress
            .filter(user_id.eq(user_id_param))
            .filter(training_step_id.eq(step_id))
            .select(crate::models::UserTrainingProgress::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;

        Ok(progress)
    }

    /// Update user training progress
    pub fn update_user_training_progress(
        &self,
        user_id_param: uuid::Uuid,
        step_id: uuid::Uuid,
        update_progress: &crate::models::UpdateUserTrainingProgress,
    ) -> Result<crate::models::UserTrainingProgress, DatabaseError> {
        use crate::schema::user_training_progress::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(
            user_training_progress
                .filter(user_id.eq(user_id_param))
                .filter(training_step_id.eq(step_id)),
        )
        .set((update_progress, updated_at.eq(chrono::Utc::now())))
        .returning(crate::models::UserTrainingProgress::as_returning())
        .get_result(&mut conn)
        .map_err(DatabaseError::Diesel)
    }

    /// Check if user is a certified instructor for a training step
    pub fn is_certified_instructor(
        &self,
        user_id: uuid::Uuid,
        step_id: uuid::Uuid,
    ) -> Result<bool, DatabaseError> {
        use crate::schema::training_instructors;

        let mut conn = self.get_connection()?;

        let count: i64 = training_instructors::table
            .filter(training_instructors::user_id.eq(user_id))
            .filter(training_instructors::training_step_id.eq(step_id))
            .filter(
                training_instructors::expires_at
                    .is_null()
                    .or(training_instructors::expires_at.gt(chrono::Utc::now())),
            )
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(count > 0)
    }

    /// Certify a user as instructor
    pub fn certify_instructor(
        &self,
        new_instructor: &crate::models::NewTrainingInstructor,
    ) -> Result<crate::models::TrainingInstructor, DatabaseError> {
        use crate::schema::training_instructors;

        let mut conn = self.get_connection()?;

        diesel::insert_into(training_instructors::table)
            .values(new_instructor)
            .returning(crate::models::TrainingInstructor::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get training instructors with filtering
    pub fn get_training_instructors(
        &self,
        query: &crate::api::training::TrainingQuery,
    ) -> Result<Vec<crate::models::TrainingInstructor>, DatabaseError> {
        use crate::schema::training_instructors::dsl::*;

        let mut conn = self.get_connection()?;
        let mut diesel_query = training_instructors.into_boxed();

        if let Some(user_filter) = query.user_id {
            diesel_query = diesel_query.filter(user_id.eq(user_filter));
        }

        let instructors = diesel_query
            .select(crate::models::TrainingInstructor::as_select())
            .load::<crate::models::TrainingInstructor>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(instructors)
    }

    /// Revoke instructor certification
    pub fn revoke_instructor_certification(
        &self,
        instructor_id: uuid::Uuid,
    ) -> Result<(), DatabaseError> {
        use crate::schema::training_instructors::dsl::*;

        let mut conn = self.get_connection()?;

        // The row count is the answer, not a detail to discard.
        // Revoking a certification nobody held reported success.
        let affected = diesel::delete(training_instructors.filter(id.eq(instructor_id)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    // ==================== TRAINERS DATABASE METHODS ====================

    /// Create a new training record
    /// Record a training completion by upserting the user's `user_training_progress`
    /// row for a step. This is the one shared completion path -- the cmi5 grant
    /// and any other sign-off flow call it -- so the web and edge access checks
    /// stay in agreement (`checks/tests/tool_access_agrees.rs`). It deliberately
    /// lives here in `database.rs`, not in `cmi5.rs`
    /// (`checks/tests/cmi5_grant_goes_through_the_gate.rs`). `completed_at` and
    /// `expires_at` are set only for a `completed` status, the latter from the
    /// step's retraining interval.
    pub fn record_step_completion(
        &self,
        trainee_user_id: uuid::Uuid,
        training_step_id: uuid::Uuid,
        instructor_id: uuid::Uuid,
        completion_status: &str,
        notes: Option<String>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::user_training_progress;

        let training_status = match completion_status {
            "completed" => crate::models::TrainingStatus::Completed,
            "partial" => crate::models::TrainingStatus::InProgress,
            "failed" => crate::models::TrainingStatus::Failed,
            _ => crate::models::TrainingStatus::InProgress,
        };

        let expires_at = if training_status == crate::models::TrainingStatus::Completed {
            if let Some(step) = self.get_training_step_by_id(training_step_id)? {
                step.calculate_expiry_date()
            } else {
                None
            }
        } else {
            None
        };
        let completed_at = if training_status == crate::models::TrainingStatus::Completed {
            Some(chrono::Utc::now())
        } else {
            None
        };

        let new_progress = crate::models::NewUserTrainingProgress {
            user_id: trainee_user_id,
            training_step_id,
            status: Some(training_status.clone()),
            instructor_id: Some(instructor_id),
            started_at: Some(chrono::Utc::now()),
            notes: notes.clone(),
        };

        let mut conn = self.get_connection()?;
        diesel::insert_into(user_training_progress::table)
            .values(&new_progress)
            .on_conflict((
                user_training_progress::user_id,
                user_training_progress::training_step_id,
            ))
            .do_update()
            .set((
                user_training_progress::status.eq(training_status),
                user_training_progress::instructor_id.eq(instructor_id),
                user_training_progress::completed_at.eq(completed_at),
                user_training_progress::expires_at.eq(expires_at),
                user_training_progress::notes.eq(&notes),
                user_training_progress::updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    /// Get training records with users information

    /// Get training records for a specific user (either as trainer or trainee)

    /// Update a training record

    /// Create a new tool trainer assignment

    /// Get tool trainers with optional filtering

    /// Update tool trainer assignment

    /// Remove tool trainer assignment (soft delete).
    ///
    /// Note the `_param` suffixes. `use ...::dsl::*` brings every *column* of
    /// the table into scope, so a parameter named `user_id` is shadowed by the
    /// column named `user_id` -- and `.filter(user_id.eq(user_id))` then
    /// compiles as `user_id = user_id`, which is true for every row.
    ///
    /// A near-duplicate of this function existed with exactly that mistake. It
    /// was reachable from nothing, which is the only reason it never ran: an
    /// "unassign this trainer from this tool" that deactivated every trainer
    /// assignment in the table. rustc's only complaint was "unused variable".
    /// `checks/tests/dsl_glob_shadowing.rs` now fails on the pattern.

    /// Get training history for a specific tool with detailed information
    pub fn get_training_history_for_tool(
        &self,
        tool_id: uuid::Uuid,
        query: &crate::api::training::TrainingHistoryQuery,
    ) -> Result<Vec<crate::api::training::TrainingHistoryRecord>, DatabaseError> {
        use crate::schema::{tools, training_steps, user_training_progress, users};
        let mut conn = self.get_connection()?;

        // Training history is derived from the structured progress table
        // (`user_training_progress`) joined to the tool's steps -- the free-form
        // `training_records` table it used to read has been retired. A row is a
        // step completion: the trainee is `user_id`, the person who signed it off
        // is `instructor_id`, and the date is `completed_at` (falling back to
        // `created_at` for not-yet-complete rows).
        let tool_name: Option<String> = tools::table
            .filter(tools::id.eq(tool_id))
            .select(tools::name)
            .first::<String>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        let Some(tool_name) = tool_name else {
            return Ok(Vec::new());
        };

        // Column-level filters run in SQL. The status filter runs in Rust (the
        // enum column, like elsewhere, is not filtered in SQL) and the date
        // filter runs against the derived `training_date`, so pagination is
        // applied last, in Rust, over the fully filtered set. The per-tool
        // progress set is small (users x that tool's steps).
        let mut q = user_training_progress::table
            .inner_join(
                training_steps::table
                    .on(training_steps::id.eq(user_training_progress::training_step_id)),
            )
            .filter(training_steps::tool_id.eq(tool_id))
            .into_boxed();
        if let Some(trainee) = query.trainee_id {
            q = q.filter(user_training_progress::user_id.eq(trainee));
        }
        if let Some(trainer) = query.trainer_id {
            q = q.filter(user_training_progress::instructor_id.eq(trainer));
        }
        if let Some(step) = query.step_id {
            q = q.filter(user_training_progress::training_step_id.eq(step));
        }

        #[allow(clippy::type_complexity)]
        let rows: Vec<(
            uuid::Uuid,                            // progress id
            uuid::Uuid,                            // trainee user_id
            Option<uuid::Uuid>,                    // instructor_id
            uuid::Uuid,                            // training_step_id
            crate::models::TrainingStatus,         // status
            Option<chrono::DateTime<chrono::Utc>>, // completed_at
            Option<String>,                        // notes
            chrono::DateTime<chrono::Utc>,         // created_at
            chrono::DateTime<chrono::Utc>,         // updated_at
            String,                                // step_name
            i32,                                   // step_number
        )> = q
            .select((
                user_training_progress::id,
                user_training_progress::user_id,
                user_training_progress::instructor_id,
                user_training_progress::training_step_id,
                user_training_progress::status,
                user_training_progress::completed_at,
                user_training_progress::notes,
                user_training_progress::created_at,
                user_training_progress::updated_at,
                training_steps::step_name,
                training_steps::step_number,
            ))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        let status_filter = query.completion_status.as_deref();
        let mut history_records: Vec<crate::api::training::TrainingHistoryRecord> = Vec::new();
        for (
            id,
            trainee_user_id,
            instructor_id,
            training_step_id,
            status,
            completed_at,
            notes,
            created_at,
            updated_at,
            step_name,
            step_number,
        ) in rows
        {
            let completion_status = status.as_str().to_string();
            if let Some(f) = status_filter {
                if completion_status != f {
                    continue;
                }
            }
            let training_date = completed_at.unwrap_or(created_at).date_naive();
            if let Some(from) = query.start_date {
                if training_date < from {
                    continue;
                }
            }
            if let Some(until) = query.end_date {
                if training_date > until {
                    continue;
                }
            }

            // The person who signed it off; falls back to the trainee for a
            // self-directed completion (e.g. cmi5) that records itself as actor.
            let trainer_user_id = instructor_id.unwrap_or(trainee_user_id);

            let (trainee_name, trainee_email) = users::table
                .filter(users::id.eq(trainee_user_id))
                .select((users::full_name, users::email))
                .first::<(String, String)>(&mut conn)
                .map_err(DatabaseError::Diesel)?;
            let (trainer_name, trainer_email) = users::table
                .filter(users::id.eq(trainer_user_id))
                .select((users::full_name, users::email))
                .first::<(String, String)>(&mut conn)
                .map_err(DatabaseError::Diesel)?;

            history_records.push(crate::api::training::TrainingHistoryRecord {
                id,
                tool_id,
                tool_name: tool_name.clone(),
                training_step_id,
                step_name,
                step_number,
                trainee_user_id,
                trainee_name,
                trainee_email,
                trainer_user_id,
                trainer_name,
                trainer_email,
                training_date,
                completion_status,
                minutes_trained: None,
                notes,
                created_at: created_at.naive_utc(),
                updated_at: updated_at.naive_utc(),
            });
        }

        // Newest first, then paginate over the filtered set.
        history_records.sort_by(|a, b| b.training_date.cmp(&a.training_date));
        let per_page = query.per_page.unwrap_or(50).clamp(1, 500) as usize;
        let page = query.page.unwrap_or(1).max(1) as usize;
        let start = (page - 1).saturating_mul(per_page);
        let paged = history_records
            .into_iter()
            .skip(start)
            .take(per_page)
            .collect();
        Ok(paged)
    }

    /// Check if user can access a tool (all training complete and valid)
    pub fn can_access_tool(
        &self,
        user_id: uuid::Uuid,
        tool_id: uuid::Uuid,
        metered_gate: Option<&crate::tool_billing::MeteredGate>,
    ) -> Result<bool, DatabaseError> {
        // First check if tool requires training
        let tool = self
            .get_tool_by_id(tool_id)?
            .ok_or_else(|| DatabaseError::Diesel(diesel::result::Error::NotFound))?;

        // Training gate, one rule, one implementation.
        //
        // The web path used to re-derive the answer with a bare existence test
        // that read neither `status` nor `expires_at`, while the sync path asked
        // `user_has_completed_all_training_steps`, which reads both -- so the web
        // UI told members they could use machines the door would not open.
        //
        // Deliberately still not identical to the sync path: that one keys off
        // `tool_has_training_steps` and ignores `tool.requires_training`, so a
        // tool with the flag off and steps configured is answered differently
        // there than here. checks/tests/tool_access_agrees.rs pins both halves.
        // One shared rule for the training gate: step completion, an active
        // waiver, or the tool not being access-controlled.
        if !self.user_is_authorized_for_tool(user_id, tool_id, tool.requires_training)? {
            return Ok(false);
        }

        // Metered-billing gate (Phase 2). When tool billing is on, the same
        // affordability + membership check the live guard and the edge allow-list
        // apply, so the web self-report agrees with what the machine will do
        // rather than promising access a member cannot afford.
        if let Some(gate) = metered_gate {
            let Some(_user) = self.find_user_by_id(user_id)? else {
                return Ok(false);
            };
            let available = self.available_balance(user_id)?;
            let is_member = self.user_has_permission(user_id, "member.access")?;
            // #34: gate on THIS member's resolved rate (their tier, else the tool
            // default), not the tool's default rate.
            let eff = self.resolve_effective_billing(user_id, &tool)?;
            if !gate.authorizes_rate(
                &eff.flat_fee,
                &eff.rate_per_min,
                eff.max_session_minutes,
                &available,
                is_member,
            ) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    // Placeholder implementations for complex training methods
    // These would need full business logic implementation

    /// Get comprehensive training overview for a tool
    pub fn get_tool_training_overview(
        &self,
        tool_id: uuid::Uuid,
        user_id: uuid::Uuid,
    ) -> Result<crate::models::ToolTrainingOverview, DatabaseError> {
        // Implementation: Get tool and training steps
        let tool = self
            .get_tool_by_id(tool_id)?
            .ok_or_else(|| DatabaseError::Diesel(diesel::result::Error::NotFound))?;

        // Get all training steps for this tool with user progress
        let steps = self.get_tool_training_steps_with_progress(tool_id, user_id)?;

        // Calculate overall progress
        let overall_progress = if steps.is_empty() {
            0.0_f32
        } else {
            let completed_steps = steps
                .iter()
                .filter(|s| {
                    s.user_progress.as_ref().map_or(false, |p| {
                        p.status == crate::models::TrainingStatus::Completed
                    })
                })
                .count();
            // Convert to percentage (0.25 becomes 25.0)
            (completed_steps as f32 / steps.len() as f32) * 100.0
        };

        // Find next step to complete - any step that's available and not completed
        let next_step = steps
            .iter()
            .find(|s| {
                // Step is available and either has no progress or progress is not completed
                s.is_available
                    && s.user_progress.as_ref().map_or(true, |p| {
                        p.status != crate::models::TrainingStatus::Completed
                    })
            })
            .map(|s| s.step.clone());

        Ok(crate::models::ToolTrainingOverview {
            tool_id,
            tool_name: tool.name,
            steps,
            overall_progress,
            // This overview field means "training complete", so it is the
            // training-only answer -- no metered gate here.
            can_access_tool: self.can_access_tool(user_id, tool_id, None)?,
            next_step,
        })
    }

    /// Get tool training steps with user progress
    pub fn get_tool_training_steps_with_progress(
        &self,
        tool_id_param: uuid::Uuid,
        user_id_param: uuid::Uuid,
    ) -> Result<Vec<crate::models::TrainingStepWithProgress>, DatabaseError> {
        use crate::schema::{training_steps, user_training_progress};

        let mut conn = self.get_connection()?;

        // Get all training steps for the tool
        let steps: Vec<crate::models::TrainingStep> = training_steps::table
            .filter(training_steps::tool_id.eq(tool_id_param))
            .order(training_steps::step_number.asc())
            .select(crate::models::TrainingStep::as_select())
            .load::<crate::models::TrainingStep>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        let mut result = Vec::new();

        for step in steps {
            // Get user's progress for this step (if any)
            let user_progress: Option<crate::models::UserTrainingProgress> =
                user_training_progress::table
                    .filter(user_training_progress::user_id.eq(user_id_param))
                    .filter(user_training_progress::training_step_id.eq(step.id))
                    .select(crate::models::UserTrainingProgress::as_select())
                    .first(&mut conn)
                    .optional()
                    .map_err(DatabaseError::Diesel)?;

            // Prerequisites concept removed - return empty vector
            let prerequisites = Vec::new();

            // All training steps are available by default (no prerequisites)
            let is_available = true;

            // Check if instructor is required for this step
            let instructor_required = step.requires_instructor();

            result.push(crate::models::TrainingStepWithProgress {
                step,
                prerequisites,
                user_progress,
                is_available,
                instructor_required,
            });
        }

        Ok(result)
    }

    /// Check if user can start a training step (no prerequisites - all steps available)
    pub fn can_start_training_step(
        &self,
        _user_id: uuid::Uuid,
        _step_id: uuid::Uuid,
    ) -> Result<bool, DatabaseError> {
        // Prerequisites concept removed - all training steps are available by default
        // Users can start any training step regardless of other completed steps
        Ok(true)
    }

    /// Start a training session
    pub fn start_training_session(
        &self,
        user_id_param: uuid::Uuid,
        request: &crate::models::StartTrainingRequest,
    ) -> Result<crate::models::UserTrainingProgress, DatabaseError> {
        use crate::schema::user_training_progress;

        let mut conn = self.get_connection()?;

        // Create or update progress record
        let new_progress = crate::models::NewUserTrainingProgress {
            user_id: user_id_param,
            training_step_id: request.training_step_id,
            status: Some(crate::models::TrainingStatus::InProgress),
            instructor_id: request.instructor_id,
            started_at: Some(chrono::Utc::now()),
            notes: request.notes.clone(),
        };

        diesel::insert_into(user_training_progress::table)
            .values(&new_progress)
            .on_conflict((
                user_training_progress::user_id,
                user_training_progress::training_step_id,
            ))
            .do_update()
            .set((
                user_training_progress::status.eq(crate::models::TrainingStatus::InProgress),
                user_training_progress::instructor_id.eq(request.instructor_id),
                user_training_progress::started_at.eq(chrono::Utc::now()),
                user_training_progress::updated_at.eq(chrono::Utc::now()),
            ))
            .returning(crate::models::UserTrainingProgress::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Complete a training session
    pub fn complete_training_session(
        &self,
        user_id_param: uuid::Uuid,
        request: &crate::models::CompleteTrainingRequest,
    ) -> Result<crate::models::UserTrainingProgress, DatabaseError> {
        use crate::schema::user_training_progress;

        let mut conn = self.get_connection()?;

        let final_status = if request.passed {
            crate::models::TrainingStatus::Completed
        } else {
            crate::models::TrainingStatus::Failed
        };

        // One load, two values derived from it: how long the certification
        // lasts, and what documentation the trainee was agreeing to at the
        // moment they agreed.
        let step = self.get_training_step_by_id(request.training_step_id)?;

        let expires_at = if request.passed {
            step.as_ref().and_then(|s| s.calculate_expiry_date())
        } else {
            None
        };

        // Snapshotted on every completion that has a document, not only on a
        // self-attestation. training_steps.training_materials_url is mutable,
        // and a trainer signing somebody off against a written procedure has
        // exactly the same problem if the procedure is edited afterwards: the
        // record would silently come to mean something nobody agreed to.
        let acknowledged_materials_url =
            step.as_ref().and_then(|s| s.training_materials_url.clone());

        let res = diesel::update(
            user_training_progress::table
                .filter(user_training_progress::user_id.eq(user_id_param))
                .filter(user_training_progress::training_step_id.eq(request.training_step_id)),
        )
        .set((
            user_training_progress::status.eq(final_status),
            user_training_progress::completed_at.eq(chrono::Utc::now()),
            user_training_progress::expires_at.eq(expires_at),
            user_training_progress::assessment_score.eq(request.assessment_score),
            user_training_progress::notes.eq(&request.notes),
            user_training_progress::acknowledged_materials_url.eq(acknowledged_materials_url),
            user_training_progress::updated_at.eq(chrono::Utc::now()),
        ))
        .returning(crate::models::UserTrainingProgress::as_returning())
        .get_result(&mut conn)
        .map_err(DatabaseError::Diesel)?;
        Ok(res)
    }
}

// ==================== TRAINER ASSIGNMENT METHODS ====================

impl DatabaseManager {
    /// Assign a user as a trainer for a specific tool

    /// Get all active trainers for a specific tool

    /// Get a specific tool trainer assignment

    /// Check if a user is an active trainer for a specific tool

    /// Get training records with optional filters

    // ── ToolGuard ────────────────────────────────────────────────────────────

    /// Look up a device by its auth token.
    /// Returns (device_id, token) if found.
    pub fn find_device_by_auth_token(
        &self,
        token: &str,
    ) -> Result<Option<(uuid::Uuid, String)>, DatabaseError> {
        use crate::schema::{space_device_auth, space_devices};

        let mut conn = self.get_connection()?;

        let result = space_device_auth::table
            .inner_join(space_devices::table)
            .filter(space_device_auth::auth_token.eq(token))
            .filter(space_devices::deleted_at.is_null())
            .select((space_device_auth::device_id, space_device_auth::auth_token))
            .first::<(uuid::Uuid, String)>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;

        Ok(result)
    }

    /// Return the IDs of all non-deleted devices (for MQTT broadcast).
    pub fn list_approved_devices(&self) -> Result<Vec<uuid::Uuid>, DatabaseError> {
        use crate::schema::space_devices::dsl::*;

        let mut conn = self.get_connection()?;

        space_devices
            .filter(deleted_at.is_null())
            .select(id)
            .load::<uuid::Uuid>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// The allow-list for **one device**.
    ///
    /// Scoped by `tool_modules` binding (#104). It used to take a device id at
    /// the API layer, echo it into the response, and return every tool and
    /// every member's card value to whoever asked -- so one compromised reader
    /// yielded the card identifier of the entire membership plus the full
    /// authorization matrix.
    ///
    /// Deny by default, with no special case for any device kind: a device
    /// receives the tools it is bound to and the users authorized for those
    /// tools, and a device bound to nothing receives nothing. An edge that
    /// coordinates twenty tools has twenty bindings. That is more rows than
    /// inferring scope from a place or a device kind, and it is an explicit
    /// grant an administrator made rather than one the topology implied.
    pub fn get_toolguard_sync_data(
        &self,
        device_id: uuid::Uuid,
        profile_field: &str,
        metered_gate: Option<&crate::tool_billing::MeteredGate>,
    ) -> Result<
        (
            Vec<crate::api::toolguard::ToolGuardSyncUser>,
            Vec<crate::api::toolguard::ToolGuardSyncTool>,
        ),
        DatabaseError,
    > {
        use crate::api::toolguard::{ToolGuardSyncTool, ToolGuardSyncUser};
        use crate::schema::{tools, users};

        let mut conn = self.get_connection()?;

        // Load all users
        let all_users = users::table
            .select(crate::models::User::as_select())
            .load::<crate::models::User>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // Only the tools this device is bound to. Everything downstream narrows
        // from here: a user's authorized list is computed against these, and the
        // top-level tool list is built from those authorizations.
        let bound: Vec<uuid::Uuid> = {
            use crate::schema::tool_modules;
            tool_modules::table
                .filter(tool_modules::device_id.eq(device_id))
                .select(tool_modules::tool_id)
                .distinct()
                .load(&mut conn)
                .map_err(DatabaseError::Diesel)?
        };
        if bound.is_empty() {
            // Loud, because the deny-by-default answer and a misconfiguration
            // look identical from the device's side: an empty allow-list. The
            // device will refuse every card and report nothing wrong.
            tracing::warn!(
                "Device {} has no tool_modules bindings, so its sync payload is \
                 empty and it will authorize nobody. Bind it to the tools it \
                 serves.",
                device_id
            );
        }
        let all_tools = tools::table
            .filter(tools::id.eq_any(&bound))
            .select(crate::models::Tool::as_select())
            .load::<crate::models::Tool>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // First-class active cards, grouped by user, loaded once. These are
        // unioned below with any legacy profile-field identifier(s) so both
        // schemes work during and after migration. Disabled/released cards are
        // excluded, so a revoked card never reaches an edge allow-list.
        let active_cards = user_cards::table
            .filter(user_cards::status.eq(CardStatus::Active))
            .select((user_cards::user_id, user_cards::code_wire_digest))
            .load::<(uuid::Uuid, Option<Vec<u8>>)>(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        // What a device will hold, resolved here rather than at the payload.
        //
        // The stored wire digest is the only thing a device matches on, and
        // with `user_cards.code` dropped (#108) it is the only thing left: a
        // card is sealed at issue and the drop migration refuses to run while
        // any row is unsealed, so every active card carries a digest here.
        // Falling back to the plaintext *code* was the disclosure this change
        // exists to end, and it is gone.
        let mut cards_by_user: std::collections::HashMap<uuid::Uuid, Vec<Vec<u8>>> =
            std::collections::HashMap::new();
        for (uid, stored) in active_cards {
            if let Some(d) = stored {
                cards_by_user.entry(uid).or_default().push(d);
            }
        }

        // #34: preload every (user, tool) tier rate once, so the per-tool
        // affordability filter below resolves the member's rate without an N+1.
        #[allow(clippy::type_complexity)]
        let tier_rates: std::collections::HashMap<
            (uuid::Uuid, uuid::Uuid),
            (
                Option<bigdecimal::BigDecimal>,
                Option<bigdecimal::BigDecimal>,
                Option<i32>,
            ),
        > = self
            .all_tier_rates()?
            .into_iter()
            .map(|(u, t, f, r, m)| ((u, t), (f, r, m)))
            .collect();

        let mut sync_users = Vec::new();
        // Track which tools appear in at least one user's authorized list
        let mut authorized_tool_ids_set: std::collections::HashSet<uuid::Uuid> =
            std::collections::HashSet::new();

        // Membership ("member.access") equals effective tier level >= active.
        // Precomputed once for the whole roster to avoid a per-user query.
        let member_levels =
            self.effective_levels_for(&all_users.iter().map(|u| u.id).collect::<Vec<_>>())?;
        let active_level = self
            .rbac()
            .level_of_name(crate::models::role::ACTIVE)
            .unwrap_or(i16::MAX);

        for user in &all_users {
            // The profile field may be either a scalar string (one identifier)
            // or an array of strings (many identifiers per user). Empty/missing
            // values are skipped.
            let mut identifiers: Vec<Vec<u8>> = Vec::new();
            let profile_values: Vec<String> = match user.profile.get(profile_field) {
                Some(v) if v.is_string() => v
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .map(|s| vec![s.to_string()])
                    .unwrap_or_default(),
                Some(v) if v.is_array() => v
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|e| e.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            // Legacy profile-field identifiers have no row to have been
            // backfilled, so they are digested here. There are few of them once
            // #33's first-class cards are in use, which is what keeps the slow
            // KDF off the hot path for the bulk of the membership.
            if let Some(c) = cipher {
                for v in &profile_values {
                    if let Ok(d) = c.wire_digest(v) {
                        if !identifiers.contains(&d) {
                            identifiers.push(d);
                        }
                    }
                }
            }

            // Union in the member's first-class active cards (deduped).
            if let Some(digests) = cards_by_user.get(&user.id) {
                for d in digests {
                    if !identifiers.contains(d) {
                        identifiers.push(d.clone());
                    }
                }
            }

            if identifiers.is_empty() {
                continue;
            }

            // Authorization is per-user, so compute it once and reuse across
            // every identifier this user exposes.
            //
            // Training is the first gate (unchanged). When tool billing is on, a
            // metered tool the member cannot currently afford (or, if required,
            // is not a member for) is additionally dropped here -- the per-user
            // affordability filter that makes edge-local actuation safe. Money is
            // never charged here; this only shapes the offered list. Available
            // balance is read once per user (not per tool).
            let metered_ctx = metered_gate.map(|g| {
                let available = self
                    .available_balance(user.id)
                    .unwrap_or_else(|_| bigdecimal::BigDecimal::from(0));
                let is_member = member_levels.get(&user.id).copied().unwrap_or(0) >= active_level;
                (g, available, is_member)
            });
            let mut authorized_tool_ids = Vec::new();
            for tool in &all_tools {
                let mut authorized =
                    self.user_is_authorized_for_tool(user.id, tool.id, tool.requires_training)?;

                if authorized {
                    if let Some((gate, available, is_member)) = &metered_ctx {
                        // #34: resolve this member's rate (their tier, else the
                        // tool default) from the preloaded map.
                        let (eflat, erate, emax) = match tier_rates.get(&(user.id, tool.id)) {
                            Some((f, r, m)) => {
                                (f.clone(), r.clone(), m.or(tool.usage_max_session_minutes))
                            }
                            None => (
                                tool.usage_flat_fee.clone(),
                                tool.usage_rate_per_min.clone(),
                                tool.usage_max_session_minutes,
                            ),
                        };
                        authorized =
                            gate.authorizes_rate(&eflat, &erate, emax, available, *is_member);
                    }
                }

                if authorized {
                    authorized_tool_ids.push(tool.id);
                    authorized_tool_ids_set.insert(tool.id);
                }
            }

            // A member authorized for nothing this device serves has no reason to
            // be in its payload, and their card value is exactly what must not
            // be there. Dropping them is the difference between scoping the
            // tools and scoping the exposure.
            if authorized_tool_ids.is_empty() {
                continue;
            }

            for digest in identifiers {
                sync_users.push(ToolGuardSyncUser {
                    profile_field_digest: hex::encode(digest),
                    full_name: user.full_name.clone(),
                    is_active: user.is_active,
                    authorized_tool_ids: authorized_tool_ids.clone(),
                });
            }
        }

        // Build the top-level tools list from all tools that appear in any user's authorized set
        let sync_tools = all_tools
            .iter()
            .filter(|t| authorized_tool_ids_set.contains(&t.id))
            .map(|t| ToolGuardSyncTool {
                id: t.id,
                external_id: t.external_id.clone(),
                name: t.name.clone(),
                status: t.status.clone(),
                requires_online: metered_gate.map(|g| g.requires_online(t)).unwrap_or(false),
            })
            .collect();

        Ok((sync_users, sync_tools))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_password() {
        let url = "postgresql://user:password@localhost:5432/database";
        let masked = mask_password(url);
        assert!(masked.contains("***"));
        assert!(!masked.contains("password"));
    }

    #[test]
    fn test_mask_password_no_password() {
        let url = "postgresql://user@localhost:5432/database";
        let masked = mask_password(url);
        assert_eq!(url, masked);
    }

    #[test]
    fn test_database_config_url_construction() {
        let config = DatabaseConfig {
            host: "localhost".to_string(),
            port: 5432,
            database: "test_db".to_string(),
            username: "test_user".to_string(),
            password: "test_pass".to_string(),
            url: None,
            ..Default::default()
        };

        let url = config.get_url();
        assert_eq!(
            url,
            "postgresql://test_user:test_pass@localhost:5432/test_db"
        );
    }

    #[test]
    fn test_database_config_explicit_url() {
        let explicit_url = "postgresql://custom:url@example.com:1234/custom_db";
        let config = DatabaseConfig {
            url: Some(explicit_url.to_string()),
            ..Default::default()
        };

        let url = config.get_url();
        assert_eq!(url, explicit_url);
    }
}

// ---------------------------------------------------------------------------
// Webhook system
// ---------------------------------------------------------------------------

impl DatabaseManager {
    // --- Auth headers (reusable, write-only credentials) ------------------

    pub fn create_webhook_auth_header(
        &self,
        new_header: &crate::models::NewWebhookAuthHeader,
    ) -> Result<crate::models::WebhookAuthHeader, DatabaseError> {
        use crate::schema::webhook_auth_headers;
        let mut conn = self.get_connection()?;
        diesel::insert_into(webhook_auth_headers::table)
            .values(new_header)
            .returning(crate::models::WebhookAuthHeader::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_webhook_auth_headers(
        &self,
    ) -> Result<Vec<crate::models::WebhookAuthHeader>, DatabaseError> {
        use crate::schema::webhook_auth_headers::dsl::*;
        let mut conn = self.get_connection()?;
        webhook_auth_headers
            .order(created_at.desc())
            .select(crate::models::WebhookAuthHeader::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_webhook_auth_header(
        &self,
        header_id: uuid::Uuid,
    ) -> Result<crate::models::WebhookAuthHeader, DatabaseError> {
        use crate::schema::webhook_auth_headers::dsl::*;
        let mut conn = self.get_connection()?;
        webhook_auth_headers
            .find(header_id)
            .select(crate::models::WebhookAuthHeader::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_webhook_auth_header(
        &self,
        header_id: uuid::Uuid,
        changes: &crate::models::UpdateWebhookAuthHeader,
    ) -> Result<crate::models::WebhookAuthHeader, DatabaseError> {
        use crate::schema::webhook_auth_headers::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(webhook_auth_headers.find(header_id))
            .set(changes)
            .returning(crate::models::WebhookAuthHeader::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_webhook_auth_header(
        &self,
        header_id: uuid::Uuid,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::webhook_auth_headers::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(webhook_auth_headers.find(header_id))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Number of webhooks currently referencing the given auth header.
    pub fn count_webhook_auth_header_usage(
        &self,
        header_id: uuid::Uuid,
    ) -> Result<i64, DatabaseError> {
        use crate::schema::webhook_auth_links::dsl::*;
        let mut conn = self.get_connection()?;
        webhook_auth_links
            .filter(auth_header_id.eq(header_id))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // --- Webhooks ---------------------------------------------------------

    /// Create a webhook together with its event subscriptions and auth links,
    /// in a single transaction.
    pub fn create_webhook(
        &self,
        new_webhook: &crate::models::NewWebhook,
        event_types: &[String],
        auth_header_ids: &[uuid::Uuid],
    ) -> Result<crate::models::Webhook, DatabaseError> {
        use crate::schema::{webhook_auth_links, webhook_event_subscriptions, webhooks};
        let mut conn = self.get_connection()?;

        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            let webhook: crate::models::Webhook = diesel::insert_into(webhooks::table)
                .values(new_webhook)
                .returning(crate::models::Webhook::as_returning())
                .get_result(conn)?;

            let subs: Vec<crate::models::NewWebhookEventSubscription> = event_types
                .iter()
                .map(|et| crate::models::NewWebhookEventSubscription {
                    webhook_id: webhook.id,
                    event_type: et.clone(),
                })
                .collect();
            if !subs.is_empty() {
                diesel::insert_into(webhook_event_subscriptions::table)
                    .values(&subs)
                    .execute(conn)?;
            }

            let links: Vec<crate::models::NewWebhookAuthLink> = auth_header_ids
                .iter()
                .map(|hid| crate::models::NewWebhookAuthLink {
                    webhook_id: webhook.id,
                    auth_header_id: *hid,
                })
                .collect();
            if !links.is_empty() {
                diesel::insert_into(webhook_auth_links::table)
                    .values(&links)
                    .execute(conn)?;
            }

            Ok(webhook)
        })
        .map_err(DatabaseError::Diesel)
    }

    pub fn list_webhooks(&self) -> Result<Vec<crate::models::Webhook>, DatabaseError> {
        use crate::schema::webhooks::dsl::*;
        let mut conn = self.get_connection()?;
        webhooks
            .order(created_at.desc())
            .select(crate::models::Webhook::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_webhook(
        &self,
        webhook_id: uuid::Uuid,
    ) -> Result<crate::models::Webhook, DatabaseError> {
        use crate::schema::webhooks::dsl::*;
        let mut conn = self.get_connection()?;
        webhooks
            .find(webhook_id)
            .select(crate::models::Webhook::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update webhook fields. When `event_types` / `auth_header_ids` are
    /// `Some`, the corresponding set is replaced wholesale.
    pub fn update_webhook(
        &self,
        webhook_id: uuid::Uuid,
        changes: &crate::models::UpdateWebhook,
        event_types: Option<&[String]>,
        auth_header_ids: Option<&[uuid::Uuid]>,
    ) -> Result<crate::models::Webhook, DatabaseError> {
        use crate::schema::{webhook_auth_links, webhook_event_subscriptions, webhooks};
        let mut conn = self.get_connection()?;

        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            let webhook: crate::models::Webhook = diesel::update(webhooks::table.find(webhook_id))
                .set(changes)
                .returning(crate::models::Webhook::as_returning())
                .get_result(conn)?;

            if let Some(events) = event_types {
                diesel::delete(
                    webhook_event_subscriptions::table
                        .filter(webhook_event_subscriptions::webhook_id.eq(webhook_id)),
                )
                .execute(conn)?;
                let subs: Vec<crate::models::NewWebhookEventSubscription> = events
                    .iter()
                    .map(|et| crate::models::NewWebhookEventSubscription {
                        webhook_id,
                        event_type: et.clone(),
                    })
                    .collect();
                if !subs.is_empty() {
                    diesel::insert_into(webhook_event_subscriptions::table)
                        .values(&subs)
                        .execute(conn)?;
                }
            }

            if let Some(ids) = auth_header_ids {
                diesel::delete(
                    webhook_auth_links::table.filter(webhook_auth_links::webhook_id.eq(webhook_id)),
                )
                .execute(conn)?;
                let links: Vec<crate::models::NewWebhookAuthLink> = ids
                    .iter()
                    .map(|hid| crate::models::NewWebhookAuthLink {
                        webhook_id,
                        auth_header_id: *hid,
                    })
                    .collect();
                if !links.is_empty() {
                    diesel::insert_into(webhook_auth_links::table)
                        .values(&links)
                        .execute(conn)?;
                }
            }

            Ok(webhook)
        })
        .map_err(DatabaseError::Diesel)
    }

    pub fn delete_webhook(&self, webhook_id: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::webhooks::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(webhooks.find(webhook_id))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Event types a webhook is subscribed to.
    pub fn get_webhook_event_types(
        &self,
        webhook_id_arg: uuid::Uuid,
    ) -> Result<Vec<String>, DatabaseError> {
        use crate::schema::webhook_event_subscriptions::dsl::*;
        let mut conn = self.get_connection()?;
        webhook_event_subscriptions
            .filter(webhook_id.eq(webhook_id_arg))
            .select(event_type)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// IDs of auth headers linked to a webhook.
    pub fn get_webhook_auth_header_ids(
        &self,
        webhook_id_arg: uuid::Uuid,
    ) -> Result<Vec<uuid::Uuid>, DatabaseError> {
        use crate::schema::webhook_auth_links::dsl::*;
        let mut conn = self.get_connection()?;
        webhook_auth_links
            .filter(webhook_id.eq(webhook_id_arg))
            .select(auth_header_id)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Full auth header rows (incl. secret values) for a webhook — used only
    /// by the dispatcher when building an outgoing request.
    pub fn get_webhook_auth_headers_for_dispatch(
        &self,
        webhook_id_arg: uuid::Uuid,
    ) -> Result<Vec<crate::models::WebhookAuthHeader>, DatabaseError> {
        use crate::schema::{webhook_auth_headers, webhook_auth_links};
        let mut conn = self.get_connection()?;
        webhook_auth_links::table
            .inner_join(
                webhook_auth_headers::table
                    .on(webhook_auth_links::auth_header_id.eq(webhook_auth_headers::id)),
            )
            .filter(webhook_auth_links::webhook_id.eq(webhook_id_arg))
            .select(crate::models::WebhookAuthHeader::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Enabled webhooks subscribed to a given event type.
    pub fn get_enabled_webhooks_for_event(
        &self,
        event_type_arg: &str,
    ) -> Result<Vec<crate::models::Webhook>, DatabaseError> {
        use crate::schema::{webhook_event_subscriptions, webhooks};
        let mut conn = self.get_connection()?;
        webhooks::table
            .inner_join(
                webhook_event_subscriptions::table
                    .on(webhook_event_subscriptions::webhook_id.eq(webhooks::id)),
            )
            .filter(webhooks::enabled.eq(true))
            .filter(webhook_event_subscriptions::event_type.eq(event_type_arg))
            .select(crate::models::Webhook::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // --- Deliveries -------------------------------------------------------

    pub fn record_webhook_delivery(
        &self,
        delivery: &crate::models::NewWebhookDelivery,
    ) -> Result<crate::models::WebhookDelivery, DatabaseError> {
        use crate::schema::webhook_deliveries;
        let mut conn = self.get_connection()?;
        diesel::insert_into(webhook_deliveries::table)
            .values(delivery)
            .returning(crate::models::WebhookDelivery::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_webhook_deliveries(
        &self,
        webhook_id_filter: Option<uuid::Uuid>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<crate::models::WebhookDelivery>, DatabaseError> {
        use crate::schema::webhook_deliveries::dsl::*;
        let mut conn = self.get_connection()?;
        let mut query = webhook_deliveries.order(created_at.desc()).into_boxed();
        if let Some(wid) = webhook_id_filter {
            query = query.filter(webhook_id.eq(wid));
        }
        query
            .limit(limit)
            .offset(offset)
            .select(crate::models::WebhookDelivery::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }
}

// ---------------------------------------------------------------------------
// MFA system
// ---------------------------------------------------------------------------

impl DatabaseManager {
    // --- TOTP ------------------------------------------------------------

    pub fn get_user_totp(
        &self,
        uid: uuid::Uuid,
    ) -> Result<Option<crate::models::UserMfaTotp>, DatabaseError> {
        use crate::schema::user_mfa_totp::dsl::*;
        let mut conn = self.get_connection()?;
        user_mfa_totp
            .filter(user_id.eq(uid))
            .select(crate::models::UserMfaTotp::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Replace any existing TOTP row for the user with a new unconfirmed one.
    /// Returns the freshly-inserted row.
    pub fn replace_user_totp_unconfirmed(
        &self,
        uid: uuid::Uuid,
        new_secret_base32: &str,
    ) -> Result<crate::models::UserMfaTotp, DatabaseError> {
        use crate::schema::user_mfa_totp::dsl::*;
        let mut conn = self.get_connection()?;
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::delete(user_mfa_totp.filter(user_id.eq(uid))).execute(conn)?;
            let new_row = crate::models::NewUserMfaTotp {
                user_id: uid,
                secret_base32: new_secret_base32.to_string(),
            };
            diesel::insert_into(user_mfa_totp)
                .values(&new_row)
                .returning(crate::models::UserMfaTotp::as_returning())
                .get_result(conn)
        })
        .map_err(DatabaseError::Diesel)
    }

    pub fn confirm_user_totp(&self, uid: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::user_mfa_totp::dsl::*;
        let mut conn = self.get_connection()?;
        // The row count is the answer, not a detail to discard.
        // Enrolment was reported confirmed whether or not a row moved.
        let affected = diesel::update(user_mfa_totp.filter(user_id.eq(uid)))
            .set((
                confirmed_at.eq(Some(chrono::Utc::now())),
                updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    pub fn delete_user_totp(&self, uid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::user_mfa_totp::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(user_mfa_totp.filter(user_id.eq(uid)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // --- WebAuthn --------------------------------------------------------

    pub fn list_user_webauthn(
        &self,
        uid: uuid::Uuid,
    ) -> Result<Vec<crate::models::UserMfaWebauthn>, DatabaseError> {
        use crate::schema::user_mfa_webauthn::dsl::*;
        let mut conn = self.get_connection()?;
        user_mfa_webauthn
            .filter(user_id.eq(uid))
            .order(created_at.desc())
            .select(crate::models::UserMfaWebauthn::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn insert_user_webauthn(
        &self,
        new_row: &crate::models::NewUserMfaWebauthn,
    ) -> Result<crate::models::UserMfaWebauthn, DatabaseError> {
        use crate::schema::user_mfa_webauthn;
        let mut conn = self.get_connection()?;
        diesel::insert_into(user_mfa_webauthn::table)
            .values(new_row)
            .returning(crate::models::UserMfaWebauthn::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Delete one of a user's WebAuthn credentials by row id.
    pub fn delete_user_webauthn(
        &self,
        uid: uuid::Uuid,
        row_id: uuid::Uuid,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::user_mfa_webauthn::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(
            user_mfa_webauthn
                .filter(id.eq(row_id))
                .filter(user_id.eq(uid)),
        )
        .execute(&mut conn)
        .map_err(DatabaseError::Diesel)
    }

    pub fn touch_user_webauthn_last_used(
        &self,
        uid: uuid::Uuid,
        cred_id_bytes: &[u8],
    ) -> Result<(), DatabaseError> {
        use crate::schema::user_mfa_webauthn::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(
            user_mfa_webauthn
                .filter(user_id.eq(uid))
                .filter(credential_id.eq(cred_id_bytes)),
        )
        .set(last_used_at.eq(Some(chrono::Utc::now())))
        .execute(&mut conn)
        .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    // --- Recovery codes --------------------------------------------------

    pub fn replace_user_recovery_codes(
        &self,
        uid: uuid::Uuid,
        hashes: Vec<String>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::user_mfa_recovery_codes::dsl::*;
        let mut conn = self.get_connection()?;
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::delete(user_mfa_recovery_codes.filter(user_id.eq(uid))).execute(conn)?;
            let rows: Vec<crate::models::NewUserMfaRecoveryCode> = hashes
                .into_iter()
                .map(|h| crate::models::NewUserMfaRecoveryCode {
                    user_id: uid,
                    code_hash: h,
                })
                .collect();
            if !rows.is_empty() {
                diesel::insert_into(user_mfa_recovery_codes)
                    .values(&rows)
                    .execute(conn)?;
            }
            Ok(())
        })
        .map_err(DatabaseError::Diesel)
    }

    /// Unused recovery codes for the user. Caller hashes-and-compares.
    pub fn list_unused_recovery_codes(
        &self,
        uid: uuid::Uuid,
    ) -> Result<Vec<crate::models::UserMfaRecoveryCode>, DatabaseError> {
        use crate::schema::user_mfa_recovery_codes::dsl::*;
        let mut conn = self.get_connection()?;
        user_mfa_recovery_codes
            .filter(user_id.eq(uid))
            .filter(used_at.is_null())
            .select(crate::models::UserMfaRecoveryCode::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn mark_recovery_code_used(&self, code_id: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::user_mfa_recovery_codes::dsl::*;
        let mut conn = self.get_connection()?;
        // The row count is the answer, not a detail to discard.
        // A recovery code is single-use. Matching zero rows means it was NOT consumed
        // while the caller went on to treat the login as authenticated, so the
        // code stays usable.
        let affected = diesel::update(user_mfa_recovery_codes.filter(id.eq(code_id)))
            .set(used_at.eq(Some(chrono::Utc::now())))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    pub fn count_unused_recovery_codes(&self, uid: uuid::Uuid) -> Result<i64, DatabaseError> {
        use crate::schema::user_mfa_recovery_codes::dsl::*;
        let mut conn = self.get_connection()?;
        user_mfa_recovery_codes
            .filter(user_id.eq(uid))
            .filter(used_at.is_null())
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // --- account tokens: password reset and email confirmation -----------
    //
    // Both flows keep the same shape, and both consume their token with a
    // single filtered UPDATE rather than a read followed by a write. The
    // reasoning is the one recorded at api/devices.rs:229-247, where a
    // check-then-claim let one device invite mint two devices: between the
    // SELECT that finds the row unused and the UPDATE that spends it, another
    // request can do the same. Here the consequence is worse than a duplicate
    // device -- two concurrent requests could each set a different password
    // from one emailed link, and only one of the two people would know which
    // won.
    //
    // The expiry check is inside the statement for the same reason. A
    // preceding `if row.expires_at > now` is a race with the clock as well as
    // with other writers.

    /// Store a reset token, invalidating any the user already had.
    ///
    /// Old tokens are spent rather than deleted, so a reset that was requested
    /// twice leaves a trail of both. Invalidating them keeps at most one live
    /// credential per account, and makes "I clicked the older email" fail
    /// cleanly instead of succeeding confusingly.
    pub fn create_password_reset_token(
        &self,
        uid: uuid::Uuid,
        hash: String,
        expiry: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::password_reset_tokens::dsl::*;
        let mut conn = self.get_connection()?;
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::update(
                password_reset_tokens
                    .filter(user_id.eq(uid))
                    .filter(used_at.is_null()),
            )
            .set(used_at.eq(Some(chrono::Utc::now())))
            .execute(conn)?;

            diesel::insert_into(password_reset_tokens)
                .values(crate::models::NewPasswordResetToken {
                    user_id: uid,
                    token_hash: hash,
                    expires_at: expiry,
                })
                .execute(conn)?;
            Ok(())
        })
        .map_err(DatabaseError::Diesel)
    }

    /// Spend a reset token, returning the user it belonged to.
    ///
    /// `Ok(None)` covers unknown, expired and already-spent alike -- the caller
    /// must not distinguish them for the requester, and there is nothing useful
    /// it could do differently.
    pub fn claim_password_reset_token(
        &self,
        hash: &str,
    ) -> Result<Option<uuid::Uuid>, DatabaseError> {
        use crate::schema::password_reset_tokens::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(
            password_reset_tokens
                .filter(token_hash.eq(hash))
                .filter(used_at.is_null())
                .filter(expires_at.gt(chrono::Utc::now())),
        )
        .set(used_at.eq(Some(chrono::Utc::now())))
        .returning(user_id)
        .get_result::<uuid::Uuid>(&mut conn)
        .optional()
        .map_err(DatabaseError::Diesel)
    }

    /// Store an email confirmation token, invalidating any the user already had.
    pub fn create_email_verification_token(
        &self,
        uid: uuid::Uuid,
        hash: String,
        expiry: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::email_verification_tokens::dsl::*;
        let mut conn = self.get_connection()?;
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::update(
                email_verification_tokens
                    .filter(user_id.eq(uid))
                    .filter(used_at.is_null()),
            )
            .set(used_at.eq(Some(chrono::Utc::now())))
            .execute(conn)?;

            diesel::insert_into(email_verification_tokens)
                .values(crate::models::NewEmailVerificationToken {
                    user_id: uid,
                    token_hash: hash,
                    expires_at: expiry,
                })
                .execute(conn)?;
            Ok(())
        })
        .map_err(DatabaseError::Diesel)
    }

    /// Spend an email confirmation token, returning the user it belonged to.
    pub fn claim_email_verification_token(
        &self,
        hash: &str,
    ) -> Result<Option<uuid::Uuid>, DatabaseError> {
        use crate::schema::email_verification_tokens::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(
            email_verification_tokens
                .filter(token_hash.eq(hash))
                .filter(used_at.is_null())
                .filter(expires_at.gt(chrono::Utc::now())),
        )
        .set(used_at.eq(Some(chrono::Utc::now())))
        .returning(user_id)
        .get_result::<uuid::Uuid>(&mut conn)
        .optional()
        .map_err(DatabaseError::Diesel)
    }

    /// Record that a user's address has been confirmed.
    ///
    /// Idempotent by construction: confirming twice is not an error, and the
    /// second call simply refreshes the timestamp.
    pub fn mark_email_verified(&self, uid: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        let affected = diesel::update(users.filter(id.eq(uid)))
            .set(email_verified_at.eq(Some(chrono::Utc::now())))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            // Reported rather than discarded, per
            // checks/tests/writes_report_what_they_changed.rs: a confirmation
            // that updated nothing means the account went away between claiming
            // the token and writing the result, and silently returning Ok would
            // leave the address unconfirmed with the token already spent.
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }
        Ok(())
    }

    /// Set (or clear) a member's mailing-list opt-out timestamp.
    ///
    /// `Some(ts)` records an explicit opt-out; `None` clears it back to
    /// subscribed-by-default. A dedicated setter rather than a field on
    /// `UpdateUser` because the column must be settable to NULL, which the
    /// skip-on-None changeset cannot express. Reports NotFound when it changed
    /// nothing, per writes_report_what_they_changed.
    pub fn set_mailing_list_opt_out(
        &self,
        uid: uuid::Uuid,
        opt_out_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        let affected = diesel::update(users.filter(id.eq(uid)))
            .set((
                mailing_list_opt_out_at.eq(opt_out_at),
                updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }
        Ok(())
    }

    /// Emails of every member who should be on the mailing list: active, with a
    /// verified address, and not opted out. This is the reconciler's notion of
    /// "intended subscribed" -- the set Groups.io is made to mirror.
    pub fn list_mailing_list_intended(&self) -> Result<Vec<String>, DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        users
            .filter(is_active.eq(true))
            .filter(email_verified_at.is_not_null())
            .filter(mailing_list_opt_out_at.is_null())
            .select(email)
            .load::<String>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Record one Groups.io reconciliation pass.
    pub fn record_groupsio_sync_run(
        &self,
        run: &crate::models::NewGroupsioSyncRun,
    ) -> Result<crate::models::GroupsioSyncRun, DatabaseError> {
        use crate::schema::groupsio_sync_runs;
        let mut conn = self.get_connection()?;
        diesel::insert_into(groupsio_sync_runs::table)
            .values(run)
            .returning(crate::models::GroupsioSyncRun::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// The most recent reconciliation passes, newest first.
    pub fn latest_groupsio_sync_runs(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::models::GroupsioSyncRun>, DatabaseError> {
        use crate::schema::groupsio_sync_runs::dsl::*;
        let mut conn = self.get_connection()?;
        groupsio_sync_runs
            .order(started_at.desc())
            .limit(limit)
            .select(crate::models::GroupsioSyncRun::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // --- membership dues ledger ------------------------------------------

    /// Insert one ledger entry.
    pub fn insert_ledger_entry(
        &self,
        entry: &crate::models::NewMembershipLedgerEntry,
    ) -> Result<crate::models::MembershipLedgerEntry, DatabaseError> {
        use crate::schema::membership_ledger;
        let mut conn = self.get_connection()?;
        diesel::insert_into(membership_ledger::table)
            .values(entry)
            .returning(crate::models::MembershipLedgerEntry::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Whether a ledger entry already exists for a Stripe reference id. The
    /// second oracle for webhook idempotency, alongside the partial unique index:
    /// callers check this before inserting so a redelivered event is a no-op
    /// rather than a unique-violation error to swallow.
    pub fn ledger_entry_exists_for_reference(
        &self,
        reference: &str,
    ) -> Result<bool, DatabaseError> {
        use crate::schema::membership_ledger::dsl::*;
        let mut conn = self.get_connection()?;
        let n: i64 = membership_ledger
            .filter(external_reference.eq(reference))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(n > 0)
    }

    /// A member's current balance: the sum of their ledger amounts, or zero if
    /// they have none. The single source of truth for entitlement -- there is no
    /// cached balance column to drift from this.
    ///
    /// Summed in Rust rather than with `diesel::dsl::sum`: that name is an
    /// ambiguous glob re-export in diesel 2.3 (a `sum` type alias and a `sum`
    /// function both land in `dsl`), which is a hard error under
    /// `deny(future_incompatible)`. A member's ledger is small, so loading the
    /// amounts and folding them is cheap and unambiguous.
    pub fn user_balance(&self, uid: uuid::Uuid) -> Result<bigdecimal::BigDecimal, DatabaseError> {
        use crate::schema::membership_ledger::dsl::*;
        let mut conn = self.get_connection()?;
        let amounts: Vec<bigdecimal::BigDecimal> = membership_ledger
            .filter(user_id.eq(uid))
            .select(amount)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(amounts
            .into_iter()
            .fold(bigdecimal::BigDecimal::from(0), |acc, a| acc + a))
    }

    /// A member's ledger, newest first, for the admin drill-down.
    pub fn list_user_ledger(
        &self,
        uid: uuid::Uuid,
        limit: i64,
    ) -> Result<Vec<crate::models::MembershipLedgerEntry>, DatabaseError> {
        use crate::schema::membership_ledger::dsl::*;
        let mut conn = self.get_connection()?;
        membership_ledger
            .filter(user_id.eq(uid))
            .order(occurred_at.desc())
            .limit(limit)
            .select(crate::models::MembershipLedgerEntry::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// How many active users have administrator access. The last-admin guard
    /// reads this before a billing-driven demotion: an admin whose lapse would
    /// take the count to zero is never demoted.
    ///
    /// "Admin" is no longer the `users.role == Admin` enum value but *effective*
    /// authority: an active user any of whose assigned roles grants `admin.access`
    /// through inheritance. Resolving that is the resolver's job, so this loads
    /// the active users' role assignments in one join and expands them against
    /// the cached graph. The join runs in SQL; the inheritance closure (which SQL
    /// cannot express without a recursive CTE) runs in Rust over a small set.
    pub fn count_active_admins(&self) -> Result<i64, DatabaseError> {
        use crate::schema::{user_roles, users};
        let mut conn = self.get_connection()?;
        // Two single-table reads rather than a join (the RBAC tables are not
        // registered for cross-table queries): the active user ids, and every
        // role assignment, grouped and expanded in Rust over the cached graph.
        let active: std::collections::HashSet<uuid::Uuid> = users::table
            .filter(users::is_active.eq(true))
            .select(users::id)
            .load::<uuid::Uuid>(&mut conn)
            .map_err(DatabaseError::Diesel)?
            .into_iter()
            .collect();
        let assignments: Vec<(uuid::Uuid, uuid::Uuid)> = user_roles::table
            .select((user_roles::user_id, user_roles::role_id))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        let mut by_user: std::collections::HashMap<uuid::Uuid, Vec<uuid::Uuid>> =
            std::collections::HashMap::new();
        for (uid, role_id) in assignments {
            if active.contains(&uid) {
                by_user.entry(uid).or_default().push(role_id);
            }
        }

        let graph = self.rbac();
        Ok(by_user
            .values()
            .filter(|role_ids| graph.has_permission(role_ids, "admin.access"))
            .count() as i64)
    }

    /// Every enrolled user (has a membership clock), for the renewal cycle.
    pub fn enrolled_users(&self) -> Result<Vec<User>, DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        users
            .filter(membership_next_due_at.is_not_null())
            .select(User::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Every user linked to a Stripe customer, for the invoice-poll backbone
    /// (which must re-credit even a lapsed, un-enrolled member whose renewal
    /// webhook was missed -- so this is not restricted to enrolled users).
    pub fn users_with_stripe_customer(&self) -> Result<Vec<User>, DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        users
            .filter(stripe_customer_id.is_not_null())
            .select(User::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Map a Stripe customer id back to the platform user (webhook handling).
    pub fn find_user_by_stripe_customer_id(
        &self,
        customer_id: &str,
    ) -> Result<Option<User>, DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        users
            .filter(stripe_customer_id.eq(customer_id))
            .select(User::as_select())
            .first::<User>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Set (or clear, with `None`) a member's next-due anniversary. Clearing it
    /// un-enrolls them: a lapse ends the membership rather than carrying a debt.
    pub fn set_membership_next_due(
        &self,
        uid: uuid::Uuid,
        next_due: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        let affected = diesel::update(users.filter(id.eq(uid)))
            .set((
                membership_next_due_at.eq(next_due),
                updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }
        Ok(())
    }

    /// Record (or clear) the Stripe customer id for a user.
    pub fn set_stripe_customer_id(
        &self,
        uid: uuid::Uuid,
        customer_id: Option<&str>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        let affected = diesel::update(users.filter(id.eq(uid)))
            .set((
                stripe_customer_id.eq(customer_id),
                updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }
        Ok(())
    }

    /// Record the user's Stripe subscription id and last-seen status together.
    /// Both are set each call (either may be `None`), so a cancellation clears
    /// the id while stamping the status.
    pub fn set_stripe_subscription(
        &self,
        uid: uuid::Uuid,
        subscription_id: Option<&str>,
        status: Option<&str>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        let affected = diesel::update(users.filter(id.eq(uid)))
            .set((
                stripe_subscription_id.eq(subscription_id),
                subscription_status.eq(status),
                updated_at.eq(chrono::Utc::now().naive_utc()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }
        Ok(())
    }

    /// Record one membership renewal-cycle pass.
    pub fn record_membership_sync_run(
        &self,
        run: &crate::models::NewMembershipSyncRun,
    ) -> Result<crate::models::MembershipSyncRun, DatabaseError> {
        use crate::schema::membership_sync_runs;
        let mut conn = self.get_connection()?;
        diesel::insert_into(membership_sync_runs::table)
            .values(run)
            .returning(crate::models::MembershipSyncRun::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// The most recent renewal-cycle passes, newest first.
    pub fn latest_membership_sync_runs(
        &self,
        limit: i64,
    ) -> Result<Vec<crate::models::MembershipSyncRun>, DatabaseError> {
        use crate::schema::membership_sync_runs::dsl::*;
        let mut conn = self.get_connection()?;
        membership_sync_runs
            .order(started_at.desc())
            .limit(limit)
            .select(crate::models::MembershipSyncRun::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // --- metered tool-billing sessions -----------------------------------

    /// Sum of the still-open holds for a user (summed in Rust to dodge the
    /// `diesel::dsl::sum` glob ambiguity, as `user_balance` does).
    pub fn sum_open_tool_holds(
        &self,
        uid: uuid::Uuid,
    ) -> Result<bigdecimal::BigDecimal, DatabaseError> {
        use crate::schema::tool_usage_sessions::dsl::*;
        let mut conn = self.get_connection()?;
        let holds: Vec<bigdecimal::BigDecimal> = tool_usage_sessions
            .filter(user_id.eq(uid))
            .filter(status.eq("open"))
            .select(hold_amount)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(holds
            .into_iter()
            .fold(bigdecimal::BigDecimal::from(0), |acc, h| acc + h))
    }

    /// A member's available balance: ledger balance minus open holds. This is
    /// what the metered-tool gate reads, so concurrent sessions cannot
    /// double-spend the same funds.
    pub fn available_balance(
        &self,
        uid: uuid::Uuid,
    ) -> Result<bigdecimal::BigDecimal, DatabaseError> {
        Ok(self.user_balance(uid)? - self.sum_open_tool_holds(uid)?)
    }

    /// The open session for a tool, if any (at most one, per the partial unique
    /// index). Used to correlate a usage report to its session and to settle.
    pub fn open_tool_session_for_tool(
        &self,
        tid: uuid::Uuid,
    ) -> Result<Option<crate::models::ToolUsageSession>, DatabaseError> {
        use crate::schema::tool_usage_sessions::dsl::*;
        let mut conn = self.get_connection()?;
        tool_usage_sessions
            .filter(tool_id.eq(tid))
            .filter(status.eq("open"))
            .select(crate::models::ToolUsageSession::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Open a session (place the hold).
    pub fn insert_tool_session(
        &self,
        session: &crate::models::NewToolUsageSession,
    ) -> Result<crate::models::ToolUsageSession, DatabaseError> {
        use crate::schema::tool_usage_sessions;
        let mut conn = self.get_connection()?;
        diesel::insert_into(tool_usage_sessions::table)
            .values(session)
            .returning(crate::models::ToolUsageSession::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Accumulate device-reported usage onto an open session (validated/capped by
    /// the caller). No-op if the session is not open.
    pub fn add_reported_seconds(
        &self,
        session_id: uuid::Uuid,
        seconds: bigdecimal::BigDecimal,
    ) -> Result<(), DatabaseError> {
        use crate::schema::tool_usage_sessions::dsl::*;
        let mut conn = self.get_connection()?;
        // COALESCE(reported_seconds, 0) + seconds, in Rust to keep the query simple.
        let current: Option<Option<bigdecimal::BigDecimal>> = tool_usage_sessions
            .filter(id.eq(session_id))
            .filter(status.eq("open"))
            .select(reported_seconds)
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        let Some(existing) = current else {
            return Ok(()); // not open (or gone) -- nothing to accumulate
        };
        let total = existing.unwrap_or_else(|| bigdecimal::BigDecimal::from(0)) + seconds;
        diesel::update(
            tool_usage_sessions
                .filter(id.eq(session_id))
                .filter(status.eq("open")),
        )
        .set(reported_seconds.eq(total))
        .execute(&mut conn)
        .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    /// Settle an open session: record the charge/usage and close it. Returns
    /// `true` iff this call was the one that closed it -- **idempotent**: a second
    /// settle (a replayed report) matches no open row and returns `false`, so a
    /// charge is posted at most once.
    pub fn settle_tool_session(
        &self,
        session_id: uuid::Uuid,
        reported: Option<bigdecimal::BigDecimal>,
        charged: bigdecimal::BigDecimal,
        ledger_entry: Option<uuid::Uuid>,
        new_status: &str,
    ) -> Result<bool, DatabaseError> {
        use crate::schema::tool_usage_sessions::dsl::*;
        let mut conn = self.get_connection()?;
        let affected = diesel::update(
            tool_usage_sessions
                .filter(id.eq(session_id))
                .filter(status.eq("open")),
        )
        .set((
            ended_at.eq(Some(chrono::Utc::now())),
            reported_seconds.eq(reported),
            charged_amount.eq(Some(charged)),
            ledger_entry_id.eq(ledger_entry),
            status.eq(new_status),
        ))
        .execute(&mut conn)
        .map_err(DatabaseError::Diesel)?;
        Ok(affected > 0)
    }

    /// Open sessions started before `cutoff`, for the abandoned-session sweep.
    pub fn open_tool_sessions_older_than(
        &self,
        cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<crate::models::ToolUsageSession>, DatabaseError> {
        use crate::schema::tool_usage_sessions::dsl::*;
        let mut conn = self.get_connection()?;
        tool_usage_sessions
            .filter(status.eq("open"))
            .filter(started_at.lt(cutoff))
            .select(crate::models::ToolUsageSession::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// A member's recent tool sessions, for the admin/self view.
    pub fn list_tool_sessions_for_user(
        &self,
        uid: uuid::Uuid,
        limit: i64,
    ) -> Result<Vec<crate::models::ToolUsageSession>, DatabaseError> {
        use crate::schema::tool_usage_sessions::dsl::*;
        let mut conn = self.get_connection()?;
        tool_usage_sessions
            .filter(user_id.eq(uid))
            .order(started_at.desc())
            .limit(limit)
            .select(crate::models::ToolUsageSession::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // --- users.mfa_enrolled_at -------------------------------------------

    pub fn set_user_mfa_enrolled(
        &self,
        uid: uuid::Uuid,
        when: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<(), DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        // The row count is the answer, not a detail to discard.
        // The flag that says an account has a second factor.
        let affected = diesel::update(users.filter(id.eq(uid)))
            .set(mfa_enrolled_at.eq(when))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        if affected == 0 {
            return Err(DatabaseError::Diesel(diesel::result::Error::NotFound));
        }

        Ok(())
    }

    /// Wipe every MFA artifact for a user in a single transaction and clear
    /// `mfa_enrolled_at`. Used by the admin "reset MFA" recovery action.
    pub fn reset_user_mfa(&self, uid: uuid::Uuid) -> Result<(), DatabaseError> {
        let mut conn = self.get_connection()?;
        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            diesel::delete(
                crate::schema::user_mfa_totp::table
                    .filter(crate::schema::user_mfa_totp::user_id.eq(uid)),
            )
            .execute(conn)?;
            diesel::delete(
                crate::schema::user_mfa_webauthn::table
                    .filter(crate::schema::user_mfa_webauthn::user_id.eq(uid)),
            )
            .execute(conn)?;
            diesel::delete(
                crate::schema::user_mfa_recovery_codes::table
                    .filter(crate::schema::user_mfa_recovery_codes::user_id.eq(uid)),
            )
            .execute(conn)?;
            diesel::update(crate::schema::users::table.filter(crate::schema::users::id.eq(uid)))
                .set(
                    crate::schema::users::mfa_enrolled_at
                        .eq::<Option<chrono::DateTime<chrono::Utc>>>(None),
                )
                .execute(conn)?;
            Ok(())
        })
        .map_err(DatabaseError::Diesel)
    }

    // ---------------------------------------------------------------------
    // Door access
    // ---------------------------------------------------------------------

    pub fn list_doors(&self) -> Result<Vec<crate::models::Door>, DatabaseError> {
        use crate::schema::doors::dsl::*;
        let mut conn = self.get_connection()?;
        doors
            .order(name.asc())
            .select(crate::models::Door::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_door(&self, did: uuid::Uuid) -> Result<crate::models::Door, DatabaseError> {
        use crate::schema::doors::dsl::*;
        let mut conn = self.get_connection()?;
        doors
            .find(did)
            .select(crate::models::Door::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_door(
        &self,
        new_door: &crate::models::NewDoor,
    ) -> Result<crate::models::Door, DatabaseError> {
        use crate::schema::doors;
        let mut conn = self.get_connection()?;
        diesel::insert_into(doors::table)
            .values(new_door)
            .returning(crate::models::Door::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_door(
        &self,
        did: uuid::Uuid,
        changes: &crate::models::UpdateDoor,
    ) -> Result<crate::models::Door, DatabaseError> {
        use crate::schema::doors::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(doors.find(did))
            .set(changes)
            .returning(crate::models::Door::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_door(&self, did: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::doors::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(doors.find(did))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Doors served by a specific edge device.
    pub fn list_doors_for_device(
        &self,
        edge_id: uuid::Uuid,
    ) -> Result<Vec<crate::models::Door>, DatabaseError> {
        use crate::schema::doors::dsl::*;
        let mut conn = self.get_connection()?;
        doors
            .filter(edge_device_id.eq(edge_id))
            .select(crate::models::Door::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Distinct edge device IDs that have at least one door bound to them.
    /// Used for "republish to every device whose state might have changed".
    pub fn list_door_device_ids(&self) -> Result<Vec<uuid::Uuid>, DatabaseError> {
        use crate::schema::doors::dsl::*;
        let mut conn = self.get_connection()?;
        let ids: Vec<Option<uuid::Uuid>> = doors
            .select(edge_device_id)
            .distinct()
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(ids.into_iter().flatten().collect())
    }

    pub fn list_rules_for_door(
        &self,
        did: uuid::Uuid,
    ) -> Result<Vec<crate::models::DoorAccessRule>, DatabaseError> {
        use crate::schema::door_access_rules::dsl::*;
        let mut conn = self.get_connection()?;
        door_access_rules
            .filter(door_id.eq(did))
            .order(created_at.asc())
            .select(crate::models::DoorAccessRule::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn insert_door_rule(
        &self,
        new_rule: &crate::models::NewDoorAccessRule,
    ) -> Result<crate::models::DoorAccessRule, DatabaseError> {
        use crate::schema::door_access_rules;
        let mut conn = self.get_connection()?;
        diesel::insert_into(door_access_rules::table)
            .values(new_rule)
            .returning(crate::models::DoorAccessRule::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_door_rule(
        &self,
        did: uuid::Uuid,
        rid: uuid::Uuid,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::door_access_rules::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(door_access_rules.filter(id.eq(rid)).filter(door_id.eq(did)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn insert_door_access_event(
        &self,
        new_event: &crate::models::NewDoorAccessEvent,
    ) -> Result<crate::models::DoorAccessEvent, DatabaseError> {
        use crate::schema::door_access_events;
        let mut conn = self.get_connection()?;
        diesel::insert_into(door_access_events::table)
            .values(new_event)
            .returning(crate::models::DoorAccessEvent::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_door_events(
        &self,
        did: uuid::Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<crate::models::DoorAccessEvent>, DatabaseError> {
        use crate::schema::door_access_events::dsl::*;
        let mut conn = self.get_connection()?;
        door_access_events
            .filter(door_id.eq(did))
            .order(occurred_at.desc())
            .limit(limit)
            .offset(offset)
            .select(crate::models::DoorAccessEvent::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn insert_door_checkin(
        &self,
        new_checkin: &crate::models::NewDoorCheckin,
    ) -> Result<crate::models::DoorCheckin, DatabaseError> {
        use crate::schema::door_checkins;
        let mut conn = self.get_connection()?;
        diesel::insert_into(door_checkins::table)
            .values(new_checkin)
            .returning(crate::models::DoorCheckin::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ---------------------------------------------------------------------
    // Home links (admin-curated links on the public home page)
    // ---------------------------------------------------------------------

    pub fn list_home_links(&self) -> Result<Vec<crate::models::HomeLink>, DatabaseError> {
        use crate::schema::home_links::dsl::*;
        let mut conn = self.get_connection()?;
        home_links
            .order((sort_order.asc(), label.asc()))
            .select(crate::models::HomeLink::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_home_link(
        &self,
        link_id: uuid::Uuid,
    ) -> Result<crate::models::HomeLink, DatabaseError> {
        use crate::schema::home_links::dsl::*;
        let mut conn = self.get_connection()?;
        home_links
            .find(link_id)
            .select(crate::models::HomeLink::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_home_link(
        &self,
        new_link: &crate::models::NewHomeLink,
    ) -> Result<crate::models::HomeLink, DatabaseError> {
        use crate::schema::home_links;
        let mut conn = self.get_connection()?;
        diesel::insert_into(home_links::table)
            .values(new_link)
            .returning(crate::models::HomeLink::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_home_link(
        &self,
        link_id: uuid::Uuid,
        changes: &crate::models::UpdateHomeLink,
    ) -> Result<crate::models::HomeLink, DatabaseError> {
        use crate::schema::home_links::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(home_links.find(link_id))
            .set(changes)
            .returning(crate::models::HomeLink::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_home_link(&self, link_id: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::home_links::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(home_links.find(link_id))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ---------------------------------------------------------------------
    // Schedules (weekly windows attachable to access rules)
    // ---------------------------------------------------------------------

    pub fn list_schedules(&self) -> Result<Vec<crate::models::Schedule>, DatabaseError> {
        use crate::schema::schedules::dsl::*;
        let mut conn = self.get_connection()?;
        schedules
            .order(name.asc())
            .select(crate::models::Schedule::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_schedule(&self, sid: uuid::Uuid) -> Result<crate::models::Schedule, DatabaseError> {
        use crate::schema::schedules::dsl::*;
        let mut conn = self.get_connection()?;
        schedules
            .find(sid)
            .select(crate::models::Schedule::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_schedule(
        &self,
        new_schedule: &crate::models::NewSchedule,
    ) -> Result<crate::models::Schedule, DatabaseError> {
        use crate::schema::schedules;
        let mut conn = self.get_connection()?;
        diesel::insert_into(schedules::table)
            .values(new_schedule)
            .returning(crate::models::Schedule::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_schedule(
        &self,
        sid: uuid::Uuid,
        changes: &crate::models::UpdateSchedule,
    ) -> Result<crate::models::Schedule, DatabaseError> {
        use crate::schema::schedules::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(schedules.find(sid))
            .set(changes)
            .returning(crate::models::Schedule::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_schedule(&self, sid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::schedules::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(schedules.find(sid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ---------------------------------------------------------------------
    // Places (configurable hierarchy)
    // ---------------------------------------------------------------------

    pub fn list_places(&self) -> Result<Vec<crate::models::Place>, DatabaseError> {
        use crate::schema::places::dsl::*;
        let mut conn = self.get_connection()?;
        places
            .order((parent_id.asc().nulls_first(), name.asc()))
            .select(crate::models::Place::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_place(&self, pid: uuid::Uuid) -> Result<crate::models::Place, DatabaseError> {
        use crate::schema::places::dsl::*;
        let mut conn = self.get_connection()?;
        places
            .find(pid)
            .select(crate::models::Place::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_child_places(
        &self,
        parent: uuid::Uuid,
    ) -> Result<Vec<crate::models::Place>, DatabaseError> {
        use crate::schema::places::dsl::*;
        let mut conn = self.get_connection()?;
        places
            .filter(parent_id.eq(Some(parent)))
            .order(name.asc())
            .select(crate::models::Place::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Walk parent links until a root is hit. Returns `[root, …, immediate_parent]`
    /// (i.e. ordered top-down, excluding `pid` itself). Bounded at 32 steps as
    /// a safety net even though the API rejects cycles on every write.
    pub fn place_ancestors(
        &self,
        pid: uuid::Uuid,
    ) -> Result<Vec<crate::models::Place>, DatabaseError> {
        let mut acc = Vec::new();
        let mut current = self.get_place(pid)?.parent_id;
        let mut depth = 0;
        while let Some(parent) = current {
            depth += 1;
            if depth > 32 {
                return Err(DatabaseError::Other(
                    "place ancestor chain exceeded 32 levels".into(),
                ));
            }
            let p = self.get_place(parent)?;
            current = p.parent_id;
            acc.push(p);
        }
        acc.reverse();
        Ok(acc)
    }

    pub fn create_place(
        &self,
        new_place: &crate::models::NewPlace,
    ) -> Result<crate::models::Place, DatabaseError> {
        use crate::schema::places;
        let mut conn = self.get_connection()?;
        diesel::insert_into(places::table)
            .values(new_place)
            .returning(crate::models::Place::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_place(
        &self,
        pid: uuid::Uuid,
        changes: &crate::models::UpdatePlace,
    ) -> Result<crate::models::Place, DatabaseError> {
        use crate::schema::places::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(places.find(pid))
            .set(changes)
            .returning(crate::models::Place::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_place(&self, pid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::places::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(places.find(pid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Counts of attached entities referencing this place. Used by the API
    /// to render "what's in here" hints.
    pub fn count_place_references(
        &self,
        pid: uuid::Uuid,
    ) -> Result<(i64, i64, i64), DatabaseError> {
        use diesel::dsl::count_star;
        let mut conn = self.get_connection()?;
        let doors_count: i64 = {
            use crate::schema::doors::dsl::*;
            doors
                .filter(place_id_from.eq(Some(pid)).or(place_id_to.eq(Some(pid))))
                .select(count_star())
                .first(&mut conn)
                .map_err(DatabaseError::Diesel)?
        };
        let tools_count: i64 = {
            use crate::schema::tools::dsl::*;
            tools
                .filter(place_id.eq(Some(pid)))
                .select(count_star())
                .first(&mut conn)
                .map_err(DatabaseError::Diesel)?
        };
        let devices_count: i64 = {
            use crate::schema::space_devices::dsl::*;
            space_devices
                .filter(place_id.eq(Some(pid)))
                .filter(deleted_at.is_null())
                .select(count_star())
                .first(&mut conn)
                .map_err(DatabaseError::Diesel)?
        };
        Ok((doors_count, tools_count, devices_count))
    }

    // ── Power topology (#42): circuits / outlets / receptacles ───────────────

    pub fn list_power_circuits(&self) -> Result<Vec<crate::models::PowerCircuit>, DatabaseError> {
        use crate::schema::power_circuits::dsl::*;
        let mut conn = self.get_connection()?;
        power_circuits
            .order((parent_circuit_id.asc().nulls_first(), breaker_label.asc()))
            .select(crate::models::PowerCircuit::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_power_circuit(
        &self,
        cid: uuid::Uuid,
    ) -> Result<crate::models::PowerCircuit, DatabaseError> {
        use crate::schema::power_circuits::dsl::*;
        let mut conn = self.get_connection()?;
        power_circuits
            .find(cid)
            .select(crate::models::PowerCircuit::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Walk `parent_circuit_id` links until a trunk is hit. Returns
    /// `[trunk, ..., immediate_parent]` (top-down, excluding `cid`). Bounded at
    /// 32 steps as a safety net even though the API rejects cycles on every
    /// write. Mirrors `place_ancestors`.
    pub fn power_circuit_ancestors(
        &self,
        cid: uuid::Uuid,
    ) -> Result<Vec<crate::models::PowerCircuit>, DatabaseError> {
        let mut acc = Vec::new();
        let mut current = self.get_power_circuit(cid)?.parent_circuit_id;
        let mut depth = 0;
        while let Some(parent) = current {
            depth += 1;
            if depth > 32 {
                return Err(DatabaseError::Other(
                    "power circuit ancestor chain exceeded 32 levels".into(),
                ));
            }
            let c = self.get_power_circuit(parent)?;
            current = c.parent_circuit_id;
            acc.push(c);
        }
        acc.reverse();
        Ok(acc)
    }

    pub fn create_power_circuit(
        &self,
        new_circuit: &crate::models::NewPowerCircuit,
    ) -> Result<crate::models::PowerCircuit, DatabaseError> {
        use crate::schema::power_circuits;
        let mut conn = self.get_connection()?;
        diesel::insert_into(power_circuits::table)
            .values(new_circuit)
            .returning(crate::models::PowerCircuit::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_power_circuit(
        &self,
        cid: uuid::Uuid,
        changes: &crate::models::UpdatePowerCircuit,
    ) -> Result<crate::models::PowerCircuit, DatabaseError> {
        use crate::schema::power_circuits::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(power_circuits.find(cid))
            .set(changes)
            .returning(crate::models::PowerCircuit::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_power_circuit(&self, cid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::power_circuits::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(power_circuits.find(cid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_power_outlets(&self) -> Result<Vec<crate::models::PowerOutlet>, DatabaseError> {
        use crate::schema::power_outlets::dsl::*;
        let mut conn = self.get_connection()?;
        power_outlets
            .order(label.asc())
            .select(crate::models::PowerOutlet::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_outlets_for_circuit(
        &self,
        cid: uuid::Uuid,
    ) -> Result<Vec<crate::models::PowerOutlet>, DatabaseError> {
        use crate::schema::power_outlets::dsl::*;
        let mut conn = self.get_connection()?;
        power_outlets
            .filter(circuit_id.eq(cid))
            .order(label.asc())
            .select(crate::models::PowerOutlet::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_power_outlet(
        &self,
        oid: uuid::Uuid,
    ) -> Result<crate::models::PowerOutlet, DatabaseError> {
        use crate::schema::power_outlets::dsl::*;
        let mut conn = self.get_connection()?;
        power_outlets
            .find(oid)
            .select(crate::models::PowerOutlet::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_power_outlet(
        &self,
        new_outlet: &crate::models::NewPowerOutlet,
    ) -> Result<crate::models::PowerOutlet, DatabaseError> {
        use crate::schema::power_outlets;
        let mut conn = self.get_connection()?;
        diesel::insert_into(power_outlets::table)
            .values(new_outlet)
            .returning(crate::models::PowerOutlet::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_power_outlet(
        &self,
        oid: uuid::Uuid,
        changes: &crate::models::UpdatePowerOutlet,
    ) -> Result<crate::models::PowerOutlet, DatabaseError> {
        use crate::schema::power_outlets::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(power_outlets.find(oid))
            .set(changes)
            .returning(crate::models::PowerOutlet::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_power_outlet(&self, oid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::power_outlets::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(power_outlets.find(oid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_receptacles_for_outlet(
        &self,
        oid: uuid::Uuid,
    ) -> Result<Vec<crate::models::PowerReceptacle>, DatabaseError> {
        use crate::schema::power_receptacles::dsl::*;
        let mut conn = self.get_connection()?;
        power_receptacles
            .filter(outlet_id.eq(oid))
            .order(label.asc())
            .select(crate::models::PowerReceptacle::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_power_receptacles(
        &self,
    ) -> Result<Vec<crate::models::PowerReceptacle>, DatabaseError> {
        use crate::schema::power_receptacles::dsl::*;
        let mut conn = self.get_connection()?;
        power_receptacles
            .order(label.asc())
            .select(crate::models::PowerReceptacle::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn get_power_receptacle(
        &self,
        rid: uuid::Uuid,
    ) -> Result<crate::models::PowerReceptacle, DatabaseError> {
        use crate::schema::power_receptacles::dsl::*;
        let mut conn = self.get_connection()?;
        power_receptacles
            .find(rid)
            .select(crate::models::PowerReceptacle::as_select())
            .first(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_power_receptacle(
        &self,
        new_receptacle: &crate::models::NewPowerReceptacle,
    ) -> Result<crate::models::PowerReceptacle, DatabaseError> {
        use crate::schema::power_receptacles;
        let mut conn = self.get_connection()?;
        diesel::insert_into(power_receptacles::table)
            .values(new_receptacle)
            .returning(crate::models::PowerReceptacle::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn update_power_receptacle(
        &self,
        rid: uuid::Uuid,
        changes: &crate::models::UpdatePowerReceptacle,
    ) -> Result<crate::models::PowerReceptacle, DatabaseError> {
        use crate::schema::power_receptacles::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(power_receptacles.find(rid))
            .set(changes)
            .returning(crate::models::PowerReceptacle::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn delete_power_receptacle(&self, rid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::power_receptacles::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(power_receptacles.find(rid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Assign a tool to a receptacle, or clear it (`None`). The partial-unique
    /// index on `tools.receptacle_id` makes a second tool claiming a live
    /// receptacle a unique violation (409), not a silent double-binding.
    /// Returns the number of tool rows changed (0 = no such tool).
    pub fn assign_tool_receptacle(
        &self,
        tid: uuid::Uuid,
        rid: Option<uuid::Uuid>,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::tools::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(tools.find(tid))
            .set((receptacle_id.eq(rid), updated_at.eq(chrono::Utc::now())))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ── Tool power telemetry (#43): latest-per-tool ─────────────────────────

    /// Insert-or-update the latest reading for a tool, keeping exactly one row
    /// per tool (`ON CONFLICT (tool_id)`). Returns the stored row.
    pub fn upsert_tool_power_state(
        &self,
        reading: &crate::models::NewToolPowerState,
    ) -> Result<crate::models::ToolPowerState, DatabaseError> {
        use crate::schema::tool_power_state::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::insert_into(tool_power_state)
            .values(reading)
            .on_conflict(tool_id)
            .do_update()
            .set((
                last_draw_amps.eq(&reading.last_draw_amps),
                last_voltage.eq(&reading.last_voltage),
                reported_max_voltage.eq(&reading.reported_max_voltage),
                reported_amperage_limit.eq(&reading.reported_amperage_limit),
                last_reported_at.eq(&reading.last_reported_at),
                last_relay_on.eq(&reading.last_relay_on),
                power_evidence_since.eq(&reading.power_evidence_since),
                updated_at.eq(chrono::Utc::now()),
            ))
            .returning(crate::models::ToolPowerState::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// When each tool was last reported as drawing unauthorized power.
    ///
    /// Compared against `power_evidence_since` by the caller so a report is made
    /// once per EPISODE rather than once per tool ever: when the tool is
    /// switched off legitimately the evidence clock resets, and a later bypass
    /// starts a new run that deserves its own row. Reporting once per tool
    /// forever would mean the second incident is invisible.
    pub fn latest_unauthorized_power_reports(
        &self,
    ) -> Result<std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>, DatabaseError>
    {
        use diesel::sql_types::{Text, Timestamptz};

        #[derive(diesel::QueryableByName)]
        struct Row {
            #[diesel(sql_type = Text)]
            tool_id: String,
            #[diesel(sql_type = Timestamptz)]
            reported_at: chrono::DateTime<chrono::Utc>,
        }

        let mut conn = self.get_connection()?;
        let rows: Vec<Row> = diesel::sql_query(
            "SELECT DISTINCT ON (event_data->>'tool_id') \
                    event_data->>'tool_id' AS tool_id, created_at AS reported_at \
             FROM audit_logs \
             WHERE event_type = 'unauthorized_power_detected' \
               AND event_data->>'tool_id' IS NOT NULL \
             ORDER BY event_data->>'tool_id', created_at DESC",
        )
        .load(&mut conn)
        .map_err(DatabaseError::Diesel)?;

        Ok(rows
            .into_iter()
            .map(|r| (r.tool_id, r.reported_at))
            .collect())
    }

    /// The most recently recorded isolation state per unbound device, from the
    /// audit trail -- the device-keyed twin of
    /// [`Self::latest_module_liveness_records`], so the unbound sweep is
    /// transition-based across a restart for the same reason.
    pub fn latest_device_isolation_records(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, DatabaseError> {
        use diesel::sql_types::Text;

        #[derive(diesel::QueryableByName)]
        struct Row {
            #[diesel(sql_type = Text)]
            device_id: String,
            #[diesel(sql_type = Text)]
            state: String,
        }

        let mut conn = self.get_connection()?;
        let rows: Vec<Row> = diesel::sql_query(
            "SELECT DISTINCT ON (event_data->>'device_id') \
                    event_data->>'device_id' AS device_id, \
                    COALESCE(event_data->>'state', 'silent') AS state \
             FROM audit_logs \
             WHERE event_type = 'edge_isolation_reported' \
               AND event_data->>'device_id' IS NOT NULL \
             ORDER BY event_data->>'device_id', created_at DESC",
        )
        .load(&mut conn)
        .map_err(DatabaseError::Diesel)?;

        Ok(rows.into_iter().map(|r| (r.device_id, r.state)).collect())
    }

    /// The latest reading for one tool, if it has ever reported.
    pub fn get_tool_power_state(
        &self,
        tid: uuid::Uuid,
    ) -> Result<Option<crate::models::ToolPowerState>, DatabaseError> {
        use crate::schema::tool_power_state::dsl::*;
        let mut conn = self.get_connection()?;
        tool_power_state
            .filter(tool_id.eq(tid))
            .select(crate::models::ToolPowerState::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_tool_power_state(
        &self,
    ) -> Result<Vec<crate::models::ToolPowerState>, DatabaseError> {
        use crate::schema::tool_power_state::dsl::*;
        let mut conn = self.get_connection()?;
        tool_power_state
            .select(crate::models::ToolPowerState::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Every tool's `(id, receptacle_id)`, unpaginated -- for resolving which
    /// circuit a tool's draw belongs to when aggregating. `get_tools` paginates,
    /// so it cannot be used for a total that must cover every tool.
    pub fn all_tool_receptacle_links(
        &self,
    ) -> Result<Vec<(uuid::Uuid, Option<uuid::Uuid>)>, DatabaseError> {
        use crate::schema::tools::dsl::*;
        let mut conn = self.get_connection()?;
        tools
            .select((id, receptacle_id))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    // ── Power interrupt / lockout (#44) ──────────────────────────────────────

    /// Summed latest draw per circuit (every circuit present, 0 when idle),
    /// resolving each tool through receptacle -> outlet -> circuit. The one place
    /// this aggregation lives; the telemetry endpoint and the overage evaluator
    /// both read it so they cannot disagree.
    pub fn circuit_draw_totals(
        &self,
    ) -> Result<Vec<(uuid::Uuid, bigdecimal::BigDecimal)>, DatabaseError> {
        use std::collections::HashMap;

        let states = self.list_tool_power_state()?;
        let links = self.all_tool_receptacle_links()?;
        let receptacles = self.list_power_receptacles()?;
        let outlets = self.list_power_outlets()?;
        let circuits = self.list_power_circuits()?;

        let recep_to_outlet: HashMap<uuid::Uuid, uuid::Uuid> =
            receptacles.iter().map(|r| (r.id, r.outlet_id)).collect();
        let outlet_to_circuit: HashMap<uuid::Uuid, uuid::Uuid> =
            outlets.iter().map(|o| (o.id, o.circuit_id)).collect();
        let tool_to_receptacle: HashMap<uuid::Uuid, uuid::Uuid> = links
            .into_iter()
            .filter_map(|(t, r)| r.map(|r| (t, r)))
            .collect();

        // The DB loads are the I/O; the per-circuit summation is pure and
        // unit-tested directly (see `sum_draw_per_circuit`).
        let circuit_ids: Vec<uuid::Uuid> = circuits.iter().map(|c| c.id).collect();
        let tool_draws: Vec<(uuid::Uuid, bigdecimal::BigDecimal)> = states
            .iter()
            .filter_map(|s| s.last_draw_amps.as_ref().map(|d| (s.tool_id, d.clone())))
            .collect();
        Ok(sum_draw_per_circuit(
            &circuit_ids,
            &tool_draws,
            &tool_to_receptacle,
            &recep_to_outlet,
            &outlet_to_circuit,
        ))
    }

    /// True if this tool must be denied for a power reason: its OWN firmware
    /// self-trip lockout, OR its CIRCUIT's lockout. A tool with no receptacle
    /// (battery/unmapped) has no circuit, so only its own lockout applies.
    pub fn tool_is_locked_out(&self, tid: uuid::Uuid) -> Result<bool, DatabaseError> {
        let mut conn = self.get_connection()?;

        // Tool-scoped (firmware self-trip).
        {
            use crate::schema::tool_power_state::dsl as tps;
            let own: Option<bool> = tps::tool_power_state
                .find(tid)
                .select(tps::locked_out)
                .first(&mut conn)
                .optional()
                .map_err(DatabaseError::Diesel)?;
            if own == Some(true) {
                return Ok(true);
            }
        }

        // Circuit-scoped: tool -> receptacle -> outlet -> circuit.
        let receptacle_id: Option<uuid::Uuid> = {
            use crate::schema::tools::dsl as t;
            t::tools
                .find(tid)
                .select(t::receptacle_id)
                .first::<Option<uuid::Uuid>>(&mut conn)
                .optional()
                .map_err(DatabaseError::Diesel)?
                .flatten()
        };
        let Some(rid) = receptacle_id else {
            return Ok(false);
        };
        let outlet_id: Option<uuid::Uuid> = {
            use crate::schema::power_receptacles::dsl as r;
            r::power_receptacles
                .find(rid)
                .select(r::outlet_id)
                .first(&mut conn)
                .optional()
                .map_err(DatabaseError::Diesel)?
        };
        let Some(oid) = outlet_id else {
            return Ok(false);
        };
        let circuit_id: Option<uuid::Uuid> = {
            use crate::schema::power_outlets::dsl as o;
            o::power_outlets
                .find(oid)
                .select(o::circuit_id)
                .first(&mut conn)
                .optional()
                .map_err(DatabaseError::Diesel)?
        };
        let Some(cid) = circuit_id else {
            return Ok(false);
        };
        use crate::schema::power_circuits::dsl as c;
        let locked: Option<bool> = c::power_circuits
            .find(cid)
            .select(c::locked_out)
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;
        Ok(locked == Some(true))
    }

    pub fn engage_circuit_lockout(
        &self,
        cid: uuid::Uuid,
        source: &str,
        reason: &str,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::power_circuits::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(power_circuits.find(cid).filter(locked_out.eq(false)))
            .set((
                locked_out.eq(true),
                lockout_source.eq(Some(source.to_string())),
                lockout_reason.eq(Some(reason.to_string())),
                locked_out_at.eq(Some(chrono::Utc::now())),
                updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn clear_circuit_lockout(
        &self,
        cid: uuid::Uuid,
        by: Option<uuid::Uuid>,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::power_circuits::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(power_circuits.find(cid))
            .set((
                locked_out.eq(false),
                lockout_source.eq(None::<String>),
                lockout_reason.eq(None::<String>),
                locked_out_at.eq(None::<chrono::DateTime<chrono::Utc>>),
                locked_out_by.eq(by),
                updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Returns the number of rows written (always 1 -- it is an upsert, so a
    /// self-trip that arrives before any reading still records the lockout).
    pub fn engage_tool_lockout(
        &self,
        tid: uuid::Uuid,
        reason: &str,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::tool_power_state::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::insert_into(tool_power_state)
            .values((
                tool_id.eq(tid),
                locked_out.eq(true),
                lockout_reason.eq(Some(reason.to_string())),
                locked_out_at.eq(Some(chrono::Utc::now())),
            ))
            .on_conflict(tool_id)
            .do_update()
            .set((
                locked_out.eq(true),
                lockout_reason.eq(Some(reason.to_string())),
                locked_out_at.eq(Some(chrono::Utc::now())),
                updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn clear_tool_lockout(&self, tid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::tool_power_state::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(tool_power_state.find(tid))
            .set((
                locked_out.eq(false),
                lockout_reason.eq(None::<String>),
                locked_out_at.eq(None::<chrono::DateTime<chrono::Utc>>),
                updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Recompute every circuit's summed draw and engage a `server_aggregate`
    /// lockout on any that now EXCEED their amperage_limit and are not already
    /// locked. Returns the circuits it just tripped as `(id, total, limit)` so
    /// the caller can audit and re-broadcast. Strictly `>`: a circuit at exactly
    /// its rating is at capacity, not overloaded.
    pub fn evaluate_circuit_overages(
        &self,
    ) -> Result<Vec<(uuid::Uuid, bigdecimal::BigDecimal, bigdecimal::BigDecimal)>, DatabaseError>
    {
        let totals = self.circuit_draw_totals()?;
        let circuits = self.list_power_circuits()?;

        // The over-limit selection (strictly `>`, skipping already-locked
        // circuits) is pure and unit-tested directly (see `overloaded_circuits`);
        // only the lockout write below touches the DB.
        let candidates: Vec<(uuid::Uuid, bigdecimal::BigDecimal, bool)> = circuits
            .iter()
            .map(|c| (c.id, c.amperage_limit.clone(), c.locked_out))
            .collect();
        let mut tripped = Vec::new();
        for (id, total, limit) in overloaded_circuits(&candidates, &totals) {
            let n = self.engage_circuit_lockout(
                id,
                "server_aggregate",
                &format!("summed draw {total} A exceeds circuit limit {limit} A"),
            )?;
            if n > 0 {
                tripped.push((id, total, limit));
            }
        }
        Ok(tripped)
    }

    /// Build the power-lockout + topology snapshot the server pushes to the edge
    /// (#48): the currently-locked tool ids (own firmware self-trip OR their
    /// circuit), plus every circuit's amperage limit and every tool's resolved
    /// circuit, so the edge can fail-secure and aggregate draw locally. Computed
    /// from a handful of small loads (no N+1 `tool_is_locked_out` walk).
    pub fn power_state_snapshot(&self) -> Result<css_lib::wire::PowerStatePayload, DatabaseError> {
        use std::collections::{HashMap, HashSet};

        let circuits = self.list_power_circuits()?;
        let receptacles = self.list_power_receptacles()?;
        let outlets = self.list_power_outlets()?;
        let states = self.list_tool_power_state()?;

        let tool_rows: Vec<(uuid::Uuid, Option<String>, Option<uuid::Uuid>)> = {
            use crate::schema::tools::dsl::*;
            let mut conn = self.get_connection()?;
            tools
                .select((id, external_id, receptacle_id))
                .load(&mut conn)
                .map_err(DatabaseError::Diesel)?
        };

        let recep_to_outlet: HashMap<uuid::Uuid, uuid::Uuid> =
            receptacles.iter().map(|r| (r.id, r.outlet_id)).collect();
        let outlet_to_circuit: HashMap<uuid::Uuid, uuid::Uuid> =
            outlets.iter().map(|o| (o.id, o.circuit_id)).collect();
        let locked_circuits: HashSet<uuid::Uuid> = circuits
            .iter()
            .filter(|c| c.locked_out)
            .map(|c| c.id)
            .collect();
        let own_locked: HashSet<uuid::Uuid> = states
            .iter()
            .filter(|s| s.locked_out)
            .map(|s| s.tool_id)
            .collect();

        let tool_circuit = |receptacle_id: Option<uuid::Uuid>| -> Option<uuid::Uuid> {
            let rid = receptacle_id?;
            let oid = recep_to_outlet.get(&rid)?;
            outlet_to_circuit.get(oid).copied()
        };

        let mut locked_tool_ids = Vec::new();
        let mut tools_out = Vec::new();
        for (tid, ext, rid) in tool_rows {
            let circuit = tool_circuit(rid);
            let locked =
                own_locked.contains(&tid) || circuit.is_some_and(|c| locked_circuits.contains(&c));
            if locked {
                locked_tool_ids.push(tid.to_string());
            }
            tools_out.push(css_lib::wire::PowerStateTool {
                id: tid.to_string(),
                external_id: ext,
                circuit_id: circuit.map(|c| c.to_string()),
            });
        }

        Ok(css_lib::wire::PowerStatePayload {
            as_of: chrono::Utc::now().to_rfc3339(),
            locked_tool_ids,
            circuits: circuits
                .iter()
                .map(|c| css_lib::wire::PowerStateCircuit {
                    id: c.id.to_string(),
                    amperage_limit: c.amperage_limit.to_string(),
                })
                .collect(),
            tools: tools_out,
        })
    }

    /// Set / clear the `place_id` on a device.
    pub fn set_space_device_place(
        &self,
        device_id: uuid::Uuid,
        new_place: Option<uuid::Uuid>,
    ) -> Result<usize, DatabaseError> {
        use crate::schema::space_devices::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(space_devices.filter(id.eq(device_id)))
            .set((place_id.eq(new_place), updated_at.eq(chrono::Utc::now())))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Every active user. Used by the door state compiler to expand role
    /// rules to a flat card allow-list.
    pub fn list_active_users(&self) -> Result<Vec<crate::models::User>, DatabaseError> {
        use crate::schema::users::dsl::*;
        let mut conn = self.get_connection()?;
        users
            .filter(is_active.eq(true))
            .select(crate::models::User::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Recompute `users.mfa_enrolled_at` for a user based on whether any
    /// confirmed method exists. Call after add/remove operations.
    pub fn recompute_user_mfa_enrolled(&self, uid: uuid::Uuid) -> Result<bool, DatabaseError> {
        let has_totp = self
            .get_user_totp(uid)?
            .map(|t| t.confirmed_at.is_some())
            .unwrap_or(false);
        let has_webauthn = !self.list_user_webauthn(uid)?.is_empty();
        let enrolled = has_totp || has_webauthn;
        self.set_user_mfa_enrolled(
            uid,
            if enrolled {
                Some(chrono::Utc::now())
            } else {
                None
            },
        )?;
        Ok(enrolled)
    }
}

// ---------------------------------------------------------------------------
// Profile config version history
// ---------------------------------------------------------------------------

impl DatabaseManager {
    /// Insert a new profile-field-schema version. `version` is one past the
    /// current highest version (starting at 1), computed inside the same
    /// transaction as the insert so concurrent admin edits can't race onto
    /// the same version number.
    pub fn insert_profile_config_version(
        &self,
        fields: serde_json::Value,
        enabled: bool,
        creator: Option<uuid::Uuid>,
    ) -> Result<crate::models::ProfileConfigVersion, DatabaseError> {
        use crate::schema::profile_config_versions::dsl::*;
        let mut conn = self.get_connection()?;

        conn.transaction::<_, diesel::result::Error, _>(|conn| {
            let current_max: Option<i64> = profile_config_versions
                .select(diesel::dsl::max(version))
                .first(conn)?;

            let new_version = crate::models::NewProfileConfigVersion {
                version: current_max.unwrap_or(0) + 1,
                profile_fields: fields,
                created_by: creator,
                profiles_enabled: enabled,
            };

            diesel::insert_into(profile_config_versions)
                .values(&new_version)
                .returning(crate::models::ProfileConfigVersion::as_returning())
                .get_result(conn)
        })
        .map_err(DatabaseError::Diesel)
    }

    /// The current (highest-version) profile field schema, if any versions
    /// have ever been saved.
    pub fn get_latest_profile_config_version(
        &self,
    ) -> Result<Option<crate::models::ProfileConfigVersion>, DatabaseError> {
        use crate::schema::profile_config_versions::dsl::*;
        let mut conn = self.get_connection()?;
        profile_config_versions
            .order(version.desc())
            .select(crate::models::ProfileConfigVersion::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// A single version by its version number.
    pub fn get_profile_config_version(
        &self,
        version_number: i64,
    ) -> Result<Option<crate::models::ProfileConfigVersion>, DatabaseError> {
        use crate::schema::profile_config_versions::dsl::*;
        let mut conn = self.get_connection()?;
        profile_config_versions
            .filter(version.eq(version_number))
            .select(crate::models::ProfileConfigVersion::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Version history, newest first.
    pub fn list_profile_config_versions(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<crate::models::ProfileConfigVersion>, DatabaseError> {
        use crate::schema::profile_config_versions::dsl::*;
        let mut conn = self.get_connection()?;
        profile_config_versions
            .order(version.desc())
            .limit(limit)
            .offset(offset)
            .select(crate::models::ProfileConfigVersion::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }
}

// ── Tool module bindings + interlocks (#83) ───────────────────────────────────

impl DatabaseManager {
    pub fn list_tool_modules(&self) -> Result<Vec<crate::models::ToolModule>, DatabaseError> {
        use crate::schema::tool_modules::dsl::*;
        let mut conn = self.get_connection()?;
        tool_modules
            .order((tool_id.asc(), role.asc(), name.asc()))
            .select(crate::models::ToolModule::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_tool_modules_for_tool(
        &self,
        tid: uuid::Uuid,
    ) -> Result<Vec<crate::models::ToolModule>, DatabaseError> {
        use crate::schema::tool_modules::dsl::*;
        let mut conn = self.get_connection()?;
        tool_modules
            .filter(tool_id.eq(tid))
            .order((role.asc(), name.asc()))
            .select(crate::models::ToolModule::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_tool_module(
        &self,
        new_module: &crate::models::NewToolModule,
    ) -> Result<crate::models::ToolModule, DatabaseError> {
        use crate::schema::tool_modules;
        let mut conn = self.get_connection()?;
        diesel::insert_into(tool_modules::table)
            .values(new_module)
            .returning(crate::models::ToolModule::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Row count, not `()`: the caller answers 404 rather than 200 for an id
    /// that matched nothing (see `checks/tests/writes_report_what_they_changed.rs`).
    pub fn delete_tool_module(&self, mid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::tool_modules::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(tool_modules.find(mid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_tool_interlocks(&self) -> Result<Vec<crate::models::ToolInterlock>, DatabaseError> {
        use crate::schema::tool_interlocks::dsl::*;
        let mut conn = self.get_connection()?;
        tool_interlocks
            .order((tool_id.asc(), kind.asc(), condition.asc()))
            .select(crate::models::ToolInterlock::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn list_tool_interlocks_for_tool(
        &self,
        tid: uuid::Uuid,
    ) -> Result<Vec<crate::models::ToolInterlock>, DatabaseError> {
        use crate::schema::tool_interlocks::dsl::*;
        let mut conn = self.get_connection()?;
        tool_interlocks
            .filter(tool_id.eq(tid))
            .order((kind.asc(), condition.asc()))
            .select(crate::models::ToolInterlock::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    pub fn create_tool_interlock(
        &self,
        new_interlock: &crate::models::NewToolInterlock,
    ) -> Result<crate::models::ToolInterlock, DatabaseError> {
        use crate::schema::tool_interlocks;
        let mut conn = self.get_connection()?;
        diesel::insert_into(tool_interlocks::table)
            .values(new_interlock)
            .returning(crate::models::ToolInterlock::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Row count; see `delete_tool_module`.
    pub fn delete_tool_interlock(&self, iid: uuid::Uuid) -> Result<usize, DatabaseError> {
        use crate::schema::tool_interlocks::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::delete(tool_interlocks.find(iid))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Whether a (not soft-deleted) device exists, for validating a binding's
    /// `device_id` before the insert turns a bad id into a foreign-key 500.
    pub fn space_device_exists(&self, did: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::space_devices::dsl::*;
        let mut conn = self.get_connection()?;
        let found: i64 = space_devices
            .filter(id.eq(did))
            .filter(deleted_at.is_null())
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(found > 0)
    }

    /// Every tool's `external_id`, for stringifying the module-state snapshot.
    pub fn tool_external_ids(&self) -> Result<Vec<(uuid::Uuid, Option<String>)>, DatabaseError> {
        use crate::schema::tools::dsl::*;
        let mut conn = self.get_connection()?;
        tools
            .select((id, external_id))
            .load::<(uuid::Uuid, Option<String>)>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Every device bound to a tool through `tool_modules`, with the module's
    /// own id/name and when the device last reported.
    ///
    /// A device bound in more than one role appears once per binding: the
    /// liveness question is asked of the binding, so a reader and a plug that
    /// happen to be the same physical box are still two things to notice.
    #[allow(clippy::type_complexity)]
    pub fn bound_modules_with_liveness(
        &self,
    ) -> Result<
        Vec<(
            uuid::Uuid,
            String,
            String,
            uuid::Uuid,
            Option<chrono::DateTime<chrono::Utc>>,
        )>,
        DatabaseError,
    > {
        use crate::schema::{space_devices, tool_modules};
        let mut conn = self.get_connection()?;
        tool_modules::table
            .inner_join(space_devices::table.on(space_devices::id.eq(tool_modules::device_id)))
            .filter(space_devices::deleted_at.is_null())
            .select((
                tool_modules::id,
                tool_modules::name,
                tool_modules::role,
                tool_modules::device_id,
                space_devices::last_seen_at,
            ))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Devices that are NOT bound to any tool as a module, with their liveness.
    ///
    /// The module sweep only sees devices in `tool_modules`, which leaves the
    /// edge coordinators themselves uncovered -- an edge is not bound to a tool
    /// as a reader or a plug, so a dark edge would have been invisible to a
    /// sweep that only walked bindings. These are the rest.
    pub fn unbound_devices_with_liveness(
        &self,
    ) -> Result<Vec<(uuid::Uuid, String, Option<chrono::DateTime<chrono::Utc>>)>, DatabaseError>
    {
        use crate::schema::{space_devices, tool_modules};
        let mut conn = self.get_connection()?;
        let bound: Vec<uuid::Uuid> = tool_modules::table
            .select(tool_modules::device_id)
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        space_devices::table
            .filter(space_devices::deleted_at.is_null())
            .filter(diesel::dsl::not(space_devices::id.eq_any(bound)))
            .select((
                space_devices::id,
                space_devices::name,
                space_devices::last_seen_at,
            ))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// The most recently *recorded* liveness state per module, read back out of
    /// the audit trail.
    ///
    /// Deriving this from the log rather than from memory is what makes the
    /// sweep transition-based across a restart: a server that came back up would
    /// otherwise have no idea it had already reported a module silent, and would
    /// say so again on its first sweep. The audit log is the record of truth, so
    /// it is also the right place to ask what has already been said.
    pub fn latest_module_liveness_records(
        &self,
    ) -> Result<std::collections::HashMap<String, String>, DatabaseError> {
        use diesel::sql_types::Text;

        #[derive(diesel::QueryableByName)]
        struct Row {
            #[diesel(sql_type = Text)]
            module_id: String,
            #[diesel(sql_type = Text)]
            event_type: String,
        }

        let mut conn = self.get_connection()?;
        let rows: Vec<Row> = diesel::sql_query(
            "SELECT DISTINCT ON (event_data->>'module_id') \
                    event_data->>'module_id' AS module_id, event_type \
             FROM audit_logs \
             WHERE event_type IN ('tool_module_silent', 'tool_module_returned') \
               AND event_data->>'module_id' IS NOT NULL \
             ORDER BY event_data->>'module_id', created_at DESC",
        )
        .load(&mut conn)
        .map_err(DatabaseError::Diesel)?;

        Ok(rows
            .into_iter()
            .map(|r| (r.module_id, r.event_type))
            .collect())
    }

    /// The `module/state` snapshot (#83): every tool that has at least one module
    /// bound or one interlock defined, with its wiring and rules.
    ///
    /// Only wired tools appear. A tool with no bindings has nothing for the edge
    /// to coordinate, and sending an empty entry for every tool in the space
    /// would make the common case (few wired tools) pay for the rare one.
    pub fn module_state_snapshot(
        &self,
    ) -> Result<css_lib::wire::ToolModuleStatePayload, DatabaseError> {
        use std::collections::BTreeMap;

        let modules = self.list_tool_modules()?;
        let interlocks = self.list_tool_interlocks()?;
        let external: std::collections::HashMap<uuid::Uuid, Option<String>> =
            self.tool_external_ids()?.into_iter().collect();

        // BTreeMap so the snapshot is ordered and therefore diffable between
        // builds; an unordered snapshot would churn on every rebuild.
        let mut by_tool: BTreeMap<uuid::Uuid, css_lib::wire::ToolModuleTool> = BTreeMap::new();

        for m in modules {
            by_tool
                .entry(m.tool_id)
                .or_insert_with(|| css_lib::wire::ToolModuleTool {
                    tool_id: m.tool_id.to_string(),
                    external_id: external.get(&m.tool_id).cloned().flatten(),
                    modules: Vec::new(),
                    interlocks: Vec::new(),
                    power_fails_safe: false,
                })
                .modules
                .push(css_lib::wire::ToolModuleBinding {
                    id: m.id.to_string(),
                    device_id: m.device_id.to_string(),
                    role: m.role,
                    name: m.name,
                    params: m.params,
                    on_disconnect: m.on_disconnect,
                });
        }

        // A disabled rule is not enforced, so it is not sent. The edge decides
        // from what it holds; shipping a rule it must remember not to apply is a
        // second place for the enabled/disabled decision to be got wrong.
        for i in interlocks.into_iter().filter(|i| i.enabled) {
            by_tool
                .entry(i.tool_id)
                .or_insert_with(|| css_lib::wire::ToolModuleTool {
                    tool_id: i.tool_id.to_string(),
                    external_id: external.get(&i.tool_id).cloned().flatten(),
                    modules: Vec::new(),
                    interlocks: Vec::new(),
                    power_fails_safe: false,
                })
                .interlocks
                .push(css_lib::wire::ToolInterlockRule {
                    id: i.id.to_string(),
                    kind: i.kind,
                    condition: i.condition,
                    source_module_id: i.source_module_id.map(|s| s.to_string()),
                    debounce_ms: i.debounce_ms,
                    latch: i.latch,
                    reset: i.reset,
                    enforcement: i.enforcement,
                });
        }

        // Derived once the bindings are in place: whether the modules that
        // actually switch this tool can reach a safe state unaided.
        let mut tools: Vec<css_lib::wire::ToolModuleTool> = by_tool.into_values().collect();
        for t in tools.iter_mut() {
            let power: Vec<css_lib::capabilities::ModuleCapabilities> = t
                .modules
                .iter()
                .filter(|m| m.role == "power")
                .map(|m| css_lib::capabilities::ModuleCapabilities::from_params(&m.params))
                .collect();
            t.power_fails_safe = css_lib::capabilities::power_can_fail_safe(&power);
        }

        Ok(css_lib::wire::ToolModuleStatePayload {
            as_of: chrono::Utc::now().to_rfc3339(),
            tools,
        })
    }
}

// ── Power aggregation: the pure arithmetic behind the lockout (#44/#53) ───────
//
// These are the safety calculation split out from their DB methods so they can
// be unit-tested directly (the e2e `circuits` stage exercises the same math end
// to end, but a pure test is a cheaper, sharper oracle for the boundary and the
// blast radius). `circuit_draw_totals` and `evaluate_circuit_overages` are the
// only callers.

/// Sum each tool's latest draw onto its circuit via the
/// tool -> receptacle -> outlet -> circuit maps. Every circuit in `circuit_ids`
/// is present in the result (0 when idle); a tool that resolves to no circuit
/// contributes nowhere. Sorted by circuit id.
fn sum_draw_per_circuit(
    circuit_ids: &[uuid::Uuid],
    tool_draws: &[(uuid::Uuid, bigdecimal::BigDecimal)],
    tool_to_receptacle: &std::collections::HashMap<uuid::Uuid, uuid::Uuid>,
    recep_to_outlet: &std::collections::HashMap<uuid::Uuid, uuid::Uuid>,
    outlet_to_circuit: &std::collections::HashMap<uuid::Uuid, uuid::Uuid>,
) -> Vec<(uuid::Uuid, bigdecimal::BigDecimal)> {
    use bigdecimal::BigDecimal;
    use std::collections::HashMap;

    let mut totals: HashMap<uuid::Uuid, BigDecimal> = circuit_ids
        .iter()
        .map(|c| (*c, BigDecimal::from(0)))
        .collect();
    for (tool_id, draw) in tool_draws {
        let Some(rid) = tool_to_receptacle.get(tool_id) else {
            continue;
        };
        let Some(oid) = recep_to_outlet.get(rid) else {
            continue;
        };
        let Some(cid) = outlet_to_circuit.get(oid) else {
            continue;
        };
        let entry = totals.entry(*cid).or_insert_with(|| BigDecimal::from(0));
        *entry = entry.clone() + draw.clone();
    }
    let mut out: Vec<(uuid::Uuid, BigDecimal)> = totals.into_iter().collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Select the circuits that must trip: summed draw strictly greater than the
/// amperage limit and not already locked, as `(id, total, limit)`, preserving
/// the input order. Strictly `>` -- a circuit at exactly its rating is at
/// capacity, not overloaded. A circuit with no reported draw is treated as 0.
fn overloaded_circuits(
    circuits: &[(uuid::Uuid, bigdecimal::BigDecimal, bool)],
    totals: &[(uuid::Uuid, bigdecimal::BigDecimal)],
) -> Vec<(uuid::Uuid, bigdecimal::BigDecimal, bigdecimal::BigDecimal)> {
    use bigdecimal::BigDecimal;

    let mut out = Vec::new();
    for (id, limit, locked) in circuits {
        if *locked {
            continue;
        }
        let total = totals
            .iter()
            .find(|(cid, _)| cid == id)
            .map(|(_, t)| t.clone())
            .unwrap_or_else(|| BigDecimal::from(0));
        if &total > limit {
            out.push((*id, total, limit.clone()));
        }
    }
    out
}

#[cfg(test)]
mod overage_math_tests {
    use super::{overloaded_circuits, sum_draw_per_circuit};
    use bigdecimal::BigDecimal;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn bd(n: i64) -> BigDecimal {
        BigDecimal::from(n)
    }
    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn overloaded_trips_only_the_over_limit_unlocked_circuit() {
        let (c1, c2, c3) = (id(1), id(2), id(3));
        // c1 over (12 > 10); c2 exactly at rating (not over); c3 over but ALREADY
        // locked (must be skipped, not re-tripped).
        let circuits = vec![(c1, bd(10), false), (c2, bd(10), false), (c3, bd(10), true)];
        let totals = vec![(c1, bd(12)), (c2, bd(10)), (c3, bd(99))];
        let trip = overloaded_circuits(&circuits, &totals);
        // Blast radius: only c1.
        assert_eq!(
            trip.iter().map(|(i, _, _)| *i).collect::<Vec<_>>(),
            vec![c1]
        );
        assert_eq!(trip[0].1, bd(12), "surfaces the summed total");
        assert_eq!(trip[0].2, bd(10), "surfaces the limit");
    }

    #[test]
    fn the_strict_greater_than_boundary_holds_from_both_sides() {
        // Self-test the safety boundary: under and AT the rating do not trip;
        // one unit over does. A `>=` regression would fail the "at" case.
        let c = id(1);
        assert!(
            overloaded_circuits(&[(c, bd(10), false)], &[(c, bd(9))]).is_empty(),
            "under the rating"
        );
        assert!(
            overloaded_circuits(&[(c, bd(10), false)], &[(c, bd(10))]).is_empty(),
            "exactly at the rating is at capacity, not overloaded"
        );
        assert_eq!(
            overloaded_circuits(&[(c, bd(10), false)], &[(c, bd(11))]).len(),
            1,
            "one unit over the rating trips"
        );
    }

    #[test]
    fn a_circuit_with_no_reported_draw_defaults_to_zero_and_does_not_trip() {
        let c = id(1);
        assert!(overloaded_circuits(&[(c, bd(10), false)], &[]).is_empty());
    }

    #[test]
    fn draw_sums_onto_the_right_circuit_and_does_not_leak() {
        // Two tools on c1 (6+6=12), one on c2 (4); a tool with no receptacle
        // mapping contributes to nothing.
        let (c1, c2) = (id(1), id(2));
        let (t1, t2, t3, t4) = (id(11), id(12), id(13), id(14));
        let (r1, r2, r3) = (id(21), id(22), id(23));
        let (o1, o2) = (id(31), id(32));
        let tool_to_receptacle: HashMap<Uuid, Uuid> =
            [(t1, r1), (t2, r2), (t3, r3)].into_iter().collect();
        let recep_to_outlet: HashMap<Uuid, Uuid> =
            [(r1, o1), (r2, o1), (r3, o2)].into_iter().collect();
        let outlet_to_circuit: HashMap<Uuid, Uuid> = [(o1, c1), (o2, c2)].into_iter().collect();
        let tool_draws = vec![(t1, bd(6)), (t2, bd(6)), (t3, bd(4)), (t4, bd(99))];

        let totals = sum_draw_per_circuit(
            &[c1, c2],
            &tool_draws,
            &tool_to_receptacle,
            &recep_to_outlet,
            &outlet_to_circuit,
        );
        assert_eq!(totals, vec![(c1, bd(12)), (c2, bd(4))]);
    }

    #[test]
    fn every_circuit_is_present_even_when_idle() {
        let (c1, c2) = (id(1), id(2));
        let totals = sum_draw_per_circuit(
            &[c1, c2],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(totals, vec![(c1, bd(0)), (c2, bd(0))]);
    }
}
