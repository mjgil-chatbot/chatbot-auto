use std::cmp::Reverse;
use std::fs;
use std::path::Path;
use anyhow::{Context, Result};
use serde::Deserialize;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecentSession {
    pub id: String,
    pub name: String,
    pub updated_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionMessage {
    pub is_user: bool,
    pub content: String,
}
#[derive(Debug, Deserialize)]
struct SessionsFile {
    sessions: serde_json::Map<String, serde_json::Value>,
}
#[derive(Debug, Deserialize)]
struct SessionInfo {
    name: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    updated_at: String,
}
pub fn recent_sessions(sessions_json_path: &Path, limit: usize) -> Result<Vec<RecentSession>> {
    let contents = fs::read_to_string(sessions_json_path)
        .with_context(|| format!("failed to read {}", sessions_json_path.display()))?;
    let sessions = serde_json::from_str::<SessionsFile>(&contents)
        .with_context(|| format!("failed to parse {}", sessions_json_path.display()))?;

    let mut recent = Vec::new();
    for (session_id, value) in sessions.sessions {
        let info: SessionInfo = serde_json::from_value(value)
            .with_context(|| format!("failed to parse session entry for {}", session_id))?;
        if info.hidden {
            continue;
        }
        recent.push(RecentSession {
            id: session_id,
            name: info.name,
            updated_at: info.updated_at,
        });
    }

    recent.sort_by_key(|session| Reverse(session.updated_at.clone()));
    recent.truncate(limit);
    Ok(recent)
}
