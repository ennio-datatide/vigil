//! Command-line interface powered by `clap`.
//!
//! Defines the top-level CLI and subcommands. The `daemon` subcommand
//! starts the server; all others act as HTTP clients to a running daemon.

use clap::{Parser, Subcommand};

/// Default daemon URL for CLI client commands.
const DEFAULT_URL: &str = "http://localhost:8000";

/// Vigil — AI coding session orchestrator.
#[derive(Debug, Parser)]
#[command(name = "vigil", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Check Claude Code auth, then start the daemon (background by default).
    Up {
        /// Port to listen on.
        #[arg(long, default_value_t = 8000)]
        port: u16,
        /// Run in the foreground (show logs in terminal).
        #[arg(short, long)]
        foreground: bool,
    },
    /// Stop the daemon.
    Down,
    /// Start the daemon server (no auth check).
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
    /// Clear Vigil chat history.
    ClearHistory,
    /// Launch the interactive TUI (daemon embedded).
    Tui {
        /// Port for the background HTTP server.
        #[arg(long, default_value_t = 8000)]
        port: u16,
    },
}

/// Execute the `up` subcommand — verify Claude Code auth, then start the daemon.
///
/// # Errors
///
/// Propagates any error from [`crate::run`] or if `claude` CLI is missing.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub async fn cmd_up(port: u16) -> anyhow::Result<()> {
    println!("Checking Claude Code authentication...");

    // Run `claude auth status` (JSON output by default).
    let output = tokio::process::Command::new("claude")
        .args(["auth", "status"])
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            let json: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap_or_default();

            let logged_in = json["loggedIn"].as_bool().unwrap_or(false);
            let sub_type = json["subscriptionType"].as_str().unwrap_or("unknown");
            let email = json["email"].as_str().unwrap_or("unknown");

            if !logged_in {
                eprintln!("Not logged in to Claude Code.");
                eprintln!("Run: claude auth login");
                std::process::exit(1);
            }

            if sub_type != "max" && sub_type != "max_5x" {
                eprintln!("Claude Code subscription: {sub_type}");
                eprintln!("Vigil requires a Claude Max subscription for unlimited usage.");
                eprintln!("You can still use Vigil, but Vigil chat will incur per-token costs.");
                eprintln!();
            }

            println!("  Logged in as {email} (plan: {sub_type})");
        }
        Ok(_) => {
            eprintln!("Claude Code is not authenticated.");
            eprintln!("Run: claude auth login");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("Could not find the `claude` CLI.");
            eprintln!("Install Claude Code: https://docs.anthropic.com/en/docs/claude-code");
            std::process::exit(1);
        }
    }

    println!("Starting Vigil daemon on port {port}...");

    // Resolve the web app directory relative to the daemon binary or workspace root.
    let web_dir = find_web_dir();

    let web_child = if let Some(dir) = &web_dir {
        println!("Starting Next.js frontend on port 3000...");
        let child = unsafe {
            tokio::process::Command::new("npm")
                .args(["run", "dev"])
                .current_dir(dir)
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                // Put child in its own process group so we can kill the
                // entire tree (node + postcss workers) on shutdown.
                .pre_exec(|| {
                    libc::setpgid(0, 0);
                    Ok(())
                })
                .kill_on_drop(true)
                .spawn()
        };

        match child {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("Warning: could not start Next.js frontend: {e}");
                None
            }
        }
    } else {
        eprintln!("Warning: could not find apps/web directory; skipping frontend.");
        None
    };

    println!();

    crate::run(port).await?;

    // Daemon stopped — kill the entire frontend process group.
    if let Some(ref child) = web_child
        && let Some(pid) = child.id()
    {
        #[allow(clippy::cast_possible_wrap)]
        let pgid = pid as i32;
        // Send SIGTERM to the entire process group (negative PID).
        unsafe {
            libc::kill(-pgid, libc::SIGTERM);
        }
        println!("Stopped frontend (process group {pgid})");
    }

    Ok(())
}

/// Locate the `apps/web` directory by walking up from the current executable
/// or the current working directory looking for the monorepo root.
fn find_web_dir() -> Option<std::path::PathBuf> {
    // Try from current working directory first (most common in dev).
    if let Ok(cwd) = std::env::current_dir() {
        let candidate = cwd.join("apps/web");
        if candidate.join("package.json").exists() {
            return Some(candidate);
        }
        // Maybe we're inside apps/daemon — go up two levels.
        for ancestor in cwd.ancestors().skip(1) {
            let candidate = ancestor.join("apps/web");
            if candidate.join("package.json").exists() {
                return Some(candidate);
            }
        }
    }

    // Try from the executable location.
    if let Ok(exe) = std::env::current_exe() {
        for ancestor in exe.ancestors().skip(1) {
            let candidate = ancestor.join("apps/web");
            if candidate.join("package.json").exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Vigil home directory (`~/.vigil`).
fn vigil_home() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".vigil")
}

/// PID file path for a backgrounded daemon.
fn pid_file_path() -> std::path::PathBuf {
    vigil_home().join("daemon.pid")
}

/// Log file path for a backgrounded daemon.
fn daemon_log_path() -> std::path::PathBuf {
    vigil_home().join("logs").join("daemon.log")
}

/// Execute `vigil up -b` — re-spawn the current binary as a detached background
/// process, then exit immediately so the user gets their terminal back.
///
/// # Errors
///
/// Returns an error if the child process cannot be spawned.
///
/// # Panics
///
/// Panics if the current executable path cannot be determined.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn cmd_up_background(port: u16) -> anyhow::Result<()> {
    // Make sure dirs exist.
    let log_path = daemon_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let pid_path = pid_file_path();
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    // Check if already running.
    if pid_path.exists()
        && let Ok(content) = std::fs::read_to_string(&pid_path)
        && let Ok(pid) = content.trim().parse::<i32>()
    {
        // Check if process is alive.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        if alive {
            println!("Vigil is already running (PID {pid}).");
            println!("Use `vigil down` to stop it first.");
            return Ok(());
        }
        // Stale PID file — remove it.
        std::fs::remove_file(&pid_path).ok();
    }

    let exe = std::env::current_exe().expect("failed to get current executable path");
    let log_file = std::fs::File::create(&log_path)?;
    let log_stderr = log_file.try_clone()?;

    let child = std::process::Command::new(exe)
        .args(["up", "--foreground", "--port", &port.to_string()])
        .stdin(std::process::Stdio::null())
        .stdout(log_file)
        .stderr(log_stderr)
        .spawn()?;

    let pid = child.id();
    std::fs::write(&pid_path, pid.to_string())?;

    println!("Vigil started in background (PID {pid})");
    println!("  Daemon:   http://localhost:{port}");
    println!("  Frontend: http://localhost:3000");
    println!("  Logs:     {}", log_path.display());
    println!();
    println!("Use `vigil down` to stop.");

    Ok(())
}

/// Execute the `down` subcommand — stop a backgrounded daemon.
///
/// # Errors
///
/// Returns an error if the PID file cannot be read.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub async fn cmd_down() -> anyhow::Result<()> {
    let pid_path = pid_file_path();

    if !pid_path.exists() {
        // Try health check in case it's running without a PID file.
        let client = reqwest::Client::new();
        if client
            .get(format!("{DEFAULT_URL}/health"))
            .send()
            .await
            .is_ok()
        {
            eprintln!("A daemon seems to be running but has no PID file.");
            eprintln!("Kill it manually or find the process with: lsof -i :8000");
        } else {
            println!("Vigil is not running.");
        }
        return Ok(());
    }

    let content = std::fs::read_to_string(&pid_path)?;
    let pid: i32 = content
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid PID in {}", pid_path.display()))?;

    // Check if alive.
    let alive = unsafe { libc::kill(pid, 0) } == 0;
    if !alive {
        println!("Vigil (PID {pid}) is not running. Cleaning up PID file.");
        std::fs::remove_file(&pid_path).ok();
        return Ok(());
    }

    // Send SIGTERM to the process group (negative PID kills the whole group).
    // Fall back to the single process if PGID kill fails.
    println!("Stopping Vigil (PID {pid})...");
    let killed = unsafe { libc::kill(-pid, libc::SIGTERM) };
    if killed != 0 {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }

    // Wait briefly for shutdown.
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let still_alive = unsafe { libc::kill(pid, 0) } == 0;
        if !still_alive {
            break;
        }
    }

    std::fs::remove_file(&pid_path).ok();
    println!("Vigil stopped.");

    Ok(())
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
            eprintln!("Could not reach Vigil server. Is it running? Try: vigil up");
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
                    format!(
                        "...{}",
                        &project_path[project_path.len() - (project_w - 3)..]
                    )
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
            eprintln!("Could not reach Vigil server. Is it running? Try: vigil up");
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
            if let Ok(sr) = client
                .get(format!("{DEFAULT_URL}/api/sessions"))
                .send()
                .await
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
            eprintln!("Could not reach Vigil server. Is it running? Try: vigil up");
        }
    }

    Ok(())
}

/// Execute the `clear-history` subcommand — clears Vigil chat history.
///
/// # Errors
///
/// Returns an error if the daemon is unreachable.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub async fn cmd_clear_history() -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .delete(format!("{DEFAULT_URL}/api/vigil/history"))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            println!("Chat history cleared.");
        }
        Ok(r) => {
            let status = r.status();
            eprintln!("Failed to clear history: {status}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("Could not reach Vigil server. Is it running? Try: vigil up");
            std::process::exit(1);
        }
    }

    Ok(())
}
