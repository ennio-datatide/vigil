//! Command-line interface powered by `clap`.
//!
//! Defines the top-level CLI and subcommands. The `daemon` subcommand
//! starts the server; all others act as HTTP clients to a running daemon.

use clap::{Parser, Subcommand};

/// Default daemon URL for CLI client commands.
const DEFAULT_URL: &str = "http://localhost:8000";

/// Praefectus — AI coding session orchestrator.
#[derive(Debug, Parser)]
#[command(name = "praefectus", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the daemon server.
    Daemon {
        /// Port to listen on.
        #[arg(long, default_value_t = 8000)]
        port: u16,
    },
    /// Start a new coding session.
    Start {
        /// Project path.
        #[arg(short, long)]
        project: String,
        /// Prompt for the session.
        prompt: String,
        /// Skill to use.
        #[arg(short, long)]
        skill: Option<String>,
    },
    /// List sessions.
    Ls {
        /// Show all sessions (including completed).
        #[arg(short, long)]
        all: bool,
    },
    /// Show daemon status.
    Status,
    /// Clean up stale sessions and worktrees.
    Cleanup,
}

/// Execute the `daemon` subcommand — starts the server.
///
/// # Errors
///
/// Propagates any error from [`crate::run`].
pub async fn cmd_daemon(port: u16) -> anyhow::Result<()> {
    crate::run(port).await?;
    Ok(())
}

/// Execute the `start` subcommand — creates a new session via the daemon API.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the daemon is unreachable.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub async fn cmd_start(project: &str, prompt: &str, skill: Option<&str>) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({
        "projectPath": project,
        "prompt": prompt,
    });
    if let Some(s) = skill {
        body["skill"] = serde_json::json!(s);
    }

    let resp = client
        .post(format!("{DEFAULT_URL}/api/sessions"))
        .json(&body)
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let session: serde_json::Value = r.json().await?;
            let id = session["id"].as_str().unwrap_or("unknown");
            let status = session["status"].as_str().unwrap_or("unknown");
            println!("Session started: {id} ({status})");
        }
        Ok(r) => {
            let status_code = r.status();
            let err: serde_json::Value = r.json().await.unwrap_or_default();
            let fallback = status_code.to_string();
            let msg = err["error"].as_str().unwrap_or(&fallback);
            eprintln!("Failed to start session: {msg}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("Could not reach Praefectus server. Is it running? Try: praefectus daemon");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Execute the `ls` subcommand — lists sessions from the daemon.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the daemon is unreachable.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub async fn cmd_ls(all: bool) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{DEFAULT_URL}/api/sessions"))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let sessions: Vec<serde_json::Value> = r.json().await?;

            let filtered: Vec<&serde_json::Value> = if all {
                sessions.iter().collect()
            } else {
                sessions
                    .iter()
                    .filter(|s| {
                        let st = s["status"].as_str().unwrap_or("");
                        !matches!(st, "completed" | "cancelled" | "error")
                    })
                    .collect()
            };

            if filtered.is_empty() {
                if all {
                    println!("No sessions found.");
                } else {
                    println!("No active sessions. Use --all to see completed.");
                }
                return Ok(());
            }

            // Column widths matching the TypeScript CLI.
            let id_w = 14;
            let status_w = 12;
            let project_w = 30;
            let prompt_w = 40;

            println!(
                "{:<id_w$}  {:<status_w$}  {:<project_w$}  {:<prompt_w$}",
                "ID", "STATUS", "PROJECT", "PROMPT",
            );
            println!("{}", "-".repeat(id_w + status_w + project_w + prompt_w + 6));

            for s in &filtered {
                let id = s["id"].as_str().unwrap_or("?");
                let status = s["status"].as_str().unwrap_or("?");
                let project_path = s["projectPath"].as_str().unwrap_or("?");
                let prompt = s["prompt"].as_str().unwrap_or("?");

                let trunc_project = if project_path.len() > project_w {
                    format!("...{}", &project_path[project_path.len() - (project_w - 3)..])
                } else {
                    project_path.to_string()
                };

                let trunc_prompt = if prompt.len() > prompt_w {
                    format!("{}...", &prompt[..prompt_w - 3])
                } else {
                    prompt.to_string()
                };

                println!(
                    "{id:<id_w$}  {status:<status_w$}  {trunc_project:<project_w$}  {trunc_prompt:<prompt_w$}",
                );
            }
        }
        Ok(_) => {
            eprintln!("Failed to list sessions.");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("Could not reach Praefectus server. Is it running? Try: praefectus daemon");
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Execute the `status` subcommand — shows whether the daemon is running and
/// a summary of active sessions.
///
/// # Errors
///
/// Returns an error if the HTTP response cannot be parsed.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub async fn cmd_status() -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let health = client.get(format!("{DEFAULT_URL}/health")).send().await;

    match health {
        Ok(r) if r.status().is_success() => {
            let h: serde_json::Value = r.json().await?;
            let status = h["status"].as_str().unwrap_or("unknown");
            println!("Server: {status} (port 8000)");

            // Fetch session counts.
            if let Ok(sr) = client.get(format!("{DEFAULT_URL}/api/sessions")).send().await
                && sr.status().is_success()
            {
                let sessions: Vec<serde_json::Value> = sr.json().await?;
                let mut counts = std::collections::BTreeMap::<String, usize>::new();
                for s in &sessions {
                    let st = s["status"].as_str().unwrap_or("unknown").to_string();
                    *counts.entry(st).or_default() += 1;
                }

                println!("\nSessions ({} total):", sessions.len());
                if counts.is_empty() {
                    println!("  No sessions");
                } else {
                    for (st, count) in &counts {
                        println!("  {st}: {count}");
                    }
                }
            }
        }
        _ => {
            println!("Daemon: not running");
        }
    }

    Ok(())
}

/// Execute the `cleanup` subcommand — triggers stale session cleanup via
/// the daemon API.
///
/// # Errors
///
/// Returns an error if the HTTP response cannot be parsed.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub async fn cmd_cleanup() -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{DEFAULT_URL}/api/cleanup"))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let data: serde_json::Value = r.json().await?;
            let removed = data["removed"].as_i64().unwrap_or(0);
            let skipped = data["skipped"].as_i64().unwrap_or(0);
            println!("Cleanup complete: {removed} removed, {skipped} skipped");
        }
        Ok(_) => {
            println!("Cleanup: daemon returned an error.");
        }
        Err(_) => {
            eprintln!("Could not reach Praefectus server. Is it running? Try: praefectus daemon");
        }
    }

    Ok(())
}
