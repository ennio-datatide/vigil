//! Claude Code hook installation.
//!
//! Installs hook scripts and settings into a working directory's `.claude/`
//! folder so that Claude Code lifecycle events are forwarded to the daemon.

use std::path::Path;

/// Shell script template that reads hook JSON from stdin and POSTs it to the
/// daemon's `/events` endpoint. Placeholders `__SESSION_ID__` and
/// `__SERVER_PORT__` are replaced at install time.
const EMIT_EVENT_TEMPLATE: &str = r#"#!/bin/bash
# emit-event.sh — reads hook JSON from stdin, forwards to praefectus server
INPUT=$(cat)
SESSION_ID="__SESSION_ID__"
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT
printf '{"session_id":"%s","data":%s}' "$SESSION_ID" "$INPUT" > "$TMPFILE"
curl -s -X POST "http://localhost:__SERVER_PORT__/events" \
  -H "Content-Type: application/json" \
  -d @"$TMPFILE" \
  > /dev/null 2>&1
"#;

/// Claude Code `settings.json` template that wires all lifecycle hooks to
/// `emit-event.sh`. The `__HOOKS_DIR__` placeholder is replaced at install
/// time with the absolute path to the hooks directory.
const SETTINGS_TEMPLATE: &str = r#"{
  "hooks": {
    "PreToolUse": [{ "hooks": [{ "type": "command", "command": "bash __HOOKS_DIR__/emit-event.sh" }] }],
    "PostToolUse": [{ "hooks": [{ "type": "command", "command": "bash __HOOKS_DIR__/emit-event.sh" }] }],
    "Stop": [{ "hooks": [{ "type": "command", "command": "bash __HOOKS_DIR__/emit-event.sh" }] }],
    "SubagentStart": [{ "hooks": [{ "type": "command", "command": "bash __HOOKS_DIR__/emit-event.sh" }] }],
    "SubagentStop": [{ "hooks": [{ "type": "command", "command": "bash __HOOKS_DIR__/emit-event.sh" }] }],
    "Notification": [{ "hooks": [{ "type": "command", "command": "bash __HOOKS_DIR__/emit-event.sh" }] }]
  }
}"#;

/// Installs Claude Code hooks into a working directory.
pub(crate) struct HookInstaller;

impl HookInstaller {
    /// Install hooks into the working directory's `.claude/hooks/` folder.
    ///
    /// Creates `emit-event.sh` (with session ID and server port baked in) and
    /// `settings.json` (with hook directory path baked in).
    ///
    /// # Errors
    ///
    /// Returns an error if directory creation or file writes fail.
    pub(crate) fn install(
        work_dir: &Path,
        session_id: &str,
        server_port: u16,
    ) -> anyhow::Result<()> {
        let hooks_dir = work_dir.join(".claude").join("hooks");
        fs_err::create_dir_all(&hooks_dir)?;

        // Write emit-event.sh with placeholders replaced.
        let emit_script = EMIT_EVENT_TEMPLATE
            .replace("__SESSION_ID__", session_id)
            .replace("__SERVER_PORT__", &server_port.to_string());
        let emit_path = hooks_dir.join("emit-event.sh");
        fs_err::write(&emit_path, &emit_script)?;

        // Make executable on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs_err::set_permissions(&emit_path, std::fs::Permissions::from_mode(0o755))?;
        }

        // Write settings.json with hooks dir path.
        let hooks_dir_str = hooks_dir.display().to_string();
        let settings = SETTINGS_TEMPLATE.replace("__HOOKS_DIR__", &hooks_dir_str);
        let settings_path = work_dir.join(".claude").join("settings.json");
        fs_err::write(&settings_path, &settings)?;

        tracing::debug!(
            work_dir = %work_dir.display(),
            session_id,
            server_port,
            "hooks installed",
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn install_creates_hook_files() {
        let dir = TempDir::new().unwrap();
        let work_dir = dir.path();

        HookInstaller::install(work_dir, "sess-abc", 4000).unwrap();

        // emit-event.sh should exist and contain the session ID and port.
        let emit_path = work_dir.join(".claude/hooks/emit-event.sh");
        assert!(emit_path.exists());
        let emit_content = fs_err::read_to_string(&emit_path).unwrap();
        assert!(emit_content.contains("sess-abc"));
        assert!(emit_content.contains("4000"));
        assert!(!emit_content.contains("__SESSION_ID__"));
        assert!(!emit_content.contains("__SERVER_PORT__"));

        // Check executable permissions on Unix.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = std::fs::metadata(&emit_path).unwrap().permissions();
            assert_ne!(perms.mode() & 0o111, 0, "emit-event.sh should be executable");
        }

        // settings.json should exist and contain the hooks dir path.
        let settings_path = work_dir.join(".claude/settings.json");
        assert!(settings_path.exists());
        let settings_content = fs_err::read_to_string(&settings_path).unwrap();
        assert!(!settings_content.contains("__HOOKS_DIR__"));
        let hooks_dir = work_dir.join(".claude/hooks");
        assert!(settings_content.contains(&hooks_dir.display().to_string()));

        // settings.json should be valid JSON.
        let parsed: serde_json::Value = serde_json::from_str(&settings_content).unwrap();
        assert!(parsed.get("hooks").is_some());
    }

    #[test]
    fn install_idempotent() {
        let dir = TempDir::new().unwrap();
        let work_dir = dir.path();

        // Installing twice should not fail.
        HookInstaller::install(work_dir, "sess-1", 4000).unwrap();
        HookInstaller::install(work_dir, "sess-2", 4001).unwrap();

        // Second install should overwrite with new values.
        let emit_content =
            fs_err::read_to_string(work_dir.join(".claude/hooks/emit-event.sh")).unwrap();
        assert!(emit_content.contains("sess-2"));
        assert!(emit_content.contains("4001"));
    }
}
