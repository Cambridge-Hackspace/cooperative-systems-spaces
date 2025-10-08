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

        Ok(Self { pool })
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
            .filter(users::is_active.eq(true))
            .count()
            .get_result(&mut conn)
            .map_err(DatabaseError::Diesel)?;
        Ok(count)
    }

    /// List users with pagination (alias for compatibility)
    pub fn list_users_paginated(&self, limit: i64, offset: i64) -> Result<Vec<User>, DatabaseError> {
        self.list_users(limit, offset)
    }

    /// Count total users
    pub fn count_active_users(&self) -> Result<i64, DatabaseError> {
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
}

/// Helper function to mask database passwords in URLs for logging
fn mask_password(url: &str) -> String {
    url.split('@').nth(1).map_or_else(
        || url.to_string(),
        |host_part| format!("postgresql://***:***@{}", host_part)
    )
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
