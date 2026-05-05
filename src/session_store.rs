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
#[derive(Debug, Deserialize)]
struct TranscriptFile {
    #[serde(default)]
    launched_from: Option<String>,
    #[serde(default)]
    messages: Vec<TranscriptMessage>,
}
#[derive(Debug, Deserialize)]
struct TranscriptMessage {
    is_user: bool,
    #[serde(default)]
    content: String,
}
pub fn session_id_for_name(sessions_json_path: &Path, name: &str) -> Result<Option<String>> {
    let contents = fs::read_to_string(sessions_json_path)
        .with_context(|| format!("failed to read {}", sessions_json_path.display()))?;
    let sessions = serde_json::from_str::<SessionsFile>(&contents)
        .with_context(|| format!("failed to parse {}", sessions_json_path.display()))?;

    let mut matches = Vec::new();
    for (session_id, value) in sessions.sessions {
        let info: SessionInfo = serde_json::from_value(value)
            .with_context(|| format!("failed to parse session entry for {}", session_id))?;
        if info.name == name && !info.hidden {
            matches.push((info.updated_at, session_id));
        }
    }

    matches.sort_by_key(|(updated_at, _)| Reverse(updated_at.clone()));
    Ok(matches.into_iter().next().map(|(_, session_id)| session_id))
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
pub fn load_last_message(session_path: &Path) -> Result<Option<SessionMessage>> {
    let contents = fs::read_to_string(session_path)
        .with_context(|| format!("failed to read {}", session_path.display()))?;
    let transcript = serde_json::from_str::<TranscriptFile>(&contents)
        .with_context(|| format!("failed to parse {}", session_path.display()))?;
    Ok(transcript.messages.last().map(|message| SessionMessage {
        is_user: message.is_user,
        content: message.content.clone(),
    }))
}
