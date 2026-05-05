use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use chrono::{DateTime, Local, Utc};
use eframe::egui;
const LEGACY_IMPORT_HISTORY_LIMIT: usize = 8;
const LEGACY_IMPORT_HISTORY_COMPACT_LIMIT: usize = 3;
#[derive(Clone, Debug, Default)]
struct TargetRuntimeState {
    session_id: Option<String>,
    worker_running: Option<bool>,
    last_message_role: Option<String>,
    last_message_preview: Option<String>,
    last_decision: String,
    last_checked_at: Option<DateTime<Utc>>,
    last_sent_at: Option<DateTime<Utc>>,
}
#[derive(Clone, Debug)]
struct ProviderDataDir {
    provider: String,
    path: std::path::PathBuf,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ImportSummary {
    processed: usize,
    added: usize,
    updated: usize,
    unchanged: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImportPreviewKind {
    Add,
    Update,
    Unchanged,
}
impl ImportPreviewKind {
    fn metadata(self) -> (&'static str, &'static str) {
        match self {
            Self::Add => ("Add", "new target"),
            Self::Update => ("Update", "managed fields changed"),
            Self::Unchanged => ("Unchanged", "already matches import"),
        }
    }}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ImportPreviewFilter {
    #[default]
    All,
    Add,
    Update,
    Unchanged,
}
