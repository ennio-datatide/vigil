//! Process and session spawning.
//!
//! Handles PTY management, output buffering, and agent process lifecycle.

pub(crate) mod agent_spawner;
pub(crate) mod output_manager;
pub(crate) mod pty_manager;
