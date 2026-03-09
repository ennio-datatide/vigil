//! Business logic services.
//!
//! Will contain session management, pipeline execution, notifications,
//! and other domain services.

pub(crate) mod cleanup;
pub(crate) mod escalation;
pub(crate) mod lictor;
pub(crate) mod memory_decay;
pub(crate) mod memory_search;
pub(crate) mod memory_store;
pub(crate) mod notification_store;
pub(crate) mod notifier;
pub(crate) mod pipeline_execution_store;
pub(crate) mod pipeline_runner;
pub(crate) mod pipeline_store;
pub(crate) mod project_store;
pub(crate) mod recovery;
pub(crate) mod session_manager;
pub(crate) mod session_store;
pub(crate) mod settings_store;
pub(crate) mod sub_session;
pub(crate) mod telegram_poller;
pub(crate) mod vigil;
pub(crate) mod vigil_chat;
