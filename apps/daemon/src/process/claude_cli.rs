//! Claude CLI invoker for Vigil.
//!
//! Calls `claude -p` with the Vigil MCP config and strategy prompt,
//! routing through Claude Code Max subscription instead of per-token API.

use std::path::Path;

use anyhow::{Context, Result};
use tokio::process::Command;

/// Result of a Vigil CLI invocation.
#[derive(Debug)]
pub(crate) struct VigilCliResult {
    /// The text response from Claude.
    pub response: String,
}

/// Write the MCP config JSON file for Vigil.
///
/// The config tells `claude` to spawn `praefectus mcp-serve` as a
/// subprocess, connecting via stdio transport.
pub(crate) fn write_mcp_config(path: &Path, daemon_url: &str) -> Result<()> {
    let config = serde_json::json!({
        "mcpServers": {
            "vigil": {
                "command": "praefectus",
                "args": ["mcp-serve", "--daemon-url", daemon_url],
            }
        }
    });

    let content = serde_json::to_string_pretty(&config)?;
    std::fs::write(path, content).context("failed to write MCP config")?;

    Ok(())
}

/// Invoke `claude -p` with the Vigil configuration.
///
/// This routes through Claude Code Max subscription — no per-token costs.
pub(crate) async fn invoke_vigil(
    prompt: &str,
    system_prompt_file: &Path,
    mcp_config_path: &Path,
    max_turns: u32,
) -> Result<VigilCliResult> {
    let output = Command::new("claude")
        .args([
            "-p",
            prompt,
            "--output-format",
            "text",
            "--append-system-prompt-file",
            &system_prompt_file.to_string_lossy(),
            "--mcp-config",
            &mcp_config_path.to_string_lossy(),
            "--max-turns",
            &max_turns.to_string(),
            "--model",
            "sonnet",
            "--dangerously-skip-permissions",
            "--no-session-persistence",
        ])
        .output()
        .await
        .context("failed to execute claude CLI")?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        tracing::error!(
            status = ?output.status,
            stderr = %stderr,
            "claude CLI failed"
        );
    }

    Ok(VigilCliResult {
        response: if stdout.trim().is_empty() {
            format!("Vigil encountered an error: {stderr}")
        } else {
            stdout.trim().to_string()
        },
    })
}
