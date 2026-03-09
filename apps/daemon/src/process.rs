//! Process and session spawning.
//!
//! Handles PTY management, output buffering, and agent process lifecycle.

pub(crate) mod agent_spawner;
pub(crate) mod claude_cli;
pub(crate) mod output_manager;
#[allow(dead_code)] // Used by pipeline runner in a later phase.
pub(crate) mod output_extract;
pub(crate) mod pty_manager;
