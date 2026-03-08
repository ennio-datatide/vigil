//! Filesystem directory listing endpoint for the project path picker.

use axum::extract::Query;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::error::Result;

#[derive(Debug, Deserialize)]
pub(crate) struct DirsQuery {
    prefix: Option<String>,
}

/// List directories matching a prefix for autocomplete.
///
/// - Expands `~` to the user's home directory.
/// - If prefix ends with `/`, lists children of that directory.
/// - Otherwise lists siblings matching the partial name.
/// - Skips hidden directories (starting with `.`).
/// - Returns at most 20 results, sorted alphabetically.
pub(crate) async fn list_dirs(Query(query): Query<DirsQuery>) -> Result<impl IntoResponse> {
    let prefix = query.prefix.unwrap_or_default();

    let expanded = if prefix.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            prefix.replacen('~', &home.display().to_string(), 1)
        } else {
            prefix
        }
    } else {
        prefix
    };

    let path = std::path::Path::new(&expanded);

    let (parent, filter) = if expanded.ends_with('/') || expanded.is_empty() {
        (path, None)
    } else {
        let parent = path.parent().unwrap_or(path);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        (parent, Some(name.to_string()))
    };

    let mut dirs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            if let Some(ref f) = filter
                && !name_str.starts_with(f.as_str())
            {
                continue;
            }
            let full = entry.path().display().to_string() + "/";
            dirs.push(full);
        }
    }

    dirs.sort();
    dirs.truncate(20);

    Ok(Json(json!({ "dirs": dirs })))
}
