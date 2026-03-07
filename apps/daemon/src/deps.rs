//! Application dependency container.
//!
//! [`AppDeps`] holds shared state passed to all request handlers via
//! Axum's `State` extractor.

use std::sync::Arc;

use tokio::sync::watch;

use crate::config::Config;
use crate::db::lance::LanceDb;
use crate::db::sqlite::SqliteDb;
use crate::error::Result;
use crate::events::EventBus;
use crate::process::output_manager::OutputManager;
use crate::process::pty_manager::PtyManager;
use crate::services::memory_search::MemorySearch;
use crate::services::memory_store::MemoryStore;

/// Shared application dependencies, cheaply cloneable via [`Arc`].
#[derive(Clone)]
#[allow(dead_code)] // Fields are used by later tasks.
pub struct AppDeps {
    pub config: Arc<Config>,
    pub db: Arc<SqliteDb>,
    pub event_bus: Arc<EventBus>,
    pub pty_manager: Arc<PtyManager>,
    pub output_manager: Arc<OutputManager>,
    pub shutdown_tx: Arc<watch::Sender<bool>>,
    pub shutdown_rx: watch::Receiver<bool>,
    /// `LanceDB` vector store.
    pub lance: LanceDb,
    /// Memory persistence service.
    pub memory_store: MemoryStore,
    /// Hybrid memory search service.
    pub memory_search: MemorySearch,
}

impl std::fmt::Debug for AppDeps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppDeps")
            .field("config", &self.config)
            .field("db", &"SqliteDb { .. }")
            .field("event_bus", &"EventBus { .. }")
            .field("pty_manager", &"PtyManager { .. }")
            .field("output_manager", &"OutputManager { .. }")
            .field("shutdown_tx", &"Sender { .. }")
            .field("shutdown_rx", &"Receiver { .. }")
            .field("lance", &"LanceDb { .. }")
            .field("memory_store", &"MemoryStore { .. }")
            .field("memory_search", &"MemorySearch { .. }")
            .finish()
    }
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
        let lance = LanceDb::connect(&config.lance_dir).await?;
        let event_bus = EventBus::new(1024);
        let pty_manager = PtyManager::new();
        let output_manager = OutputManager::new(logs_dir);
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        let db = Arc::new(db);
        let memory_store = MemoryStore::new(Arc::clone(&db), lance.clone());
        let memory_search = MemorySearch::new(Arc::clone(&db), lance.clone());

        Ok(Self {
            config: Arc::new(config),
            db,
            event_bus: Arc::new(event_bus),
            pty_manager: Arc::new(pty_manager),
            output_manager: Arc::new(output_manager),
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
            lance,
            memory_store,
            memory_search,
        })
    }
}
