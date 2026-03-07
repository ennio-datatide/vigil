//! Daemon configuration resolution.
//!
//! Reads settings from `~/.praefectus/config.json` and resolves paths
//! for the data directory, database, and logs.

use std::path::PathBuf;

use crate::error::{ConfigError, Result};

/// Resolved daemon configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Port the HTTP server listens on.
    pub port: u16,
    /// Root directory for all daemon data (`~/.praefectus`).
    pub home: PathBuf,
    /// Path to the `SQLite` database file.
    pub database_path: PathBuf,
    /// Directory for log files.
    pub log_dir: PathBuf,
    /// Optional bearer token for API authentication.
    pub auth_token: Option<String>,
}

impl Config {
    /// Resolve configuration for the given port.
    ///
    /// Reads `~/.praefectus/config.json` if it exists, otherwise uses defaults.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn resolve(port: u16) -> Result<Self> {
        let home = dirs::home_dir()
            .map(|h| h.join(".praefectus"))
            .ok_or_else(|| ConfigError::Invalid("cannot determine home directory".into()))?;

        let database_path = home.join("praefectus.db");
        let log_dir = home.join("logs");

        let auth_token = std::env::var("PRAEFECTUS_AUTH_TOKEN").ok();

        Ok(Self {
            port,
            home,
            database_path,
            log_dir,
            auth_token,
        })
    }

    /// Create all required directories under the daemon home.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [&self.home, &self.log_dir] {
            std::fs::create_dir_all(dir).map_err(ConfigError::CreateDir)?;
        }
        Ok(())
    }
}
