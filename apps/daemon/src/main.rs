//! Praefectus daemon entry point.
//!
//! Parses CLI arguments and delegates to the library crate.

use clap::Parser;
use praefectus_daemon::cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Daemon { port } => praefectus_daemon::cli::cmd_daemon(port).await,
        Command::Start {
            project,
            prompt,
            skill,
        } => praefectus_daemon::cli::cmd_start(&project, &prompt, skill.as_deref()).await,
        Command::Ls { all } => praefectus_daemon::cli::cmd_ls(all).await,
        Command::Status => praefectus_daemon::cli::cmd_status().await,
        Command::Cleanup => praefectus_daemon::cli::cmd_cleanup().await,
        Command::McpServe { daemon_url } => praefectus_daemon::mcp::run_mcp_server(daemon_url).await,
    }
}
