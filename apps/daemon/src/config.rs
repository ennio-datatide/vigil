//! Daemon configuration resolution.
//!
//! Reads settings from `~/.praefectus/config.json` and resolves paths
//! for the data directory, database, and logs.

use std::path::PathBuf;

use crate::error::{ConfigError, Result};

/// Resolved daemon configuration.
#[derive(Clone, Debug)]
#[allow(dead_code)] // Fields are used by later tasks.
pub struct Config {
    /// Port the HTTP server listens on.
    pub server_port: u16,
    /// Port for the web dashboard.
    pub web_port: u16,
    /// Root directory for all daemon data (`~/.praefectus`).
    pub praefectus_home: PathBuf,
    /// Path to the `SQLite` database file.
    pub db_path: PathBuf,
    /// Directory for log files.
    pub logs_dir: PathBuf,
    /// Directory for skill definitions.
    pub skills_dir: PathBuf,
    /// Path to the PID file.
    pub pid_file: PathBuf,
    /// Base directory for git worktrees.
    pub worktree_base: PathBuf,
    /// Optional bearer token for API authentication.
    pub api_token: Option<String>,
    /// Optional dashboard URL override.
    pub dashboard_url: Option<String>,
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
        let praefectus_home = dirs::home_dir()
            .map(|h| h.join(".praefectus"))
            .ok_or_else(|| ConfigError::Invalid("cannot determine home directory".into()))?;

        let db_path = praefectus_home.join("praefectus.db");
        let logs_dir = praefectus_home.join("logs");
        let skills_dir = praefectus_home.join("skills");
        let pid_file = praefectus_home.join("daemon.pid");
        let worktree_base = praefectus_home.join("worktrees");

        let api_token = std::env::var("PRAEFECTUS_AUTH_TOKEN").ok();
        let dashboard_url = std::env::var("PRAEFECTUS_DASHBOARD_URL").ok();

        Ok(Self {
            server_port: port,
            web_port: 3000,
            praefectus_home,
            db_path,
            logs_dir,
            skills_dir,
            pid_file,
            worktree_base,
            api_token,
            dashboard_url,
        })
    }

    /// Build a config rooted in `base` for use in tests.
    ///
    /// All paths are derived from `base` so each test gets an isolated
    /// filesystem without touching the real `~/.praefectus`.
    #[cfg(test)]
    #[must_use]
    pub fn for_testing(base: &std::path::Path) -> Self {
        Self {
            server_port: 0,
            web_port: 0,
            praefectus_home: base.to_path_buf(),
            db_path: base.join("test.db"),
            logs_dir: base.join("logs"),
            skills_dir: base.join("skills"),
            pid_file: base.join("daemon.pid"),
            worktree_base: base.join("worktrees"),
            api_token: None,
            dashboard_url: None,
        }
    }

    /// Create all required directories under the daemon home.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation fails.
    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [&self.praefectus_home, &self.logs_dir, &self.skills_dir, &self.worktree_base] {
            std::fs::create_dir_all(dir).map_err(ConfigError::CreateDir)?;
        }
        Ok(())
    }
}
