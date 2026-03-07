//! WebSocket endpoint for real-time dashboard updates.
//!
//! On connect, sends a full state sync with all sessions. Then forwards
//! relevant [`AppEvent`]s (session updates, removals, notifications) to
//! the client as JSON messages.

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde_json::json;

use crate::deps::AppDeps;
use crate::events::AppEvent;
use crate::services::notification_store::NotificationStore;
use crate::services::session_store::SessionStore;

/// Upgrade an HTTP request to a WebSocket connection for the dashboard.
pub(crate) async fn ws_dashboard(
    ws: WebSocketUpgrade,
    State(deps): State<AppDeps>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_dashboard(socket, deps))
}

/// Main loop for a single dashboard WebSocket connection.
async fn handle_dashboard(socket: WebSocket, deps: AppDeps) {
    let (mut sender, mut receiver) = socket.split();

    // Send initial state sync with all sessions.
    let session_store = SessionStore::new(deps.db.clone());
    if let Ok(sessions) = session_store.list().await {
        let msg = json!({
            "type": "state_sync",
            "sessions": sessions,
        });
        if sender
            .send(Message::Text(msg.to_string().into()))
            .await
            .is_err()
        {
            return;
        }
    }

    // Subscribe to event bus.
    let mut event_rx = deps.event_bus.subscribe();

    // Spawn task to forward events to the WebSocket client.
    let db = deps.db.clone();
    let forward_task = tokio::spawn(async move {
        loop {
            match event_rx.recv().await {
                Ok(event) => {
                    let msg = match &event {
                        AppEvent::SessionUpdate { session } => {
                            // Fetch fresh from DB for consistency.
                            let store = SessionStore::new(db.clone());
                            if let Ok(Some(fresh)) = store.get(&session.id).await {
                                Some(json!({
                                    "type": "session_update",
                                    "session": fresh,
                                }))
                            } else {
                                None
                            }
                        }
                        AppEvent::SessionSpawned { session } => Some(json!({
                            "type": "session_update",
                            "session": session,
                        })),
                        AppEvent::SessionRemoved { session_id } => Some(json!({
                            "type": "session_removed",
                            "sessionId": session_id,
                        })),
                        AppEvent::NotificationCreated { notification_id } => {
                            let store = NotificationStore::new(db.clone());
                            if let Ok(Some(notification)) =
                                store.get_by_id(*notification_id).await
                            {
                                Some(json!({
                                    "type": "notification",
                                    "notification": notification,
                                }))
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    if let Some(payload) = msg
                        && sender
                            .send(Message::Text(payload.to_string().into()))
                            .await
                            .is_err()
                    {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(skipped = n, "dashboard WS lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Keep reading client messages (for keepalive / ping handling).
    // We don't expect meaningful client messages.
    while let Some(Ok(msg)) = receiver.next().await {
        if matches!(msg, Message::Close(_)) {
            break;
        }
    }

    forward_task.abort();
}
