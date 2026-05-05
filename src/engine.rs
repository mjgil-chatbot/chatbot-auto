use std::path::Path;
use std::sync::mpsc::Sender;
use chrono::{DateTime, Utc};
use crate::config::TargetConfig;
use crate::session_store::SessionMessage;
#[derive(Clone, Debug)]
pub enum EngineAction {
    CheckAll,
    CheckTarget(usize),
    ForceSend(usize),
}
#[derive(Clone, Debug)]
pub enum TargetDecision {
    Sent,
    Skipped(String),
    Error(String),
}
#[derive(Clone, Debug)]
pub struct TargetUpdate {
    pub index: usize,
    pub session_id: Option<String>,
    pub worker_running: Option<bool>,
    pub last_message_role: Option<String>,
    pub last_message_preview: Option<String>,
    pub build_docs_completed: Option<bool>,
    pub verification_session_id: Option<String>,
    pub target_override: Option<TargetConfig>,
    pub decision: TargetDecision,
    pub checked_at: DateTime<Utc>,
}
#[derive(Clone, Debug)]
pub enum EngineEvent {
    Log(String),
    TargetUpdated(TargetUpdate),
    Finished,
}
struct EngineDeps {
    session_id_for_name: fn(&Path, &str) -> anyhow::Result<Option<String>>,
    worker_running_for_chat: fn(&Path, &str) -> anyhow::Result<bool>,
    load_last_message: fn(&Path) -> anyhow::Result<Option<SessionMessage>>,
    send_continue: fn(&Path, &str, &str, &str) -> anyhow::Result<()>,
    create_chat: fn(&Path, &str, &str) -> anyhow::Result<String>,
    rename_chat: fn(&Path, &str, &str) -> anyhow::Result<()>,
    kill_chat: fn(&Path, &str) -> anyhow::Result<()>,
}
const NEXT_STEPS_OR_COMPLETION_PROMPT: &str = "Reply with one of the following only:\n\n1. An explicit \"Next Steps:\" section if more work remains.\n2. Exactly \"Design Doc Fully Implemented\" if the current iteration is complete.";
