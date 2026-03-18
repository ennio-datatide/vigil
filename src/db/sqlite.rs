//! `SQLite` database connection and pool management.
//!
//! Handles connecting with WAL mode and running migrations.

use std::path::Path;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

use crate::error::{DbError, Result};

/// Wrapper around a [`SqlitePool`] with migration support.
#[derive(Clone, Debug)]
pub struct SqliteDb {
    pool: SqlitePool,
}

impl SqliteDb {
    /// Connect to (or create) the database at `path` with WAL mode and normal sync.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection or migrations fail.
    pub async fn connect(path: &Path) -> Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(DbError::from)?;

        let db = Self { pool };
        db.run_migrations().await?;

        Ok(db)
    }

    /// Run embedded migrations from the `./migrations` directory.
    async fn run_migrations(&self) -> Result<()> {
        sqlx::migrate!("./migrations")
            .run(&self.pool)
            .await
            .map_err(DbError::from)?;

        Ok(())
    }

    /// Access the underlying connection pool.
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}
