//! Skills listing endpoint.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use serde::Serialize;

use crate::deps::AppDeps;
use crate::error::Result;

#[derive(Serialize)]
pub(crate) struct SkillInfo {
    name: String,
    path: String,
}

pub(crate) async fn list_skills(State(deps): State<AppDeps>) -> Result<impl IntoResponse> {
    let skills_dir = &deps.config.skills_dir;
    let mut skills = Vec::new();

    if let Ok(entries) = std::fs::read_dir(skills_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "md")
                && let Some(name) = path.file_stem().and_then(|s| s.to_str())
            {
                skills.push(SkillInfo {
                    name: name.to_string(),
                    path: path.display().to_string(),
                });
            }
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(Json(skills))
}
