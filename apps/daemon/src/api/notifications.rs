//! Notification route handlers.
//!
//! Implements the REST endpoints for notification management:
//! list, test creation, mark single read, and mark all read.

use axum::extract::{Json, Path, Query, State};
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use crate::db::models::NotificationType;
use crate::deps::AppDeps;
use crate::error::{Error, Result};
use crate::events::AppEvent;
use crate::services::notification_store::NotificationStore;

/// Query parameters for `GET /api/notifications`.
#[derive(Debug, Deserialize)]
pub(crate) struct ListQuery {
    pub unread: Option<bool>,
}

/// `GET /api/notifications` — list notifications, optionally filtered to unread.
pub(crate) async fn list_notifications(
    State(deps): State<AppDeps>,
    Query(query): Query<ListQuery>,
) -> Result<impl IntoResponse> {
    let store = NotificationStore::new(deps.db);
    let unread_only = query.unread.unwrap_or(false);
    let notifications = store.list(unread_only).await?;
    Ok(Json(notifications))
}

/// `POST /api/notifications/test` — create a test notification.
pub(crate) async fn test_notification(
    State(deps): State<AppDeps>,
) -> Result<impl IntoResponse> {
    let store = NotificationStore::new(deps.db.clone());
    let notification = store
        .create(
            "system",
            NotificationType::SessionDone,
            "Test notification from Praefectus",
        )
        .await?;

    let _ = deps.event_bus.emit(AppEvent::NotificationCreated {
        notification_id: notification.id,
    });

    Ok(Json(json!({ "ok": true, "message": "Test notification sent" })))
}

/// `PATCH /api/notifications/read-all` — mark all unread notifications as read.
pub(crate) async fn read_all(
    State(deps): State<AppDeps>,
) -> Result<impl IntoResponse> {
    let store = NotificationStore::new(deps.db);
    let updated = store.mark_all_read().await?;
    Ok(Json(json!({ "updated": updated })))
}

/// `PATCH /api/notifications/:id/read` — mark a single notification as read.
pub(crate) async fn mark_read(
    State(deps): State<AppDeps>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse> {
    let store = NotificationStore::new(deps.db);
    let notification = store
        .mark_read(id)
        .await?
        .ok_or_else(|| Error::NotFound(format!("notification {id}")))?;
    Ok(Json(notification))
}
