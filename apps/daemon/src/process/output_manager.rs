//! Per-session output buffer management with disk persistence.
//!
//! Stores terminal output in memory (for fast replay on WebSocket connect)
//! and appends to a log file on disk for persistence across restarts.

use std::collections::HashMap;
use std::path::PathBuf;

use tokio::sync::{broadcast, RwLock};

/// In-memory output buffer for a single session.
struct OutputBuffer {
    data: Vec<u8>,
    sender: broadcast::Sender<Vec<u8>>,
}

/// Manages output buffers for all sessions.
#[derive(Debug)]
pub(crate) struct OutputManager {
    buffers: RwLock<HashMap<String, OutputBuffer>>,
    logs_dir: PathBuf,
}

// `OutputBuffer` contains a `broadcast::Sender` which may not be `Debug`,
// so we implement `Debug` manually.
impl std::fmt::Debug for OutputBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputBuffer")
            .field("data_len", &self.data.len())
            .finish_non_exhaustive()
    }
}

#[allow(dead_code)] // Methods used by agent spawner (Task 1.12).
impl OutputManager {
    /// Create a new output manager that persists logs to `logs_dir`.
    #[must_use]
    pub(crate) fn new(logs_dir: PathBuf) -> Self {
        Self {
            buffers: RwLock::new(HashMap::new()),
            logs_dir,
        }
    }

    /// Append output data for a session.
    ///
    /// Writes to the in-memory buffer, broadcasts to subscribers, and
    /// appends to the disk log file.
    pub(crate) async fn append(&self, session_id: &str, data: &[u8]) {
        let mut buffers = self.buffers.write().await;
        let buffer = buffers
            .entry(session_id.to_owned())
            .or_insert_with(|| OutputBuffer {
                data: Vec::new(),
                sender: broadcast::channel(256).0,
            });

        buffer.data.extend_from_slice(data);

        // Best-effort broadcast; receivers that lagged will skip.
        let _ = buffer.sender.send(data.to_vec());

        // Append to disk log file (best-effort).
        let log_path = self.log_path(session_id);
        if let Err(e) = Self::append_to_disk(&log_path, data) {
            tracing::warn!(session_id, error = %e, "failed to append to log file");
        }
    }

    /// Get the current in-memory buffer contents for a session.
    pub(crate) async fn get_buffer(&self, session_id: &str) -> Option<Vec<u8>> {
        self.buffers
            .read()
            .await
            .get(session_id)
            .map(|b| b.data.clone())
    }

    /// Subscribe to live output updates for a session.
    pub(crate) async fn subscribe(
        &self,
        session_id: &str,
    ) -> Option<broadcast::Receiver<Vec<u8>>> {
        self.buffers
            .read()
            .await
            .get(session_id)
            .map(|b| b.sender.subscribe())
    }

    /// Ensure a buffer exists for the given session (creates if needed).
    pub(crate) async fn ensure_buffer(&self, session_id: &str) {
        let mut buffers = self.buffers.write().await;
        buffers.entry(session_id.to_owned()).or_insert_with(|| {
            let (sender, _) = broadcast::channel(256);
            OutputBuffer {
                data: Vec::new(),
                sender,
            }
        });
    }

    /// Read history from the disk log file for a session.
    pub(crate) fn read_log(&self, session_id: &str) -> Option<Vec<u8>> {
        let path = self.log_path(session_id);
        std::fs::read(&path).ok()
    }

    /// Remove the buffer for a session.
    pub(crate) async fn remove(&self, session_id: &str) {
        self.buffers.write().await.remove(session_id);
    }

    /// Get the full output for a session, preferring the disk log (complete)
    /// over the in-memory buffer (may be truncated or cleared).
    pub(crate) fn get_full_output(&self, session_id: &str) -> Option<Vec<u8>> {
        // Prefer disk log — it's never truncated during a session.
        if let Some(disk) = self.read_log(session_id) {
            if !disk.is_empty() {
                return Some(disk);
            }
        }
        None
    }

    // -- helpers --

    fn log_path(&self, session_id: &str) -> PathBuf {
        self.logs_dir.join(format!("{session_id}.log"))
    }

    fn append_to_disk(path: &PathBuf, data: &[u8]) -> std::io::Result<()> {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?;
        file.write_all(data)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn append_and_get_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());

        mgr.append("s1", b"hello ").await;
        mgr.append("s1", b"world").await;

        let buf = mgr.get_buffer("s1").await.unwrap();
        assert_eq!(buf, b"hello world");
    }

    #[tokio::test]
    async fn subscribe_receives_new_data() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());

        mgr.ensure_buffer("s1").await;
        let mut rx = mgr.subscribe("s1").await.unwrap();

        mgr.append("s1", b"data").await;
        let received = rx.recv().await.unwrap();
        assert_eq!(received, b"data");
    }

    #[tokio::test]
    async fn read_log_from_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());

        mgr.append("s1", b"line1\n").await;
        mgr.append("s1", b"line2\n").await;

        let log = mgr.read_log("s1").unwrap();
        assert_eq!(log, b"line1\nline2\n");
    }

    #[tokio::test]
    async fn read_log_missing_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());

        assert!(mgr.read_log("missing").is_none());
    }

    #[tokio::test]
    async fn remove_clears_buffer() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());

        mgr.append("s1", b"data").await;
        mgr.remove("s1").await;

        assert!(mgr.get_buffer("s1").await.is_none());
    }

    #[tokio::test]
    async fn ensure_buffer_creates_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());

        mgr.ensure_buffer("s1").await;
        let buf = mgr.get_buffer("s1").await.unwrap();
        assert!(buf.is_empty());
    }

    #[tokio::test]
    async fn get_full_output_prefers_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());
        mgr.append("s1", b"hello ").await;
        mgr.append("s1", b"world").await;
        mgr.remove("s1").await;
        let output = mgr.get_full_output("s1");
        assert_eq!(output, Some(b"hello world".to_vec()));
    }

    #[tokio::test]
    async fn get_full_output_no_disk_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());
        mgr.ensure_buffer("s1").await;
        // No disk log exists, get_full_output returns None
        // (caller should use get_buffer as fallback)
        assert!(mgr.get_full_output("s1").is_none());
    }

    #[tokio::test]
    async fn get_full_output_returns_none_for_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = OutputManager::new(dir.path().to_path_buf());
        assert!(mgr.get_full_output("unknown").is_none());
    }
}
