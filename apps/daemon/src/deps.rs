//! Application dependency container.
//!
//! [`AppDeps`] holds shared state passed to all request handlers via
//! Axum's `State` extractor.

use std::sync::Arc;

use tokio::sync::watch;

use crate::config::Config;
use crate::db::sqlite::SqliteDb;
use crate::error::Result;
use crate::events::EventBus;
use crate::process::output_manager::OutputManager;
use crate::process::pty_manager::PtyManager;

/// Shared application dependencies, cheaply cloneable via [`Arc`].
#[derive(Clone, Debug)]
#[allow(dead_code)] // Fields are used by later tasks.
pub struct AppDeps {
    pub config: Arc<Config>,
    pub db: Arc<SqliteDb>,
    pub event_bus: Arc<EventBus>,
    pub pty_manager: Arc<PtyManager>,
    pub output_manager: Arc<OutputManager>,
    pub shutdown_tx: Arc<watch::Sender<bool>>,
    pub shutdown_rx: watch::Receiver<bool>,
}

impl AppDeps {
    /// Build all dependencies from the resolved configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or database connection fails.
    pub async fn new(config: Config) -> Result<Self> {
        config.ensure_dirs()?;

        let logs_dir = config.logs_dir.clone();
        let db = SqliteDb::connect(&config.db_path).await?;
        let event_bus = EventBus::new(1024);
        let pty_manager = PtyManager::new();
        let output_manager = OutputManager::new(logs_dir);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        Ok(Self {
            config: Arc::new(config),
            db: Arc::new(db),
            event_bus: Arc::new(event_bus),
            pty_manager: Arc::new(pty_manager),
            output_manager: Arc::new(output_manager),
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
        })
    }
}
