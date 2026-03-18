//! Vigil daemon entry point.
//!
//! Parses CLI arguments and delegates to the library crate.

use clap::Parser;
use vigil_daemon::cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Up { port, foreground } => {
            if foreground {
                vigil_daemon::cli::cmd_up(port).await
            } else {
                vigil_daemon::cli::cmd_up_background(port)
            }
        }
        Command::Down => vigil_daemon::cli::cmd_down().await,
        Command::Daemon { port } => vigil_daemon::cli::cmd_daemon(port).await,
        Command::Start {
            project,
            prompt,
            skill,
        } => vigil_daemon::cli::cmd_start(&project, &prompt, skill.as_deref()).await,
        Command::Ls { all } => vigil_daemon::cli::cmd_ls(all).await,
        Command::Status => vigil_daemon::cli::cmd_status().await,
        Command::Cleanup => vigil_daemon::cli::cmd_cleanup().await,
        Command::ClearHistory => vigil_daemon::cli::cmd_clear_history().await,
        Command::Tui { port } => {
            vigil_daemon::run_tui(port)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    }
}
