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

pub fn load_launched_from(session_path: &Path) -> Result<Option<String>> {
    let contents = fs::read_to_string(session_path)
        .with_context(|| format!("failed to read {}", session_path.display()))?;
    let transcript = serde_json::from_str::<TranscriptFile>(&contents)
        .with_context(|| format!("failed to parse {}", session_path.display()))?;
    Ok(transcript
        .launched_from
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty()))
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{load_last_message, load_launched_from, recent_sessions, session_id_for_name};

    fn unique_temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("chatbot-auto-tests-{nanos}"));
        std::fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn resolves_most_recent_visible_session() {
        let dir = unique_temp_dir();
        let path = dir.join("sessions.json");
        std::fs::write(
            &path,
            r#"{
                "sessions": {
                    "a": {"name": "ziglua", "updated_at": "2026-05-05T01:00:00Z", "hidden": false},
                    "b": {"name": "ziglua", "updated_at": "2026-05-05T02:00:00Z", "hidden": true},
                    "c": {"name": "ziglua", "updated_at": "2026-05-05T03:00:00Z", "hidden": false}
                }
            }"#,
        )
        .expect("test fixture should be written");

        let resolved = session_id_for_name(&path, "ziglua").expect("session lookup should work");
        assert_eq!(resolved.as_deref(), Some("c"));
    }

    #[test]
    fn loads_last_message() {
        let dir = unique_temp_dir();
        let path = dir.join("session.json");
        std::fs::write(
            &path,
            r#"{
                "messages": [
                    {"is_user": true, "content": "start"},
                    {"is_user": false, "content": "Next Steps:\n1. Continue with tests"}
                ]
            }"#,
        )
        .expect("test fixture should be written");

        let message = load_last_message(&path)
            .expect("transcript should parse")
            .expect("message should exist");
        assert!(!message.is_user);
        assert!(message.content.contains("Next Steps"));
    }

    #[test]
    fn lists_top_recent_visible_sessions() {
        let dir = unique_temp_dir();
        let path = dir.join("sessions.json");
        std::fs::write(
            &path,
            r#"{
                "sessions": {
                    "a": {"name": "one", "updated_at": "2026-05-05T01:00:00Z", "hidden": false},
                    "b": {"name": "two", "updated_at": "2026-05-05T02:00:00Z", "hidden": false},
                    "c": {"name": "three", "updated_at": "2026-05-05T03:00:00Z", "hidden": true},
                    "d": {"name": "four", "updated_at": "2026-05-05T04:00:00Z", "hidden": false}
                }
            }"#,
        )
        .expect("test fixture should be written");

        let recent = recent_sessions(&path, 2).expect("recent sessions should load");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].id, "d");
        assert_eq!(recent[0].name, "four");
        assert_eq!(recent[1].id, "b");
        assert_eq!(recent[1].name, "two");
    }

    #[test]
    fn loads_launched_from_repo_path() {
        let dir = unique_temp_dir();
        let path = dir.join("session.json");
        std::fs::write(
            &path,
            r#"{
                "launched_from": "/tmp/repo",
                "messages": [
                    {"is_user": true, "content": "start"}
                ]
            }"#,
        )
        .expect("test fixture should be written");

        let launched_from = load_launched_from(&path).expect("transcript should parse");
        assert_eq!(launched_from.as_deref(), Some("/tmp/repo"));
    }
}
