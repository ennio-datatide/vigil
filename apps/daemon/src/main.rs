//! Praefectus daemon entry point.
//!
//! Parses CLI arguments and delegates to the library crate.

use clap::{Parser, Subcommand};

/// Praefectus — AI session orchestrator.
#[derive(Debug, Parser)]
#[command(name = "praefectus", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the daemon server.
    Daemon {
        /// Port to listen on.
        #[arg(long, default_value_t = 8000)]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Daemon { port } => praefectus_daemon::run(port).await?,
    }

    Ok(())
}
