//! MCP server for Vigil tools.
//!
//! Exposes the 6 Vigil tools (memory_recall, memory_save, memory_delete,
//! session_recall, acta_update, spawn_worker) via the Model Context Protocol
//! stdio transport. Launched as a subprocess by the `claude` CLI.

use anyhow::Result;

/// Run the MCP server over stdin/stdout.
pub async fn run_mcp_server(daemon_url: String) -> Result<()> {
    tracing::info!(daemon_url, "starting Vigil MCP server");
    // TODO: implement in Task 2
    Ok(())
}
