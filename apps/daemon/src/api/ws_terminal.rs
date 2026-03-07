//! WebSocket endpoint for terminal PTY proxy.
//!
//! Connects a client to a session's PTY, replaying buffered and persisted
//! output history on connect, then streaming live output and forwarding
//! client input/resize commands.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;

use crate::deps::AppDeps;
use crate::error::Error;
use crate::events::AppEvent;
use crate::services::session_store::SessionStore;

/// Client-to-server message types.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
}

/// Upgrade an HTTP request to a terminal WebSocket for the given session.
pub(crate) async fn ws_terminal(
    ws: WebSocketUpgrade,
    Path(session_id): Path<String>,
    State(deps): State<AppDeps>,
) -> Result<impl IntoResponse, Error> {
    // Validate session exists before upgrading.
    let store = SessionStore::new(deps.db.clone());
    let _session = store
        .get(&session_id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("session {session_id}")))?;

    Ok(ws.on_upgrade(move |socket| handle_terminal(socket, session_id, deps)))
}

/// Main loop for a single terminal WebSocket connection.
async fn handle_terminal(socket: WebSocket, session_id: String, deps: AppDeps) {
    let (mut sender, mut receiver) = socket.split();

    // 1. Send PTY status.
    let alive = deps.pty_manager.is_alive(&session_id).await;
    let status_msg = json!({ "type": "pty_status", "alive": alive });
    if sender
        .send(Message::Text(status_msg.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    // 2. Replay history: first in-memory buffer, then disk log.
    let history = deps
        .output_manager
        .get_buffer(&session_id)
        .await
        .or_else(|| deps.output_manager.read_log(&session_id));

    if let Some(data) = history
        && !data.is_empty()
    {
        let output_msg = json!({
            "type": "output",
            "data": String::from_utf8_lossy(&data),
        });
        if sender
            .send(Message::Text(output_msg.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
    }

    // 3. Ensure buffer exists so we can subscribe.
    deps.output_manager.ensure_buffer(&session_id).await;

    // 4. Subscribe to live output updates.
    let output_rx = deps.output_manager.subscribe(&session_id).await;

    // 5. Subscribe to event bus for session_spawned events.
    let mut event_rx = deps.event_bus.subscribe();

    // Spawn task to forward live output and pty_status events to the client.
    let sid = session_id.clone();
    let forward_task = tokio::spawn(async move {
        let Some(mut output_rx) = output_rx else {
            return;
        };

        loop {
            tokio::select! {
                result = output_rx.recv() => {
                    match result {
                        Ok(data) => {
                            let msg = json!({
                                "type": "output",
                                "data": String::from_utf8_lossy(&data),
                            });
                            if sender.send(Message::Text(msg.to_string().into())).await.is_err() {
                                break;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(session_id = %sid, skipped = n, "terminal WS output lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
                result = event_rx.recv() => {
                    match result {
                        Ok(AppEvent::SessionSpawned { session }) if session.id == sid => {
                            let msg = json!({ "type": "pty_status", "alive": true });
                            if sender.send(Message::Text(msg.to_string().into())).await.is_err() {
                                break;
                            }
                        }
                        Ok(_) => {} // Ignore other events.
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(session_id = %sid, skipped = n, "terminal WS event lagged");
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            }
        }
    });

    // 6. Process client messages (input + resize).
    let pty_manager = deps.pty_manager.clone();
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                    match client_msg {
                        ClientMessage::Input { data } => {
                            pty_manager.write(&session_id, data.as_bytes()).await;
                        }
                        ClientMessage::Resize { cols, rows } => {
                            pty_manager.resize(&session_id, cols, rows);
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {} // Ignore binary/ping/pong.
        }
    }

    forward_task.abort();
}
