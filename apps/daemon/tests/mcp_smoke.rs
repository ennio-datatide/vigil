//! Smoke test for the MCP server subcommand.

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[tokio::test]
async fn mcp_serve_responds_to_initialize() {
    // Build the binary first.
    let status = Command::new("cargo")
        .args(["build", "--bin", "praefectus"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .status()
        .await
        .expect("cargo build");
    assert!(status.success(), "cargo build failed");

    // Find the binary. CARGO_MANIFEST_DIR is apps/daemon, and since there is
    // no Cargo workspace the target dir lives directly under it.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let binary = std::path::Path::new(manifest_dir).join("target/debug/praefectus");

    let mut child = Command::new(&binary)
        .args(["mcp-serve", "--daemon-url", "http://localhost:9999"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-serve");

    let stdin = child.stdin.as_mut().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    // Send MCP initialize request (JSON-RPC).
    let init_request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "test",
                "version": "0.1"
            }
        }
    });

    let msg = serde_json::to_string(&init_request).unwrap();
    stdin.write_all(msg.as_bytes()).await.unwrap();
    stdin.write_all(b"\n").await.unwrap();
    stdin.flush().await.unwrap();

    // Read the response with a timeout.
    let response_line = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        reader.next_line(),
    )
    .await
    .expect("timeout waiting for MCP response")
    .expect("IO error")
    .expect("no response line");

    let response: serde_json::Value =
        serde_json::from_str(&response_line).expect("valid JSON response");

    assert_eq!(response["id"], 1, "response ID should match request");
    assert!(
        response["result"]["serverInfo"]["name"]
            .as_str()
            .unwrap_or("")
            .contains("praefectus"),
        "server info should contain praefectus, got: {response}"
    );

    // Clean up.
    child.kill().await.ok();
}
