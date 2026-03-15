//! Praefectus daemon library crate.
//!
//! Provides the core server logic: configuration, database, event bus,
//! HTTP API, and service layer.

pub mod cli;
pub mod mcp;
pub(crate) mod api;
pub(crate) mod config;
pub(crate) mod db;
pub(crate) mod deps;
pub(crate) mod events;
pub(crate) mod hooks;
pub(crate) mod process;
pub(crate) mod services;

mod error;

#[cfg(test)]
mod e2e;

pub use error::{Error, Result};

use std::net::SocketAddr;

use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::deps::AppDeps;

/// Boot the daemon server on the given port.
///
/// # Errors
///
/// Returns an error if configuration resolution, database connection,
/// or the TCP listener fails.
pub async fn run(port: u16) -> Result<()> {
    init_tracing();

    tracing::info!(port, "starting praefectus daemon");

    let config = Config::resolve(port)?;
    let deps = AppDeps::new(config).await?;

    // Run recovery -- mark orphaned sessions from a previous crash.
    let recovery = services::recovery::RecoveryService::new(&deps);
    if let Err(e) = recovery.run().await {
        tracing::error!(error = %e, "recovery failed");
    }

    // Start background services.
    let session_mgr = services::session_manager::SessionManager::new(&deps);
    let session_mgr_handle = session_mgr.start();

    let notifier = services::notifier::TelegramNotifier::new(&deps);
    let notifier_handle = notifier.start();

    let cleanup = services::cleanup::CleanupService::new(&deps);
    let cleanup_handle = cleanup.start();

    let memory_decay = services::memory_decay::MemoryDecayService::new(&deps);
    let memory_decay_handle = memory_decay.start();

    let vigil_handle = deps.vigil_service.clone().start();

    let telegram_poller = services::telegram_poller::TelegramPoller::new(&deps);
    let telegram_poller_handle = telegram_poller.start();

    // Handle ctrl+c by triggering the shutdown channel.
    let shutdown_tx = deps.shutdown_tx.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("ctrl+c received");
        let _ = shutdown_tx.send(true);
    });

    // Build router and start HTTP server.
    let router = api::router(deps.clone());

    let address = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(address)
        .await
        .map_err(|error| anyhow::anyhow!("failed to bind to {address}: {error}"))?;

    tracing::info!(%address, "listening");

    let mut shutdown_rx = deps.shutdown_rx.clone();

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.wait_for(|&shutdown| shutdown).await;
            tracing::info!("shutdown signal received");
        })
        .await
        .map_err(|error| anyhow::anyhow!("server error: {error}"))?;

    // Cleanup background tasks.
    session_mgr_handle.abort();
    notifier_handle.abort();
    cleanup_handle.abort();
    memory_decay_handle.abort();
    vigil_handle.abort();
    telegram_poller_handle.abort();

    tracing::info!("praefectus daemon stopped");
    Ok(())
}

/// Initialise the tracing subscriber with env-filter support.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("praefectus_daemon=info,tower_http=info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();
}
