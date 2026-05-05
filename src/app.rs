use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use chrono::{DateTime, Local, Utc};
use eframe::egui;
use crate::chatbot_ctl;
use crate::config::{self, AppConfig, AppPaths, LegacyImportResult, TargetConfig};
use crate::engine;
use crate::engine::EngineAction;
use crate::engine::EngineEvent;
use crate::session_store::{self, RecentSession};
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
pub struct ChatbotAutoApp {
    paths: Arc<AppPaths>,
    config: AppConfig,
    statuses: Vec<TargetRuntimeState>,
    logs: Vec<String>,
    selected_target: Option<usize>,
    last_poll: Instant,
    engine_receiver: Option<Receiver<EngineEvent>>,
    engine_running: bool,
    status_message: String,
    provider_data_dirs: Vec<ProviderDataDir>,
    last_saved_config_snapshot: String,
    import_preview: Option<ImportPreview>,
    import_preview_filter: ImportPreviewFilter,
    import_preview_sort: ImportPreviewSort,
    import_preview_search: String,
    #[allow(dead_code)]
    import_history_expanded: bool,
    preview_focused_target: Option<usize>,
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
impl ImportPreviewFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Add => "Add",
            Self::Update => "Update",
            Self::Unchanged => "Unchanged",
        }
    }}
impl ImportPreviewFilter {
    fn matches(self, kind: ImportPreviewKind) -> bool {
        match self {
            Self::All => true,
            Self::Add => kind == ImportPreviewKind::Add,
            Self::Update => kind == ImportPreviewKind::Update,
            Self::Unchanged => kind == ImportPreviewKind::Unchanged,
        }
    }}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ImportPreviewSort {
    #[default]
    KindThenName,
    ProviderThenName,
    Name,
}
impl ImportPreviewSort {
    fn label(self) -> &'static str {
        match self {
            Self::KindThenName => "Kind then Name",
            Self::ProviderThenName => "Provider then Name",
            Self::Name => "Name",
        }
    }}
#[derive(Clone, Debug, PartialEq, Eq)]
struct ImportPreviewEntry {
    name: String,
    provider: String,
    kind: ImportPreviewKind,
}
#[derive(Clone, Debug)]
struct ImportPreview {
    source_path: String,
    summary: ImportSummary,
    entries: Vec<ImportPreviewEntry>,
    imported_targets: Vec<TargetConfig>,
}
impl ChatbotAutoApp {
    pub fn new(config: AppConfig, paths: Arc<AppPaths>) -> Self {
        let last_saved_config_snapshot = config_snapshot(&config);
        let mut app = Self {
            paths,
            statuses: Vec::new(),
            config,
            logs: Vec::new(),
            selected_target: None,
            last_poll: Instant::now(),
            engine_receiver: None,
            engine_running: false,
            status_message: String::new(),
            provider_data_dirs: discover_provider_data_dirs(),
            last_saved_config_snapshot,
            import_preview: None,
            import_preview_filter: ImportPreviewFilter::All,
            import_preview_sort: ImportPreviewSort::KindThenName,
            import_preview_search: String::new(),
            import_history_expanded: false,
            preview_focused_target: None,
        };
        app.sync_statuses();
        app.refresh_ctl_health();
        app
    }}
impl ChatbotAutoApp {
    fn sync_statuses(&mut self) {
        while self.statuses.len() < self.config.targets.len() {
            self.statuses.push(TargetRuntimeState::default());
        }
        self.statuses.truncate(self.config.targets.len());
        if self.selected_target.is_none() && !self.config.targets.is_empty() {
            self.selected_target = Some(0);
        }
        if matches!(self.selected_target, Some(index) if index >= self.config.targets.len()) {
            self.selected_target = self.config.targets.len().checked_sub(1);
        }
    }}
impl ChatbotAutoApp {
    fn refresh_ctl_health(&mut self) {
        self.status_message = match chatbot_ctl::health_check(&self.config.chatbot_ctl_path) {
            Ok(()) => format!(
                "chatbot-ctl ready: {}",
                self.config.chatbot_ctl_path.display()
            ),
            Err(error) => format!("chatbot-ctl error: {error}"),
        };
    }}
impl ChatbotAutoApp {
    fn run_engine(&mut self, action: EngineAction) {
        if self.engine_running {
            self.logs.push(format!(
                "{} engine run skipped: already running",
                timestamp_now()
            ));
            return;
        }

        self.engine_running = true;
        let (sender, receiver) = mpsc::channel();
        let config = normalized_config_clone(&mut self.config);
        let last_sent_at = self
            .statuses
            .iter()
            .map(|status| status.last_sent_at)
            .collect();
        thread::spawn(move || engine::run(action, config, last_sent_at, sender));
        self.engine_receiver = Some(receiver);
    }}
fn format_timestamp(timestamp: DateTime<Utc>) -> String {
    timestamp
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}
fn timestamp_now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}
fn blank_to_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}
fn provider_path<'a>(
    provider_data_dirs: &'a [ProviderDataDir],
    provider: &str,
) -> Option<&'a std::path::Path> {
    provider_data_dirs
        .iter()
        .find(|option| option.provider == provider)
        .map(|option| option.path.as_path())
}
fn recent_sessions_for_provider(
    provider_data_dirs: &[ProviderDataDir],
    provider: &str,
) -> Vec<RecentSession> {
    let Some(data_dir) = provider_path(provider_data_dirs, provider) else {
        return Vec::new();
    };
    let sessions_json_path = data_dir.join("sessions.json");
    session_store::recent_sessions(&sessions_json_path, 10).unwrap_or_default()
}
fn discover_provider_data_dirs() -> Vec<ProviderDataDir> {
    config::discover_provider_data_dirs()
        .into_iter()
        .map(|(provider, path)| ProviderDataDir { provider, path })
        .collect()
}
fn merge_imported_targets(
    existing: &mut Vec<TargetConfig>,
    imported: Vec<TargetConfig>,
) -> ImportSummary {
    let mut summary = ImportSummary::default();
    for imported_target in imported {
        if let Some(existing_target) = existing.iter_mut().find(|target| {
            target.name == imported_target.name && target.provider == imported_target.provider
        }) {
            let changed = import_managed_fields_changed(existing_target, &imported_target);
            let enabled = existing_target.enabled;
            let cooldown_seconds = existing_target.cooldown_seconds;
            let notes = existing_target.notes.clone();
            let build_docs_before_continue = existing_target.build_docs_before_continue;
            let build_docs_completed = existing_target.build_docs_completed;
            let auto_increment_iterations = existing_target.auto_increment_iterations;
            let current_iteration = existing_target.current_iteration;
            let max_iterations = existing_target.max_iterations;
            let repo_path = existing_target.repo_path.clone();
            let design_document = existing_target.design_document.clone();
            let task_document = existing_target.task_document.clone();

            *existing_target = imported_target;
            existing_target.enabled = enabled;
            existing_target.cooldown_seconds = cooldown_seconds;
            existing_target.notes = notes;
            existing_target.build_docs_before_continue = build_docs_before_continue;
            existing_target.build_docs_completed = build_docs_completed;
            existing_target.auto_increment_iterations = auto_increment_iterations;
            existing_target.current_iteration = current_iteration;
            existing_target.max_iterations = max_iterations;
            existing_target.repo_path = repo_path;
            existing_target.design_document = design_document;
            existing_target.task_document = task_document;
            if changed {
                summary.updated += 1;
            } else {
                summary.unchanged += 1;
            }
        } else {
            existing.push(imported_target);
            summary.added += 1;
        }
        summary.processed += 1;
    }
    summary
}
fn import_managed_fields_changed(existing: &TargetConfig, imported: &TargetConfig) -> bool {
    existing.data_dir != imported.data_dir || existing.continue_text != imported.continue_text
}
fn build_import_preview(
    existing: &[TargetConfig],
    imported_targets: Vec<TargetConfig>,
    source_path: String,
) -> ImportPreview {
    let mut summary = ImportSummary::default();
    let mut entries = Vec::new();

    for imported_target in &imported_targets {
        let kind = match existing.iter().find(|target| {
            target.name == imported_target.name && target.provider == imported_target.provider
        }) {
            None => {
                summary.added += 1;
                ImportPreviewKind::Add
            }
            Some(existing_target)
                if import_managed_fields_changed(existing_target, imported_target) =>
            {
                summary.updated += 1;
                ImportPreviewKind::Update
            }
            Some(_) => {
                summary.unchanged += 1;
                ImportPreviewKind::Unchanged
            }
        };
        summary.processed += 1;
        entries.push(ImportPreviewEntry {
            name: imported_target.name.clone(),
            provider: imported_target.provider.clone(),
            kind,
        });
    }

    ImportPreview {
        source_path,
        summary,
        entries,
        imported_targets,
    }
}
fn config_snapshot(config: &AppConfig) -> String {
    serde_json::to_string(config).unwrap_or_default()
}
fn normalized_config_in_place(config: &mut AppConfig) {
    config::normalize_config(config);
}
fn normalized_config_clone(config: &mut AppConfig) -> AppConfig {
    normalized_config_in_place(config);
    config.clone()
}
fn legacy_import_result(
    outcome: &str,
    source_path: &str,
    summary: ImportSummary,
    detail: String,
) -> LegacyImportResult {
    LegacyImportResult {
        recorded_at: timestamp_now(),
        source_path: source_path.to_owned(),
        outcome: outcome.to_owned(),
        processed: summary.processed,
        added: summary.added,
        updated: summary.updated,
        unchanged: summary.unchanged,
        detail,
    }
}
fn filtered_import_preview_entries(
    entries: &[ImportPreviewEntry],
    filter: ImportPreviewFilter,
) -> Vec<&ImportPreviewEntry> {
    entries
        .iter()
        .filter(|entry| filter.matches(entry.kind))
        .collect()
}
fn searched_sorted_filtered_import_preview_entries<'a>(
    entries: &'a [ImportPreviewEntry],
    filter: ImportPreviewFilter,
    sort: ImportPreviewSort,
    search: &str,
) -> Vec<&'a ImportPreviewEntry> {
    let mut entries = filtered_import_preview_entries(entries, filter);
    if !search.trim().is_empty() {
        let search = search.trim().to_ascii_lowercase();
        entries.retain(|entry| {
            entry.name.to_ascii_lowercase().contains(&search)
                || entry.provider.to_ascii_lowercase().contains(&search)
        });
    }
    entries.sort_by(|left, right| match sort {
        ImportPreviewSort::KindThenName => (
            import_preview_kind_rank(left.kind),
            left.provider.as_str(),
            left.name.as_str(),
        )
            .cmp(&(
                import_preview_kind_rank(right.kind),
                right.provider.as_str(),
                right.name.as_str(),
            )),
        ImportPreviewSort::ProviderThenName => (left.provider.as_str(), left.name.as_str())
            .cmp(&(right.provider.as_str(), right.name.as_str())),
        ImportPreviewSort::Name => left.name.as_str().cmp(right.name.as_str()),
    });
    entries
}
fn import_preview_kind_rank(kind: ImportPreviewKind) -> u8 {
    match kind {
        ImportPreviewKind::Add => 0,
        ImportPreviewKind::Update => 1,
        ImportPreviewKind::Unchanged => 2,
    }
}
#[allow(dead_code)]
fn import_preview_kind_color(kind: ImportPreviewKind) -> egui::Color32 {
    match kind {
        ImportPreviewKind::Add => egui::Color32::LIGHT_GREEN,
        ImportPreviewKind::Update => egui::Color32::YELLOW,
        ImportPreviewKind::Unchanged => egui::Color32::LIGHT_GRAY,
    }
}
fn empty_import_preview_message(filter: ImportPreviewFilter, search: &str) -> String {
    let search = search.trim();
    if !search.is_empty() {
        return format!("No preview entries match search `{search}`.");
    }

    match filter {
        ImportPreviewFilter::All => "No preview entries are available.".to_owned(),
        ImportPreviewFilter::Add => "No add preview entries are available.".to_owned(),
        ImportPreviewFilter::Update => "No update preview entries are available.".to_owned(),
        ImportPreviewFilter::Unchanged => "No unchanged preview entries are available.".to_owned(),
    }
}
fn record_legacy_import_result(config: &mut AppConfig, result: LegacyImportResult) {
    config.last_legacy_import_result = Some(result.clone());
    config.legacy_import_history.insert(0, result);
    config
        .legacy_import_history
        .truncate(LEGACY_IMPORT_HISTORY_LIMIT);
}
fn clear_legacy_import_status(config: &mut AppConfig) {
    config.last_legacy_import_result = None;
    config.legacy_import_history.clear();
}
fn last_legacy_import_result_lines(result: Option<&LegacyImportResult>) -> Vec<String> {
    let Some(result) = result else {
        return vec!["No legacy import has run yet.".to_owned()];
    };

    let mut lines = vec![
        format!("When: {}", result.recorded_at),
        format!("Source: {}", result.source_path),
        format!(
            "Outcome: {} ({}, {} add, {} update, {} unchanged)",
            result.outcome, result.processed, result.added, result.updated, result.unchanged
        ),
    ];
    if !result.detail.trim().is_empty() {
        lines.push(format!("Detail: {}", result.detail));
    }
    lines
}
fn legacy_import_history_lines(history: &[LegacyImportResult], limit: usize) -> Vec<String> {
    if history.is_empty() {
        return Vec::new();
    }

    let mut lines = vec!["Recent import outcomes:".to_owned()];
    for item in history.iter().take(limit) {
        lines.push(format!(
            "- {} | {} | {} | {} add / {} update / {} unchanged",
            item.recorded_at,
            item.outcome,
            item.source_path,
            item.added,
            item.updated,
            item.unchanged
        ));
    }
    lines
}
fn find_target_index_by_identity(
    targets: &[TargetConfig],
    provider: &str,
    name: &str,
) -> Option<usize> {
    targets
        .iter()
        .position(|target| target.provider == provider && target.name == name)
}
fn preview_entry_identity_text(entry: &ImportPreviewEntry) -> String {
    format!("{} / {}", entry.provider, entry.name)
}
fn preview_entry_stable_id(entry: &ImportPreviewEntry) -> String {
    format!("{}::{}", entry.provider, entry.name)
}
fn preview_entry_stable_id_label(entry: &ImportPreviewEntry) -> String {
    format!("id={}", preview_entry_stable_id(entry))
}
fn preview_entry_summary_text(entry: &ImportPreviewEntry) -> String {
    let (badge, detail) = entry.kind.metadata();
    format!(
        "[{badge}] {} - {detail}",
        preview_entry_identity_text(entry)
    )
}
fn visible_preview_summary_text(entries: &[&ImportPreviewEntry]) -> String {
    entries
        .iter()
        .map(|entry| preview_entry_summary_text(entry))
        .collect::<Vec<_>>()
        .join("\n")
}
fn preview_review_snapshot_text(
    source_path: &str,
    summary: ImportSummary,
    filter: ImportPreviewFilter,
    sort: ImportPreviewSort,
    search: &str,
    entries: &[&ImportPreviewEntry],
) -> String {
    let visible_entries = entries
        .iter()
        .map(|entry| {
            format!(
                "{} | id={}",
                preview_entry_summary_text(entry),
                preview_entry_stable_id(entry)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "Source: {source_path}\nFilter: {}\nSort: {}\nSearch: {}\nProcessed {} targets: {} add, {} update, {} unchanged\n\n{}",
        filter.label(),
        sort.label(),
        if search.trim().is_empty() {
            "(none)"
        } else {
            search.trim()
        },
        summary.processed,
        summary.added,
        summary.updated,
        summary.unchanged,
        visible_entries
    )
}
fn preview_review_snapshot_markdown(
    source_path: &str,
    summary: ImportSummary,
    filter: ImportPreviewFilter,
    sort: ImportPreviewSort,
    search: &str,
    entries: &[&ImportPreviewEntry],
) -> String {
    let mut lines = vec![
        "# Legacy Import Review Snapshot".to_owned(),
        format!("- Source: `{source_path}`"),
        format!("- Filter: `{}`", filter.label()),
        format!("- Sort: `{}`", sort.label()),
        format!(
            "- Search: `{}`",
            if search.trim().is_empty() {
                "(none)"
            } else {
                search.trim()
            }
        ),
        format!(
            "- Counts: `{}` processed, `{}` add, `{}` update, `{}` unchanged",
            summary.processed, summary.added, summary.updated, summary.unchanged
        ),
        String::new(),
        "## Visible Entries".to_owned(),
    ];

    for entry in entries {
        lines.push(format!(
            "- {} (`{}`)",
            preview_entry_summary_text(entry),
            preview_entry_stable_id(entry)
        ));
    }

    lines.join("\n")
}
fn preview_review_snapshot_json(
    source_path: &str,
    summary: ImportSummary,
    filter: ImportPreviewFilter,
    sort: ImportPreviewSort,
    search: &str,
    entries: &[&ImportPreviewEntry],
) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "source_path": source_path,
        "filter": filter.label(),
        "sort": sort.label(),
        "search": if search.trim().is_empty() { "(none)" } else { search.trim() },
        "counts": {
            "processed": summary.processed,
            "added": summary.added,
            "updated": summary.updated,
            "unchanged": summary.unchanged,
        },
        "visible_entries": entries.iter().map(|entry| serde_json::json!({
            "id": preview_entry_stable_id(entry),
            "kind": entry.kind.metadata().0,
            "detail": entry.kind.metadata().1,
            "provider": entry.provider,
            "name": entry.name,
            "identity": preview_entry_identity_text(entry),
            "summary": preview_entry_summary_text(entry),
        })).collect::<Vec<_>>(),
    }))
    .unwrap_or_default()
}
fn target_list_label(provider: &str, name: &str, focused_from_preview: bool) -> String {
    let mut label = format!("{provider} / {name}");
    if focused_from_preview {
        label.push_str(" [focused]");
    }
    label
}
fn build_docs_paths_changed(
    design_document_changed: bool,
    task_document_changed: bool,
    previous_design_document: &str,
    current_design_document: &str,
    previous_task_document: &str,
    current_task_document: &str,
) -> bool {
    (design_document_changed && previous_design_document != current_design_document)
        || (task_document_changed && previous_task_document != current_task_document)
}
fn reset_bootstrap_completion_for_targets(targets: &mut [TargetConfig]) {
    for target in targets {
        if target.build_docs_before_continue {
            target.build_docs_completed = false;
        }
    }
}
