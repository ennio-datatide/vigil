//! Praefectus daemon library crate.
//!
//! Provides the core server logic: configuration, database, event bus,
//! HTTP API, and service layer.

pub mod api;
pub mod config;
pub mod db;
pub mod deps;
pub mod error;
pub mod events;
pub mod hooks;
pub mod process;
pub mod services;

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
pub async fn run(port: u16) -> error::Result<()> {
    init_tracing();

    tracing::info!(port, "starting praefectus daemon");

    let config = Config::resolve(port)?;
    let deps = AppDeps::new(config).await?;
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
