use anyhow::{Result};
use diesel::prelude::*;
use diesel::pg::PgConnection;
use diesel::r2d2::{ConnectionManager, Pool, PoolError, PooledConnection};
use diesel::{Connection, RunQueryDsl, QueryDsl, ExpressionMethods, SelectableHelper};
use diesel_migrations::{embed_migrations, EmbeddedMigrations, MigrationHarness};
use std::fmt;
use tracing::{info, warn, error, debug};

use crate::config::DatabaseConfig;
use crate::schema::users;
use crate::models::{User, NewUser, UpdateUser};

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
}

impl DatabaseManager {
    /// Create a new database manager with connection pool
    pub fn new(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let database_url = config.get_url();
        
        info!("Initializing database connection pool");
        debug!("Database URL: {}", mask_password(&database_url));

        // Test database connectivity first
        info!("Testing database connectivity...");
        let _test_conn = PgConnection::establish(&database_url)
            .map_err(|e| {
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
            .connection_timeout(std::time::Duration::from_secs(config.connect_timeout_seconds))
            .idle_timeout(Some(std::time::Duration::from_secs(config.idle_timeout_seconds)))
            .build(manager)
            .map_err(|e| {
                error!("Failed to create database connection pool: {}", e);
                DatabaseError::Pool(e)
            })?;

        info!(
            "Database connection pool created successfully (min: {}, max: {})",
            config.min_connections, config.max_connections
        );

        Ok(Self { pool, webhook_tx: std::sync::OnceLock::new() })
    }

    /// Register the channel the webhook dispatcher listens on. Called once at
    /// startup after the dispatcher is created. Subsequent calls are ignored.
    pub fn set_webhook_sender(
        &self,
        tx: tokio::sync::mpsc::UnboundedSender<crate::models::AuditLog>,
    ) {
        let _ = self.webhook_tx.set(tx);
    }

    /// Get a connection from the pool
    pub fn get_connection(&self) -> Result<DbConnection, DatabaseError> {
        self.pool
            .get()
            .map_err(|e| {
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
        
        conn.run_pending_migrations(MIGRATIONS)
            .map_err(|e| {
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
pub async fn initialize_database(config: &DatabaseConfig) -> Result<DatabaseManager, DatabaseError> {
    let db_manager = DatabaseManager::new(config)?;

    // Run migrations if auto_migrate is enabled
    if config.auto_migrate {
        db_manager.run_migrations()?;
    }

    // Perform initial health check
    db_manager.health_check()?;

    let status = db_manager.pool_status();
    info!("Database initialization complete. {}", status);

    Ok(db_manager)
}

/// User-related database operations
impl DatabaseManager {
    /// Create a new user
    pub fn create_user(&self, new_user: &NewUser) -> Result<User, DatabaseError> {
        let mut conn = self.get_connection()?;

        diesel::insert_into(users::table)
            .values(new_user)
            .returning(User::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update a tool
    pub fn update_tool(&self, tool_id: uuid::Uuid, payload: &crate::api::tools::UpdateToolRequest) -> Result<crate::models::Tool, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        // Create a modified payload that includes the updated_at timestamp
        let update_payload = payload.clone();

        // Use the AsChangeset implementation to only update non-None fields
        diesel::update(tools.filter(id.eq(tool_id)))
            .set((
                &update_payload,
                updated_at.eq(chrono::Utc::now())
            ))
            .returning(crate::models::Tool::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update tool status
    pub fn update_tool_status(&self, tool_id: uuid::Uuid, new_status: &crate::models::ToolStatus) -> Result<crate::models::Tool, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(tools.filter(id.eq(tool_id)))
            .set((
                status.eq(new_status),
                updated_at.eq(chrono::Utc::now())
            ))
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
    pub fn find_user_by_profile_field(&self, field_name: &str, field_value: &str) -> Result<Option<User>, DatabaseError> {
        use diesel::dsl::sql;
        use diesel::sql_types::{Text, Bool};

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

    /// Update user information
    pub fn update_user(&self, user_id: uuid::Uuid, updates: &UpdateUser) -> Result<User, DatabaseError> {
        let mut conn = self.get_connection()?;

        // Set updated_at to current time if not explicitly provided
        let mut updates = updates.clone();
        if updates.updated_at.is_none() {
            updates.updated_at = Some(chrono::Utc::now().naive_utc());
        }
        
        let updated_user = diesel::update(users::table.filter(users::id.eq(user_id)))
            .set(&updates)
            .returning(User::as_returning())
            .get_result::<User>(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        
        Ok(updated_user)
    }

    /// Update user profile only
    pub fn update_user_profile(&self, user_id: uuid::Uuid, profile: &serde_json::Value) -> Result<User, DatabaseError> {
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

        diesel::delete(users::table.filter(users::id.eq(user_id)))
            .execute(&mut conn)
            .map(|_| ())
            .map_err(DatabaseError::Diesel)
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
    pub fn list_users_paginated(&self, limit: i64, offset: i64) -> Result<Vec<User>, DatabaseError> {
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
    pub fn is_user_trainer_for_tool(&self, user_id: uuid::Uuid, tool_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::tool_trainers;
        let mut conn = self.get_connection()?;

        let count: i64 = tool_trainers::table
            .filter(tool_trainers::user_id.eq(user_id))
            .filter(tool_trainers::tool_id.eq(tool_id))
            .filter(tool_trainers::is_active.eq(true))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(count > 0)
    }

    /// Check if a user is a trainer for any tool
    pub fn is_user_trainer_for_any_tool(&self, user_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::tool_trainers;
        let mut conn = self.get_connection()?;

        let count: i64 = tool_trainers::table
            .filter(tool_trainers::user_id.eq(user_id))
            .filter(tool_trainers::is_active.eq(true))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(count > 0)
    }

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
    pub fn get_audit_logs(&self, offset: i64, limit: i64, event_type_filter: Option<String>) -> Result<Vec<crate::models::AuditLog>, diesel::result::Error> {
        use crate::schema::audit_logs::dsl::*;

        let mut conn = self.pool.get().map_err(|_| diesel::result::Error::NotFound)?;

        let mut query = audit_logs
            .order(created_at.desc())
            .into_boxed();

        if let Some(event_filter) = event_type_filter {
            query = query.filter(event_type.eq(event_filter));
        }

        query
            .limit(limit)
            .offset(offset)
            .load::<crate::models::AuditLog>(&mut conn)
    }

    /// Create a new audit log entry
    pub fn create_audit_log(&self, new_audit_log: &crate::models::NewAuditLog) -> Result<crate::models::AuditLog, DatabaseError> {
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

        Ok(created)
    }

    /// Get tools with filtering
    pub fn get_tools(&self, query: crate::api::tools::ToolQuery) -> Result<Vec<crate::models::Tool>, DatabaseError> {
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
    pub fn get_tool_by_id(&self, tool_id: uuid::Uuid) -> Result<Option<crate::models::Tool>, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        tools
            .find(tool_id)
            .first::<crate::models::Tool>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Get a tool by external_id (for ToolGuard integration)
    pub fn get_tool_by_external_id(&self, external_id_value: &str) -> Result<Option<crate::models::Tool>, DatabaseError> {
        use crate::schema::tools::dsl::*;

        let mut conn = self.get_connection()?;

        tools
            .filter(external_id.eq(external_id_value))
            .first::<crate::models::Tool>(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }

    /// Create a new tool
    pub fn create_tool(&self, new_tool: &crate::models::NewTool) -> Result<crate::models::Tool, DatabaseError> {
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

        diesel::delete(tools.find(tool_id))
            .execute(&mut conn)
            .map(|_| ())
            .map_err(DatabaseError::Diesel)
    }

    /// Create a tool event
    pub fn create_tool_event(&self, new_event: &crate::models::NewToolEvent) -> Result<crate::models::ToolEvent, DatabaseError> {
        use crate::schema::tool_events;

        let mut conn = self.get_connection()?;

        diesel::insert_into(tool_events::table)
            .values(new_event)
            .returning(crate::models::ToolEvent::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }
    /// Get tool events
    pub fn get_tool_events(&self, _tool_id: uuid::Uuid) -> Result<Vec<crate::models::ToolEvent>, DatabaseError> {
        use crate::schema::tool_events::dsl::*;

        let mut conn = self.get_connection()?;

        tool_events
            .filter(tool_id.eq(_tool_id))
            .order(created_at.desc())
            .load::<crate::models::ToolEvent>(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Check if user has valid training for a tool
    pub fn user_has_valid_training(&self, user_id: uuid::Uuid, tool_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::{user_tool_training, tool_training_types};

        let mut conn = self.get_connection()?;

        let training_count: i64 = user_tool_training::table
            .inner_join(tool_training_types::table)
            .filter(user_tool_training::user_id.eq(user_id))
            .filter(tool_training_types::tool_id.eq(tool_id))
            .filter(
                user_tool_training::expires_at.is_null()
                    .or(user_tool_training::expires_at.gt(chrono::Utc::now()))
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
    pub fn user_has_completed_all_training_steps(&self, user_id: uuid::Uuid, tool_id: uuid::Uuid) -> Result<bool, DatabaseError> {
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
                .select((user_training_progress::status, user_training_progress::expires_at))
                .first::<(crate::models::TrainingStatus, Option<chrono::DateTime<chrono::Utc>>)>(&mut conn)
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

    // ==================== TRAINING SYSTEM DATABASE METHODS ====================

    /// Create a new training step
    pub fn create_training_step(&self, new_step: &crate::models::NewTrainingStep) -> Result<crate::models::TrainingStep, DatabaseError> {
        use crate::schema::training_steps;

        let mut conn = self.get_connection()?;

        diesel::insert_into(training_steps::table)
            .values(new_step)
            .returning(crate::models::TrainingStep::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get training steps with optional filtering
    pub fn get_training_steps(&self, query: &crate::api::training::TrainingQuery) -> Result<Vec<crate::models::TrainingStep>, DatabaseError> {
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
    pub fn get_training_step_by_id(&self, step_id: uuid::Uuid) -> Result<Option<crate::models::TrainingStep>, DatabaseError> {
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
    pub fn update_training_step(&self, step_id: uuid::Uuid, update_step: &crate::models::UpdateTrainingStep) -> Result<crate::models::TrainingStep, DatabaseError> {
        use crate::schema::training_steps::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(training_steps.filter(id.eq(step_id)))
            .set((
                update_step,
                updated_at.eq(chrono::Utc::now())
            ))
            .returning(crate::models::TrainingStep::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update training step position/order
    pub fn update_training_step_position(&self, step_id: uuid::Uuid, new_step_number: i32) -> Result<(), DatabaseError> {
        use crate::schema::training_steps::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(training_steps.filter(id.eq(step_id)))
            .set((
                step_number.eq(new_step_number),
                updated_at.eq(chrono::Utc::now())
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

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

        diesel::delete(training_steps.filter(id.eq(step_id)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(())
    }

    /// Add a training prerequisite
    pub fn add_training_prerequisite(&self, new_prereq: &crate::models::NewTrainingPrerequisite) -> Result<crate::models::TrainingPrerequisite, DatabaseError> {
        use crate::schema::training_prerequisites;

        let mut conn = self.get_connection()?;

        diesel::insert_into(training_prerequisites::table)
            .values(new_prereq)
            .returning(crate::models::TrainingPrerequisite::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get training prerequisites for a step
    pub fn get_training_prerequisites(&self, step_id: uuid::Uuid) -> Result<Vec<crate::models::TrainingStep>, DatabaseError> {
        use crate::schema::{training_prerequisites, training_steps};

        let mut conn = self.get_connection()?;

        let prerequisites = training_prerequisites::table
            .inner_join(training_steps::table.on(training_steps::id.eq(training_prerequisites::prerequisite_step_id)))
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

        diesel::delete(training_prerequisites.filter(id.eq(prereq_id)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(())
    }

    /// Get user training progress with filtering
    pub fn get_user_training_progress(&self, user_id_param: uuid::Uuid, query: &crate::api::training::TrainingQuery) -> Result<Vec<crate::models::UserTrainingProgress>, DatabaseError> {
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
    pub fn get_user_training_progress_for_step(&self, user_id_param: uuid::Uuid, step_id: uuid::Uuid) -> Result<Option<crate::models::UserTrainingProgress>, DatabaseError> {
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
    pub fn update_user_training_progress(&self, user_id_param: uuid::Uuid, step_id: uuid::Uuid, update_progress: &crate::models::UpdateUserTrainingProgress) -> Result<crate::models::UserTrainingProgress, DatabaseError> {
        use crate::schema::user_training_progress::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(
            user_training_progress
                .filter(user_id.eq(user_id_param))
                .filter(training_step_id.eq(step_id))
        )
        .set((
            update_progress,
            updated_at.eq(chrono::Utc::now())
        ))
        .returning(crate::models::UserTrainingProgress::as_returning())
        .get_result(&mut conn)
        .map_err(DatabaseError::Diesel)
    }

    /// Check if user is a certified instructor for a training step
    pub fn is_certified_instructor(&self, user_id: uuid::Uuid, step_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::training_instructors;

        let mut conn = self.get_connection()?;

        let count: i64 = training_instructors::table
            .filter(training_instructors::user_id.eq(user_id))
            .filter(training_instructors::training_step_id.eq(step_id))
            .filter(
                training_instructors::expires_at.is_null()
                    .or(training_instructors::expires_at.gt(chrono::Utc::now()))
            )
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(count > 0)
    }

    /// Certify a user as instructor
    pub fn certify_instructor(&self, new_instructor: &crate::models::NewTrainingInstructor) -> Result<crate::models::TrainingInstructor, DatabaseError> {
        use crate::schema::training_instructors;

        let mut conn = self.get_connection()?;

        diesel::insert_into(training_instructors::table)
            .values(new_instructor)
            .returning(crate::models::TrainingInstructor::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get training instructors with filtering
    pub fn get_training_instructors(&self, query: &crate::api::training::TrainingQuery) -> Result<Vec<crate::models::TrainingInstructor>, DatabaseError> {
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
    pub fn revoke_instructor_certification(&self, instructor_id: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::training_instructors::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::delete(training_instructors.filter(id.eq(instructor_id)))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(())
    }

    // ==================== TRAINERS DATABASE METHODS ====================

    /// Create a new training record
    pub fn create_training_record(&self, new_record: &crate::models::trainers::NewTrainingRecord) -> Result<crate::models::trainers::TrainingRecord, DatabaseError> {
        use crate::schema::{training_records, user_training_progress};

        let mut conn = self.get_connection()?;

        // Start a transaction to ensure both operations succeed or both fail
        conn.build_transaction().run::<_, DatabaseError, _>(|conn| {
            // Create the training record
            let training_record = diesel::insert_into(training_records::table)
                .values(new_record)
                .returning(crate::models::trainers::TrainingRecord::as_returning())
                .get_result(conn)
                .map_err(DatabaseError::Diesel)?;

            // Map training record completion status to training progress status
            let training_status = match new_record.completion_status.as_str() {
                "completed" => crate::models::TrainingStatus::Completed,
                "partial" => crate::models::TrainingStatus::InProgress,
                "failed" => crate::models::TrainingStatus::Failed,
                _ => crate::models::TrainingStatus::InProgress,
            };

            // If there's a specific training step, create progress for that step
            if let Some(step_id) = new_record.training_step_id {
                // Calculate expiry date for completed training
                let expires_at = if training_status == crate::models::TrainingStatus::Completed {
                    if let Some(step) = self.get_training_step_by_id(step_id)? {
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

                // Create or update user training progress
                let new_progress = crate::models::NewUserTrainingProgress {
                    user_id: new_record.trainee_user_id,
                    training_step_id: step_id,
                    status: Some(training_status.clone()),
                    instructor_id: Some(new_record.trainer_user_id),
                    started_at: Some(chrono::Utc::now()),
                    notes: new_record.notes.clone(),
                };

                diesel::insert_into(user_training_progress::table)
                    .values(&new_progress)
                    .on_conflict((user_training_progress::user_id, user_training_progress::training_step_id))
                    .do_update()
                    .set((
                        user_training_progress::status.eq(training_status),
                        user_training_progress::instructor_id.eq(new_record.trainer_user_id),
                        user_training_progress::completed_at.eq(completed_at),
                        user_training_progress::expires_at.eq(expires_at),
                        user_training_progress::notes.eq(&new_record.notes),
                        user_training_progress::updated_at.eq(chrono::Utc::now()),
                    ))
                    .execute(conn)
                    .map_err(DatabaseError::Diesel)?;
            } else {
                // If no specific training step is provided, we need to find or create
                // a general training step for this tool, or handle this case appropriately
                // For now, we'll log a warning and continue without creating progress
                warn!("Training record created without specific training_step_id for tool {}", new_record.tool_id);
            }

            Ok(training_record)
        })
    }

    /// Get training records with users information
    pub fn get_training_records_with_users(
        &self,
        query: &crate::api::trainers::TrainingRecordsQuery,
    ) -> Result<Vec<crate::models::trainers::TrainingRecordWithUsers>, DatabaseError> {
        use crate::schema::{training_records, users, tools};

        let mut conn = self.get_connection()?;

        // Build the query based on filters
        let mut diesel_query = training_records::table
            .inner_join(users::table.on(users::id.eq(training_records::trainee_user_id)))
            .inner_join(tools::table.on(tools::id.eq(training_records::tool_id)))
            .into_boxed();

        if let Some(tool_filter) = query.tool_id {
            diesel_query = diesel_query.filter(training_records::tool_id.eq(tool_filter));
        }

        if let Some(trainer_filter) = query.trainer_id {
            diesel_query = diesel_query.filter(training_records::trainer_user_id.eq(trainer_filter));
        }

        if let Some(trainee_filter) = query.trainee_id {
            diesel_query = diesel_query.filter(training_records::trainee_user_id.eq(trainee_filter));
        }

        // Execute the query with joins to get the data we need
        let query = diesel_query.select((
            crate::models::trainers::TrainingRecord::as_select(),
            crate::models::User::as_select(),
            crate::models::Tool::as_select(),
        ));

        let results: Vec<(crate::models::trainers::TrainingRecord, crate::models::User, crate::models::Tool)> = query
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // Transform the results to include the trainer names
        let mut records_with_users = Vec::new();

        for (record, trainee_user, tool) in results {
            // Get trainer info
            let trainer_user = users::table
                .filter(users::id.eq(record.trainer_user_id))
                .select(crate::models::User::as_select())
                .first::<crate::models::User>(&mut conn)
                .map_err(DatabaseError::Diesel)?;

            records_with_users.push(crate::models::trainers::TrainingRecordWithUsers {
                record,
                trainee_name: trainee_user.full_name,
                trainer_name: trainer_user.full_name,
                tool_name: tool.name,
            });
        }

        Ok(records_with_users)
    }

    /// Get training records for a specific user (either as trainer or trainee)
    pub fn get_user_training_records(&self, user_id_param: uuid::Uuid, as_trainer: bool) -> Result<Vec<crate::models::trainers::TrainingRecordWithUsers>, DatabaseError> {
        let query = crate::api::trainers::TrainingRecordsQuery {
            tool_id: None,
            trainer_id: if as_trainer { Some(user_id_param) } else { None },
            trainee_id: if !as_trainer { Some(user_id_param) } else { None },
            limit: Some(100),
            offset: Some(0),
        };
        
        self.get_training_records_with_users(&query)
    }

    /// Update a training record
    pub fn update_training_record(&self, record_id: uuid::Uuid, updates: &crate::models::trainers::UpdateTrainingRecord) -> Result<crate::models::trainers::TrainingRecord, DatabaseError> {
        use crate::schema::training_records::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(training_records.filter(id.eq(record_id)))
            .set((
                updates,
                updated_at.eq(chrono::Utc::now())
            ))
            .returning(crate::models::trainers::TrainingRecord::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Create a new tool trainer assignment
    pub fn create_tool_trainer(&self, new_trainer: &crate::models::trainers::NewToolTrainer) -> Result<crate::models::trainers::ToolTrainer, DatabaseError> {
        use crate::schema::tool_trainers;

        let mut conn = self.get_connection()?;

        diesel::insert_into(tool_trainers::table)
            .values(new_trainer)
            .returning(crate::models::trainers::ToolTrainer::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get tool trainers with optional filtering
    pub fn get_tool_trainers_list(&self, tool_id_param: Option<uuid::Uuid>, user_id_param: Option<uuid::Uuid>) -> Result<Vec<crate::models::trainers::ToolTrainer>, DatabaseError> {
        use crate::schema::tool_trainers::dsl::*;

        let mut conn = self.get_connection()?;
        let mut query = tool_trainers.into_boxed();

        if let Some(tool_filter) = tool_id_param {
            query = query.filter(tool_id.eq(tool_filter));
        }

        if let Some(user_filter) = user_id_param {
            query = query.filter(user_id.eq(user_filter));
        }

        query
            .filter(is_active.eq(true))
            .select(crate::models::trainers::ToolTrainer::as_select())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Update tool trainer assignment
    pub fn update_tool_trainer(&self, tool_id_param: uuid::Uuid, user_id_param: uuid::Uuid, updates: &crate::models::trainers::UpdateToolTrainer) -> Result<crate::models::trainers::ToolTrainer, DatabaseError> {
        use crate::schema::tool_trainers::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(tool_trainers
            .filter(tool_id.eq(tool_id_param))
            .filter(user_id.eq(user_id_param)))
            .set((
                updates,
                updated_at.eq(chrono::Utc::now())
            ))
            .returning(crate::models::trainers::ToolTrainer::as_returning())
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Remove tool trainer assignment (soft delete)
    pub fn remove_tool_trainer(&self, tool_id_param: uuid::Uuid, user_id_param: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::tool_trainers::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(tool_trainers
            .filter(tool_id.eq(tool_id_param))
            .filter(user_id.eq(user_id_param)))
            .set((
                is_active.eq(false),
                updated_at.eq(chrono::Utc::now())
            ))
            .execute(&mut conn)
            .map(|_| ())
            .map_err(DatabaseError::Diesel)
    }

    /// Remove tool trainer assignment by user and tool ID
    pub fn remove_tool_trainer_by_user_tool(&self, user_id: uuid::Uuid, tool_id: uuid::Uuid) -> Result<(), DatabaseError> {
        use crate::schema::tool_trainers::dsl::*;

        let mut conn = self.get_connection()?;

        diesel::update(tool_trainers
            .filter(user_id.eq(user_id))
            .filter(tool_id.eq(tool_id))
            .filter(is_active.eq(true))
        )
        .set((
            is_active.eq(false),
            updated_at.eq(chrono::Utc::now())
        ))
        .execute(&mut conn)
        .map(|_| ())
        .map_err(DatabaseError::Diesel)
    }

    /// Get training history for a specific tool with detailed information
    pub fn get_training_history_for_tool(&self, tool_id: uuid::Uuid, query: &crate::api::training::TrainingHistoryQuery) -> Result<Vec<crate::api::training::TrainingHistoryRecord>, DatabaseError> {
        use crate::schema::{training_records, users, tools, training_steps};
        let mut conn = self.get_connection()?;

        // Note: Diesel doesn't support joining the same table twice with different aliases easily,
        // so we'll do a simpler query and then enrich the data
        let results = training_records::table
            .filter(training_records::tool_id.eq(tool_id))
            .select((
                training_records::id,
                training_records::tool_id,
                training_records::training_step_id,
                training_records::trainee_user_id,
                training_records::trainer_user_id,
                training_records::training_date,
                training_records::completion_status,
                training_records::minutes_trained,
                training_records::notes,
                training_records::created_at,
                training_records::updated_at,
            ))
            .order_by(training_records::training_date.desc())
            .load::<(
                uuid::Uuid,  // record id
                uuid::Uuid,  // tool_id
                Option<uuid::Uuid>,  // training_step_id (nullable)
                uuid::Uuid,  // trainee_user_id
                uuid::Uuid,  // trainer_user_id
                chrono::NaiveDate,      // training_date
                String,      // completion_status
                Option<i32>, // minutes_trained
                Option<String>, // notes
                chrono::NaiveDateTime, // created_at
                chrono::NaiveDateTime, // updated_at
            )>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // Now we need to enrich this data with tool, user, and step information
        let mut history_records = Vec::new();

        for (id, tool_id, training_step_id, trainee_user_id, trainer_user_id, training_date, completion_status, minutes_trained, notes, created_at, updated_at) in results {
            // Get tool info
            let tool = tools::table
                .filter(tools::id.eq(tool_id))
                .select(tools::name)
                .first::<String>(&mut conn)
                .map_err(DatabaseError::Diesel)?;

            // Get trainee info
            let (trainee_name, trainee_email) = users::table
                .filter(users::id.eq(trainee_user_id))
                .select((users::full_name, users::email))
                .first::<(String, String)>(&mut conn)
                .map_err(DatabaseError::Diesel)?;

            // Get trainer info
            let (trainer_name, trainer_email) = users::table
                .filter(users::id.eq(trainer_user_id))
                .select((users::full_name, users::email))
                .first::<(String, String)>(&mut conn)
                .map_err(DatabaseError::Diesel)?;

            // Get training step info (handle nullable training_step_id)
            let (step_name, step_number, actual_training_step_id) = if let Some(step_id) = training_step_id {
                let (name, number) = training_steps::table
                    .filter(training_steps::id.eq(step_id))
                    .select((training_steps::step_name, training_steps::step_number))
                    .first::<(String, i32)>(&mut conn)
                    .map_err(DatabaseError::Diesel)?;
                (name, number, step_id)
            } else {
                ("General Training".to_string(), 0, uuid::Uuid::nil())
            };

            history_records.push(crate::api::training::TrainingHistoryRecord {
                id,
                tool_id,
                tool_name: tool,
                training_step_id: actual_training_step_id,
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
                minutes_trained,
                notes,
                created_at,
                updated_at,
            });
        }

        Ok(history_records)
    }

    /// Check if user can access a tool (all training complete and valid)
    pub fn can_access_tool(&self, user_id: uuid::Uuid, tool_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        // First check if tool requires training
        let tool = self.get_tool_by_id(tool_id)?
            .ok_or_else(|| DatabaseError::Other("Tool not found".to_string()))?;

        if !tool.requires_training {
            return Ok(true); // No training required
        }

        // Get all required training steps for this tool
        use crate::schema::training_steps;
        let mut conn = self.get_connection()?;

        let required_steps: Vec<uuid::Uuid> = training_steps::table
            .filter(training_steps::tool_id.eq(tool_id))
            .select(training_steps::id)
            .load::<uuid::Uuid>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // Check if user has completed all required steps with valid (non-expired) certifications
        for step_id in required_steps {
            // Check for any completed training for this step
            let has_completed_training = crate::schema::user_training_progress::table
                .filter(crate::schema::user_training_progress::user_id.eq(user_id))
                .filter(crate::schema::user_training_progress::training_step_id.eq(step_id))
                .count()
                .get_result::<i64>(&mut conn)
                .map_err(DatabaseError::Diesel)?
                > 0;

            if !has_completed_training {
                return Ok(false); // Missing training for this step
            }
        }

        Ok(true) // All required training is complete and valid
    }

    // Placeholder implementations for complex training methods
    // These would need full business logic implementation

    /// Get comprehensive training overview for a tool
    pub fn get_tool_training_overview(&self, tool_id: uuid::Uuid, user_id: uuid::Uuid) -> Result<crate::models::ToolTrainingOverview, DatabaseError> {
        // Implementation: Get tool and training steps
        let tool = self.get_tool_by_id(tool_id)?
            .ok_or_else(|| DatabaseError::Other("Tool not found".to_string()))?;

        // Get all training steps for this tool with user progress
        let steps = self.get_tool_training_steps_with_progress(tool_id, user_id)?;

        // Calculate overall progress
        let overall_progress = if steps.is_empty() {
            0.0_f32
        } else {
            let completed_steps = steps.iter().filter(|s| {
                s.user_progress.as_ref().map_or(false, |p| p.status == crate::models::TrainingStatus::Completed)
            }).count();
            // Convert to percentage (0.25 becomes 25.0)
            (completed_steps as f32 / steps.len() as f32) * 100.0
        };

        // Find next step to complete - any step that's available and not completed
        let next_step = steps.iter()
            .find(|s| {
                // Step is available and either has no progress or progress is not completed
                s.is_available && 
                s.user_progress.as_ref().map_or(true, |p| p.status != crate::models::TrainingStatus::Completed)
            })
            .map(|s| s.step.clone());

        Ok(crate::models::ToolTrainingOverview {
            tool_id,
            tool_name: tool.name,
            steps,
            overall_progress,
            can_access_tool: self.can_access_tool(user_id, tool_id)?,
            next_step,
        })
    }

    /// Get tool training steps with user progress
    pub fn get_tool_training_steps_with_progress(&self, tool_id_param: uuid::Uuid, user_id_param: uuid::Uuid) -> Result<Vec<crate::models::TrainingStepWithProgress>, DatabaseError> {
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
            let user_progress: Option<crate::models::UserTrainingProgress> = user_training_progress::table
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
                step: step,
                prerequisites: prerequisites,
                user_progress: user_progress,
                is_available: is_available,
                instructor_required: instructor_required,
            });
        }
        
        Ok(result)
    }

    /// Check if user can start a training step (no prerequisites - all steps available)
    pub fn can_start_training_step(&self, _user_id: uuid::Uuid, _step_id: uuid::Uuid) -> Result<bool, DatabaseError> {
        // Prerequisites concept removed - all training steps are available by default
        // Users can start any training step regardless of other completed steps
        Ok(true)
    }

    /// Start a training session
    pub fn start_training_session(&self, user_id_param: uuid::Uuid, request: &crate::models::StartTrainingRequest) -> Result<crate::models::UserTrainingProgress, DatabaseError> {
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
            .on_conflict((user_training_progress::user_id, user_training_progress::training_step_id))
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
    pub fn complete_training_session(&self, user_id_param: uuid::Uuid, request: &crate::models::CompleteTrainingRequest) -> Result<crate::models::UserTrainingProgress, DatabaseError> {
        use crate::schema::user_training_progress;

        let mut conn = self.get_connection()?;

        let final_status = if request.passed {
            crate::models::TrainingStatus::Completed
        } else {
            crate::models::TrainingStatus::Failed
        };

        // Calculate expiry date if training was passed
        let expires_at = if request.passed {
            // Get the training step to check expiry days
            if let Some(step) = self.get_training_step_by_id(request.training_step_id)? {
                step.calculate_expiry_date()
            } else {
                None
            }
        } else {
            None
        };

        let res = diesel::update(
            user_training_progress::table
                .filter(user_training_progress::user_id.eq(user_id_param))
                .filter(user_training_progress::training_step_id.eq(request.training_step_id))
        )
        .set((
            user_training_progress::status.eq(final_status),
            user_training_progress::completed_at.eq(chrono::Utc::now()),
            user_training_progress::expires_at.eq(expires_at),
            user_training_progress::assessment_score.eq(request.assessment_score),
            user_training_progress::notes.eq(&request.notes),
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
    pub fn assign_tool_trainer(&self, new_trainer: &crate::models::NewToolTrainer) -> Result<crate::models::trainers::ToolTrainer, DatabaseError> {
        use crate::schema::tool_trainers::dsl::*;
        let mut conn = self.get_connection()?;

        diesel::insert_into(tool_trainers)
            .values(new_trainer)
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)
    }

    /// Get all active trainers for a specific tool
    pub fn get_tool_trainers(&self, tool_id_param: uuid::Uuid, include_inactive: bool) -> Result<Vec<crate::models::trainers::ToolTrainerWithUser>, DatabaseError> {
        use crate::schema::{tool_trainers, users};
        let mut conn = self.get_connection()?;

        let mut query = tool_trainers::table
            .inner_join(users::table.on(tool_trainers::user_id.eq(users::id)))
            .filter(tool_trainers::tool_id.eq(tool_id_param))
            .into_boxed();

        if !include_inactive {
            query = query.filter(tool_trainers::is_active.eq(true));
        }

        let results: Vec<(crate::models::trainers::ToolTrainer, crate::models::User)> = query
            .select((crate::models::trainers::ToolTrainer::as_select(), crate::models::User::as_select()))
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        Ok(results.into_iter().map(|(trainer, user)| {
            crate::models::trainers::ToolTrainerWithUser {
                trainer,
                user_name: user.username,
                user_email: user.email,
                user_full_name: Some(user.full_name),
            }
        }).collect())
    }

    /// Get a specific tool trainer assignment
    pub fn get_tool_trainer(&self, tool_id_param: uuid::Uuid, user_id_param: uuid::Uuid) -> Result<Option<crate::models::trainers::ToolTrainer>, DatabaseError> {
        use crate::schema::tool_trainers::dsl::*;
        let mut conn = self.get_connection()?;

        tool_trainers
            .filter(tool_id.eq(tool_id_param))
            .filter(user_id.eq(user_id_param))
            .select(crate::models::trainers::ToolTrainer::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)
    }





    /// Check if a user is an active trainer for a specific tool
    pub fn is_active_tool_trainer(&self, tool_id_param: uuid::Uuid, user_id_param: uuid::Uuid) -> Result<bool, DatabaseError> {
        use crate::schema::tool_trainers::dsl::*;
        let mut conn = self.get_connection()?;

        let trainer = tool_trainers
            .filter(tool_id.eq(tool_id_param))
            .filter(user_id.eq(user_id_param))
            .filter(is_active.eq(true))
            .select(crate::models::trainers::ToolTrainer::as_select())
            .first(&mut conn)
            .optional()
            .map_err(DatabaseError::Diesel)?;

        Ok(trainer.map_or(false, |t| t.is_currently_active()))
    }



    /// Get training records with optional filters
    pub fn get_training_records(
        &self, 
        tool_id_filter: Option<uuid::Uuid>,
        trainer_id_filter: Option<uuid::Uuid>,
        trainee_id_filter: Option<uuid::Uuid>,
        limit: Option<i64>,
        offset: Option<i64>
    ) -> Result<Vec<crate::models::trainers::TrainingRecordWithUsers>, DatabaseError> {
        use crate::schema::{training_records, users, tools};
        let mut conn = self.get_connection()?;

        let mut query = training_records::table
            .inner_join(users::table.on(training_records::trainee_user_id.eq(users::id)))
            .inner_join(tools::table.on(training_records::tool_id.eq(tools::id)))
            .into_boxed();

        // Apply filters
        if let Some(tool_id) = tool_id_filter {
            query = query.filter(training_records::tool_id.eq(tool_id));
        }
        if let Some(trainer_id) = trainer_id_filter {
            query = query.filter(training_records::trainer_user_id.eq(trainer_id));
        }
        if let Some(trainee_id) = trainee_id_filter {
            query = query.filter(training_records::trainee_user_id.eq(trainee_id));
        }

        // Apply pagination
        if let Some(limit_val) = limit {
            query = query.limit(limit_val);
        }
        if let Some(offset_val) = offset {
            query = query.offset(offset_val);
        }

        let results: Vec<(crate::models::trainers::TrainingRecord, crate::models::User, crate::models::Tool)> = query
            .select((
                crate::models::trainers::TrainingRecord::as_select(),
                crate::models::User::as_select(),
                crate::models::Tool::as_select()
            ))
            .order_by(training_records::training_date.desc())
            .load(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // Get trainer names for each record
        let mut records_with_users = Vec::new();
        for (record, trainee_user, tool) in results {
            let trainer_user = self.find_user_by_id(record.trainer_user_id)?
                .ok_or_else(|| DatabaseError::Other("Trainer user not found".to_string()))?;

            records_with_users.push(crate::models::trainers::TrainingRecordWithUsers {
                record,
                trainee_name: trainee_user.full_name,
                trainer_name: trainer_user.full_name,
                tool_name: tool.name,
            });
        }

        Ok(records_with_users)
    }

    // ── ToolGuard ────────────────────────────────────────────────────────────

    /// Look up a device by its auth token.
    /// Returns (device_id, token) if found.
    pub fn find_device_by_auth_token(&self, token: &str) -> Result<Option<(uuid::Uuid, String)>, DatabaseError> {
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

    /// Build the ToolGuard sync data: a flat tool list and per-user authorized tool ID lists.
    pub fn get_toolguard_sync_data(
        &self,
        profile_field: &str,
    ) -> Result<(Vec<crate::api::toolguard::ToolGuardSyncUser>, Vec<crate::api::toolguard::ToolGuardSyncTool>), DatabaseError> {
        use crate::schema::{tools, users};
        use crate::api::toolguard::{ToolGuardSyncTool, ToolGuardSyncUser};

        let mut conn = self.get_connection()?;

        // Load all users
        let all_users = users::table
            .select(crate::models::User::as_select())
            .load::<crate::models::User>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        // Load all tools
        let all_tools = tools::table
            .select(crate::models::Tool::as_select())
            .load::<crate::models::Tool>(&mut conn)
            .map_err(DatabaseError::Diesel)?;

        let mut sync_users = Vec::new();
        // Track which tools appear in at least one user's authorized list
        let mut authorized_tool_ids_set: std::collections::HashSet<uuid::Uuid> = std::collections::HashSet::new();

        for user in &all_users {
            // The profile field may be either a scalar string (one identifier)
            // or an array of strings (many identifiers per user). Empty/missing
            // values are skipped.
            let identifiers: Vec<String> = match user.profile.get(profile_field) {
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

            if identifiers.is_empty() {
                continue;
            }

            // Authorization is per-user, so compute it once and reuse across
            // every identifier this user exposes.
            let mut authorized_tool_ids = Vec::new();
            for tool in &all_tools {
                let has_steps = self.tool_has_training_steps(tool.id)?;
                let authorized = if has_steps {
                    self.user_has_completed_all_training_steps(user.id, tool.id)?
                } else {
                    true
                };

                if authorized {
                    authorized_tool_ids.push(tool.id);
                    authorized_tool_ids_set.insert(tool.id);
                }
            }

            for profile_field_value in identifiers {
                sync_users.push(ToolGuardSyncUser {
                    profile_field_value,
                    full_name: user.full_name.clone(),
                    is_active: user.is_active,
                    authorized_tool_ids: authorized_tool_ids.clone(),
                });
            }
        }

        // Build the top-level tools list from all tools that appear in any user's authorized set
        let sync_tools = all_tools.iter()
            .filter(|t| authorized_tool_ids_set.contains(&t.id))
            .map(|t| ToolGuardSyncTool {
                id: t.id,
                external_id: t.external_id.clone(),
                name: t.name.clone(),
                status: t.status.clone(),
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
        assert_eq!(url, "postgresql://test_user:test_pass@localhost:5432/test_db");
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
                    webhook_auth_links::table
                        .filter(webhook_auth_links::webhook_id.eq(webhook_id)),
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
        diesel::update(user_mfa_totp.filter(user_id.eq(uid)))
            .set((
                confirmed_at.eq(Some(chrono::Utc::now())),
                updated_at.eq(chrono::Utc::now()),
            ))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
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
        diesel::delete(user_mfa_webauthn.filter(id.eq(row_id)).filter(user_id.eq(uid)))
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

    pub fn mark_recovery_code_used(
        &self,
        code_id: uuid::Uuid,
    ) -> Result<(), DatabaseError> {
        use crate::schema::user_mfa_recovery_codes::dsl::*;
        let mut conn = self.get_connection()?;
        diesel::update(user_mfa_recovery_codes.filter(id.eq(code_id)))
            .set(used_at.eq(Some(chrono::Utc::now())))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(())
    }

    pub fn count_unused_recovery_codes(
        &self,
        uid: uuid::Uuid,
    ) -> Result<i64, DatabaseError> {
        use crate::schema::user_mfa_recovery_codes::dsl::*;
        let mut conn = self.get_connection()?;
        user_mfa_recovery_codes
            .filter(user_id.eq(uid))
            .filter(used_at.is_null())
            .count()
            .get_result(&mut conn)
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
        diesel::update(users.filter(id.eq(uid)))
            .set(mfa_enrolled_at.eq(when))
            .execute(&mut conn)
            .map_err(DatabaseError::Diesel)?;
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
                .set(crate::schema::users::mfa_enrolled_at.eq::<Option<chrono::DateTime<chrono::Utc>>>(None))
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
        self.set_user_mfa_enrolled(uid, if enrolled { Some(chrono::Utc::now()) } else { None })?;
        Ok(enrolled)
    }
}
