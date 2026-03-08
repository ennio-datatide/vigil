//! Tracks active PTY processes per session.
//!
//! Provides a handle-based API for writing to PTY stdin, checking liveness,
//! and resizing terminals. Actual PTY spawning is done by the agent spawner
//! (Task 1.12); this module only manages registered handles.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

/// Handle to a running PTY process.
pub(crate) struct PtyHandle {
    /// Channel to write data to the PTY's stdin.
    pub stdin_tx: tokio::sync::mpsc::Sender<Vec<u8>>,
    /// Whether the PTY process is still alive.
    pub alive: Arc<AtomicBool>,
}

/// Manages PTY handles for all active sessions.
#[derive(Debug)]
pub(crate) struct PtyManager {
    ptys: RwLock<HashMap<String, PtyHandle>>,
}

// `PtyHandle` contains an `mpsc::Sender` which is `!Debug`, so we implement
// `Debug` manually for `PtyManager` by skipping the inner map contents.
impl std::fmt::Debug for PtyHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyHandle")
            .field("alive", &self.alive.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)] // Methods used by agent spawner (Task 1.12).
impl PtyManager {
    /// Create a new, empty PTY manager.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            ptys: RwLock::new(HashMap::new()),
        }
    }

    /// Register a PTY handle for a session.
    pub(crate) async fn register(&self, session_id: &str, handle: PtyHandle) {
        self.ptys
            .write()
            .await
            .insert(session_id.to_owned(), handle);
    }

    /// Check if a PTY is alive for the given session.
    pub(crate) async fn is_alive(&self, session_id: &str) -> bool {
        self.ptys
            .read()
            .await
            .get(session_id)
            .is_some_and(|h| h.alive.load(Ordering::Relaxed))
    }

    /// Write data to a PTY's stdin. Returns `true` if the write was sent.
    pub(crate) async fn write(&self, session_id: &str, data: &[u8]) -> bool {
        let ptys = self.ptys.read().await;
        if let Some(handle) = ptys.get(session_id) {
            handle.stdin_tx.send(data.to_vec()).await.is_ok()
        } else {
            false
        }
    }

    /// Resize a PTY (placeholder — actual OS-level resize wired in Task 1.12).
    #[allow(clippy::unused_self)] // Will use self when wired to real PTY in Task 1.12.
    pub(crate) fn resize(&self, session_id: &str, cols: u16, rows: u16) {
        tracing::debug!(session_id, cols, rows, "PTY resize requested (placeholder)");
    }

    /// Remove a PTY handle for a session.
    pub(crate) async fn remove(&self, session_id: &str) {
        self.ptys.write().await.remove(session_id);
    }

    /// Kill a PTY process by marking it dead and removing its handle.
    pub(crate) async fn kill(&self, session_id: &str) {
        if let Some(handle) = self.ptys.write().await.remove(session_id) {
            handle.alive.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_check_alive() {
        let mgr = PtyManager::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let alive = Arc::new(AtomicBool::new(true));

        mgr.register(
            "s1",
            PtyHandle {
                stdin_tx: tx,
                alive: alive.clone(),
            },
        )
        .await;

        assert!(mgr.is_alive("s1").await);

        alive.store(false, Ordering::Relaxed);
        assert!(!mgr.is_alive("s1").await);
    }

    #[tokio::test]
    async fn write_sends_data() {
        let mgr = PtyManager::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let alive = Arc::new(AtomicBool::new(true));

        mgr.register(
            "s1",
            PtyHandle {
                stdin_tx: tx,
                alive,
            },
        )
        .await;

        assert!(mgr.write("s1", b"hello").await);
        let received = rx.recv().await.unwrap();
        assert_eq!(received, b"hello");
    }

    #[tokio::test]
    async fn write_to_missing_session_returns_false() {
        let mgr = PtyManager::new();
        assert!(!mgr.write("missing", b"data").await);
    }

    #[tokio::test]
    async fn kill_marks_dead_and_removes() {
        let mgr = PtyManager::new();
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let alive = Arc::new(AtomicBool::new(true));

        mgr.register(
            "s1",
            PtyHandle {
                stdin_tx: tx,
                alive: alive.clone(),
            },
        )
        .await;

        mgr.kill("s1").await;
        assert!(!alive.load(Ordering::Relaxed));
        assert!(!mgr.is_alive("s1").await);
    }
}
