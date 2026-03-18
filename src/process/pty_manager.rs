//! Tracks active PTY processes per session.
//!
//! Provides a handle-based API for writing to PTY stdin, checking liveness,
//! resizing terminals, and killing child processes. Uses `portable-pty` for
//! real OS PTY allocation with SIGWINCH support.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use portable_pty::MasterPty;
use tokio::sync::{mpsc, Mutex};

/// Handle to a running PTY process.
pub(crate) struct PtyHandle {
    /// Channel to serialize writes to the PTY master.
    pub stdin_tx: mpsc::Sender<Vec<u8>>,
    /// Master PTY handle — used for resize operations.
    pub master: Box<dyn MasterPty + Send>,
    /// Child process handle — used for forceful kill.
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Whether the PTY process is still alive.
    pub alive: Arc<AtomicBool>,
}

// `PtyHandle` contains trait objects which are `!Debug`, so we implement
// `Debug` manually by skipping the inner contents.
impl std::fmt::Debug for PtyHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyHandle")
            .field("alive", &self.alive.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

/// Manages PTY handles for all active sessions.
#[derive(Debug)]
pub(crate) struct PtyManager {
    ptys: Mutex<HashMap<String, PtyHandle>>,
}

impl PtyManager {
    /// Create a new, empty PTY manager.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            ptys: Mutex::new(HashMap::new()),
        }
    }

    /// Register a PTY handle for a session.
    pub(crate) async fn register(&self, session_id: &str, handle: PtyHandle) {
        self.ptys
            .lock()
            .await
            .insert(session_id.to_owned(), handle);
    }

    /// Check if a PTY is alive for the given session.
    pub(crate) async fn is_alive(&self, session_id: &str) -> bool {
        self.ptys
            .lock()
            .await
            .get(session_id)
            .is_some_and(|h| h.alive.load(Ordering::Relaxed))
    }

    /// Write bytes to the PTY via the serialized channel.
    pub(crate) async fn write(&self, session_id: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let ptys = self.ptys.lock().await;
        let handle = ptys
            .get(session_id)
            .ok_or_else(|| anyhow::anyhow!("no pty for session"))?;
        handle
            .stdin_tx
            .send(data)
            .await
            .map_err(|_| anyhow::anyhow!("stdin channel closed"))?;
        Ok(())
    }

    /// Resize the PTY. Delivers SIGWINCH to the child process.
    pub(crate) async fn resize(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<()> {
        let mut ptys = self.ptys.lock().await;
        let handle = ptys
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("no pty for session"))?;
        handle
            .master
            .resize(portable_pty::PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow::anyhow!("resize failed: {e}"))?;
        Ok(())
    }

    /// Graceful kill: drop master (SIGHUP), then SIGKILL fallback.
    pub(crate) async fn kill(&self, session_id: &str) {
        let handle = self.ptys.lock().await.remove(session_id);
        if let Some(mut handle) = handle {
            handle.alive.store(false, Ordering::Relaxed);
            // Drop master — sends SIGHUP to child process group.
            drop(handle.master);
            // Grace period then force kill.
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            let _ = handle.child.kill();
        }
    }

    /// Remove a PTY handle for a session.
    pub(crate) async fn remove(&self, session_id: &str) {
        self.ptys.lock().await.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Read;
    use tokio::time::{sleep, Duration};

    /// Helper: spawn /bin/cat inside a real PTY and register it.
    /// Returns (manager, session_id, reader) where reader is the master read handle.
    async fn spawn_cat_pty() -> (PtyManager, String, Box<dyn Read + Send>) {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("open pty");

        let cmd = CommandBuilder::new("/bin/cat");
        let child = pair.slave.spawn_command(cmd).expect("spawn cat");
        drop(pair.slave); // Drop slave so EOF works

        let reader = pair.master.try_clone_reader().expect("clone reader");
        let writer = pair.master.take_writer().expect("take writer");

        let (stdin_tx, mut stdin_rx) = mpsc::channel::<Vec<u8>>(64);
        let alive = Arc::new(AtomicBool::new(true));

        // Spawn writer drain thread (must use spawn_blocking for tokio context)
        tokio::task::spawn_blocking(move || {
            let mut writer = writer;
            while let Some(data) = stdin_rx.blocking_recv() {
                use std::io::Write;
                if writer.write_all(&data).is_err() {
                    break;
                }
            }
        });

        let manager = PtyManager::new();
        let session_id = "test-pty-session".to_string();

        manager
            .register(
                &session_id,
                PtyHandle {
                    stdin_tx,
                    master: pair.master,
                    child,
                    alive: Arc::clone(&alive),
                },
            )
            .await;

        (manager, session_id, reader)
    }

    #[tokio::test]
    async fn test_write_and_read_through_pty() {
        let (manager, sid, mut reader) = spawn_cat_pty().await;

        manager.write(&sid, b"hello\n".to_vec()).await.unwrap();

        // Give cat time to echo
        sleep(Duration::from_millis(100)).await;

        let mut buf = [0u8; 256];
        // cat echoes input back through the PTY
        let n = reader.read(&mut buf).unwrap();
        let output = String::from_utf8_lossy(&buf[..n]);
        assert!(output.contains("hello"), "Expected echoed input, got: {output}");

        manager.kill(&sid).await;
    }

    #[tokio::test]
    async fn test_resize() {
        let (manager, sid, _reader) = spawn_cat_pty().await;

        // Should not panic or error
        let result = manager.resize(&sid, 120, 40).await;
        assert!(result.is_ok(), "Resize should succeed");

        manager.kill(&sid).await;
    }

    #[tokio::test]
    async fn test_kill_marks_dead() {
        let (manager, sid, _reader) = spawn_cat_pty().await;

        assert!(manager.is_alive(&sid).await);
        manager.kill(&sid).await;
        // After kill, handle is removed
        assert!(!manager.is_alive(&sid).await);
    }

    #[tokio::test]
    async fn test_concurrent_writes() {
        let (manager, sid, mut reader) = spawn_cat_pty().await;
        let manager = Arc::new(manager);

        let m1 = Arc::clone(&manager);
        let s1 = sid.clone();
        let t1 = tokio::spawn(async move {
            for i in 0..10 {
                m1.write(&s1, format!("a{i}\n").into_bytes()).await.unwrap();
            }
        });

        let m2 = Arc::clone(&manager);
        let s2 = sid.clone();
        let t2 = tokio::spawn(async move {
            for i in 0..10 {
                m2.write(&s2, format!("b{i}\n").into_bytes()).await.unwrap();
            }
        });

        t1.await.unwrap();
        t2.await.unwrap();

        sleep(Duration::from_millis(200)).await;

        let mut buf = vec![0u8; 4096];
        let n = reader.read(&mut buf).unwrap();
        let output = String::from_utf8_lossy(&buf[..n]);

        // All 20 lines should appear (order may vary)
        for i in 0..10 {
            assert!(output.contains(&format!("a{i}")), "Missing a{i} in output");
            assert!(output.contains(&format!("b{i}")), "Missing b{i} in output");
        }

        manager.kill(&sid).await;
    }
}
