//! Vigil daemon entry point.
//!
//! Parses CLI arguments and delegates to the library crate.

use clap::Parser;
use vigil::cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Up { port, foreground } => {
            if foreground {
                vigil::cli::cmd_up(port).await
            } else {
                vigil::cli::cmd_up_background(port)
            }
        }
        Command::Down => vigil::cli::cmd_down().await,
        Command::Daemon { port } => vigil::cli::cmd_daemon(port).await,
        Command::Start {
            project,
            prompt,
            skill,
        } => vigil::cli::cmd_start(&project, &prompt, skill.as_deref()).await,
        Command::Ls { all } => vigil::cli::cmd_ls(all).await,
        Command::Status => vigil::cli::cmd_status().await,
        Command::Cleanup => vigil::cli::cmd_cleanup().await,
        Command::ClearHistory => vigil::cli::cmd_clear_history().await,
        Command::Tui { port } => {
            vigil::run_tui(port)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
    }
}
