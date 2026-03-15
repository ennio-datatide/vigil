//! Claude CLI invoker for Vigil.
//!
//! Calls `claude -p` with the Vigil MCP config and strategy prompt,
//! routing through Claude Code Max subscription instead of per-token API.

use std::path::Path;

use anyhow::{Context, Result};
use tokio::process::Command;

/// Timeout for a single `claude -p` invocation.
///
/// 300s gives MCP server startup + multi-turn tool use enough room.
const CLAUDE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Result of a Vigil CLI invocation.
#[derive(Debug)]
pub(crate) struct VigilCliResult {
    /// The text response from Claude.
    pub response: String,
    /// Session ID returned by Claude CLI (when using JSON output).
    pub session_id: Option<String>,
    /// Whether the invocation hit the max-turns limit.
    pub hit_max_turns: bool,
}

/// Write the MCP config JSON file for Vigil.
///
/// The config tells `claude` to spawn `praefectus mcp-serve` as a
/// subprocess, connecting via stdio transport.
pub(crate) fn write_mcp_config(path: &Path, daemon_url: &str) -> Result<()> {
    let config = serde_json::json!({
        "mcpServers": {
            "vigil": {
                "command": "pf",
                "args": ["mcp-serve", "--daemon-url", daemon_url],
            }
        }
    });

    let content = serde_json::to_string_pretty(&config)?;
    std::fs::write(path, content).context("failed to write MCP config")?;

    tracing::debug!(path = %path.display(), "wrote MCP config");

    Ok(())
}

/// Invoke `claude -p` with the Vigil configuration.
///
/// This routes through Claude Code Max subscription — no per-token costs.
/// The call is wrapped with a timeout to prevent indefinite hangs.
pub(crate) async fn invoke_vigil(
    prompt: &str,
    system_prompt_file: &Path,
    mcp_config_path: &Path,
    max_turns: u32,
) -> Result<VigilCliResult> {
    tracing::debug!(
        max_turns,
        system_prompt = %system_prompt_file.display(),
        mcp_config = %mcp_config_path.display(),
        prompt_len = prompt.len(),
        "invoking claude CLI"
    );

    let child = Command::new("claude")
        .args([
            "-p",
            prompt,
            "--output-format",
            "json",
            "--append-system-prompt-file",
            &system_prompt_file.to_string_lossy(),
            "--mcp-config",
            &mcp_config_path.to_string_lossy(),
            "--max-turns",
            &max_turns.to_string(),
            // Disable all built-in tools (Bash, Read, Write, etc.) so Vigil
            // can only use its 6 MCP tools. This forces delegation via
            // spawn_worker instead of doing work inline.
            "--tools",
            "",
            "--dangerously-skip-permissions",
        ])
        // Clear CLAUDECODE so the child `claude` process doesn't think
        // it's nested inside another Claude Code session.
        .env_remove("CLAUDECODE")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("failed to spawn claude CLI")?;

    tracing::debug!("claude CLI process spawned, waiting for output...");

    // Wait for the child with a timeout. kill_on_drop ensures cleanup on timeout.
    let output = if let Ok(result) =
        tokio::time::timeout(CLAUDE_TIMEOUT, child.wait_with_output()).await
    {
        result.context("claude CLI process failed")?
    } else {
        tracing::error!(timeout_secs = CLAUDE_TIMEOUT.as_secs(), "claude CLI timed out");
        anyhow::bail!("claude CLI timed out after {}s", CLAUDE_TIMEOUT.as_secs());
    };

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    tracing::debug!(
        status = ?output.status,
        stdout_len = stdout.len(),
        stderr_len = stderr.len(),
        "claude CLI finished"
    );

    if !stderr.is_empty() {
        tracing::debug!(stderr = %stderr, "claude CLI stderr");
    }

    // Parse the JSON output to extract session_id and result text.
    let parsed = parse_json_output(&stdout);

    tracing::debug!(
        session_id = ?parsed.session_id,
        response_len = parsed.response.len(),
        hit_max_turns = parsed.hit_max_turns,
        "parsed claude CLI output"
    );

    // Max turns is not a fatal error — return partial response with session link.
    if parsed.hit_max_turns {
        tracing::warn!(
            session_id = ?parsed.session_id,
            "claude CLI hit max turns limit ({max_turns})"
        );
        return Ok(parsed);
    }

    if !output.status.success() {
        tracing::error!(
            status = ?output.status,
            stderr = %stderr,
            "claude CLI failed"
        );
        anyhow::bail!(
            "claude CLI exited with {}: {}",
            output.status,
            stderr.lines().last().unwrap_or(&stderr)
        );
    }

    if parsed.response.is_empty() {
        anyhow::bail!(
            "claude CLI returned empty response. stderr: {}",
            stderr.lines().last().unwrap_or("(none)")
        );
    }

    Ok(parsed)
}

/// Parse the JSON output from `claude -p --output-format json`.
///
/// The output can be either:
/// - A single JSON object with `type: "result"` (default `--output-format json`)
/// - A JSON array of conversation messages (streaming mode)
fn parse_json_output(stdout: &str) -> VigilCliResult {
    let trimmed = stdout.trim();

    // Try parsing as a single JSON object first (most common case).
    if let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if obj.is_object() && obj["type"] == "result" {
            return extract_result(&obj);
        }

        // If it's an array, search for the result message.
        if let Some(messages) = obj.as_array() {
            if let Some(result_msg) = messages.iter().rev().find(|m| m["type"] == "result") {
                return extract_result(result_msg);
            }

            // Fallback: collect all assistant text blocks.
            let mut text = String::new();
            for msg in messages {
                if msg["type"] == "assistant"
                    && let Some(content) = msg["message"]["content"].as_array()
                {
                    for block in content {
                        if block["type"] == "text"
                            && let Some(t) = block["text"].as_str()
                        {
                            if !text.is_empty() {
                                text.push('\n');
                            }
                            text.push_str(t);
                        }
                    }
                }
            }

            return VigilCliResult {
                response: text,
                session_id: None,
                hit_max_turns: false,
            };
        }
    }

    // If JSON parsing fails entirely, treat raw stdout as plain text response.
    tracing::warn!("failed to parse claude CLI JSON output, using raw stdout");
    VigilCliResult {
        response: trimmed.to_string(),
        session_id: None,
        hit_max_turns: false,
    }
}

/// Extract a [`VigilCliResult`] from a `type: "result"` JSON object.
fn extract_result(obj: &serde_json::Value) -> VigilCliResult {
    let session_id = obj["session_id"].as_str().map(String::from);
    let response = obj["result"].as_str().unwrap_or("").to_string();
    let hit_max_turns = obj["subtype"] == "max_turns";

    VigilCliResult {
        response,
        session_id,
        hit_max_turns,
    }
}
