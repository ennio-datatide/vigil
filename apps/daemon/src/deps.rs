//! Application dependency container.
//!
//! [`AppDeps`] holds shared state passed to all request handlers via
//! Axum's `State` extractor.

use std::sync::Arc;

use tokio::sync::watch;

use crate::config::Config;
use crate::db::kv::KvStore;
use crate::db::lance::LanceDb;
use crate::db::sqlite::SqliteDb;
use crate::error::Result;
use crate::events::EventBus;
use crate::process::output_manager::OutputManager;
use crate::process::pty_manager::PtyManager;
use crate::services::escalation::EscalationService;
use crate::services::memory_search::MemorySearch;
use crate::services::memory_store::MemoryStore;
use crate::services::sub_session::SubSessionService;
use crate::services::vigil::VigilService;
use crate::services::vigil_chat::VigilChatStore;

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
    /// Sub-session spawning service.
    pub sub_session_service: SubSessionService,
    /// Key-value store (redb).
    pub kv: KvStore,
    /// Vigil (per-project overseer) service.
    pub vigil_service: Arc<VigilService>,
    /// Vigil chat history persistence.
    pub vigil_chat_store: VigilChatStore,
    /// Blocker escalation timer service.
    pub escalation_service: EscalationService,
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
            .field("sub_session_service", &"SubSessionService { .. }")
            .field("kv", &"KvStore { .. }")
            .field("vigil_service", &"VigilService { .. }")
            .field("vigil_chat_store", &"VigilChatStore { .. }")
            .field("escalation_service", &"EscalationService { .. }")
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

        let kv = KvStore::open(&config.kv_path)?;

        let db = Arc::new(db);
        let event_bus = Arc::new(event_bus);
        let memory_store = MemoryStore::new(Arc::clone(&db), lance.clone());
        let memory_search = MemorySearch::new(Arc::clone(&db), lance.clone());
        let sub_session_service =
            SubSessionService::new(Arc::clone(&db), Arc::clone(&event_bus));
        let vigil_chat_store = VigilChatStore::new(Arc::clone(&db));
        let vigil_service = Arc::new(VigilService::new(
            Arc::clone(&event_bus),
            Arc::clone(&db),
            memory_store.clone(),
            memory_search.clone(),
            kv.clone(),
            sub_session_service.clone(),
        ));
        let escalation_service = EscalationService::with_default_timeout(Arc::clone(&event_bus));

        Ok(Self {
            config: Arc::new(config),
            db,
            event_bus,
            pty_manager: Arc::new(pty_manager),
            output_manager: Arc::new(output_manager),
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
            lance,
            memory_store,
            memory_search,
            sub_session_service,
            kv,
            vigil_service,
            vigil_chat_store,
            escalation_service,
        })
    }
}
