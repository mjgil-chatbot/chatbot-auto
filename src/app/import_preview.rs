use eframe::egui;
use super::{ChatbotAutoApp, timestamp_now};
use crate::config::{self, AppConfig, LegacyImportResult, TargetConfig};
use crate::legacy_import;
pub(super) const LEGACY_IMPORT_HISTORY_LIMIT: usize = 8;
pub(super) const LEGACY_IMPORT_HISTORY_COMPACT_LIMIT: usize = 3;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ImportSummary {
    pub(super) processed: usize,
    pub(super) added: usize,
    pub(super) updated: usize,
    pub(super) unchanged: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ImportPreviewKind {
    Add,
    Update,
    Unchanged,
}
impl ImportPreviewKind {
    pub(super) fn metadata(self) -> (&'static str, &'static str) {
        match self {
            Self::Add => ("Add", "new target"),
            Self::Update => ("Update", "managed fields changed"),
            Self::Unchanged => ("Unchanged", "already matches import"),
        }
    }}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ImportPreviewFilter {
    #[default]
    All,
    Add,
    Update,
    Unchanged,
}
impl ImportPreviewFilter {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Add => "Add",
            Self::Update => "Update",
            Self::Unchanged => "Unchanged",
        }
    }}
impl ImportPreviewFilter {
    pub(super) fn matches(self, kind: ImportPreviewKind) -> bool {
        match self {
            Self::All => true,
            Self::Add => kind == ImportPreviewKind::Add,
            Self::Update => kind == ImportPreviewKind::Update,
            Self::Unchanged => kind == ImportPreviewKind::Unchanged,
        }
    }}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ImportPreviewSort {
    #[default]
    KindThenName,
    ProviderThenName,
    Name,
}
impl ImportPreviewSort {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::KindThenName => "Kind then Name",
            Self::ProviderThenName => "Provider then Name",
            Self::Name => "Name",
        }
    }}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ImportPreviewEntry {
    pub(super) name: String,
    pub(super) provider: String,
    pub(super) kind: ImportPreviewKind,
}
#[derive(Clone, Debug)]
pub(super) struct ImportPreview {
    pub(super) source_path: String,
    pub(super) summary: ImportSummary,
    pub(super) entries: Vec<ImportPreviewEntry>,
    pub(super) imported_targets: Vec<TargetConfig>,
}
impl ChatbotAutoApp {
    pub(super) fn import_legacy_targets(&mut self) {
        let import_issues =
            config::validate_legacy_watcher_script_path(&self.config.legacy_watcher_script_path);
        if !import_issues.is_empty() {
            let detail = import_issues.join("; ");
            self.record_legacy_import_result(LegacyImportResult {
                recorded_at: timestamp_now(),
                source_path: self.config.legacy_watcher_script_path.clone(),
                outcome: "skipped".to_owned(),
                processed: 0,
                added: 0,
                updated: 0,
                unchanged: 0,
                detail: detail.clone(),
            });
            self.logs.push(format!(
                "{} legacy watcher import skipped: {}",
                timestamp_now(),
                detail
            ));
            return;
        }

        match legacy_import::import_targets_from_watcher_script(std::path::Path::new(
            &self.config.legacy_watcher_script_path,
        )) {
            Ok(imported) => {
                let preview = build_import_preview(
                    &self.config.targets,
                    imported,
                    self.config.legacy_watcher_script_path.clone(),
                );
                self.logs.push(format!(
                    "{} prepared import preview from {} (processed {}, added {}, updated {}, unchanged {})",
                    timestamp_now(),
                    preview.source_path,
                    preview.summary.processed,
                    preview.summary.added,
                    preview.summary.updated,
                    preview.summary.unchanged
                ));
                self.record_legacy_import_result(legacy_import_result(
                    "previewed",
                    &preview.source_path,
                    preview.summary,
                    "import preview prepared".to_owned(),
                ));
                self.import_preview_filter = ImportPreviewFilter::All;
                self.import_preview_sort = ImportPreviewSort::KindThenName;
                self.import_preview_search.clear();
                self.import_preview = Some(preview);
            }
            Err(error) => {
                self.record_legacy_import_result(LegacyImportResult {
                    recorded_at: timestamp_now(),
                    source_path: self.config.legacy_watcher_script_path.clone(),
                    outcome: "failed".to_owned(),
                    processed: 0,
                    added: 0,
                    updated: 0,
                    unchanged: 0,
                    detail: error.to_string(),
                });
                self.logs.push(format!(
                    "{} failed to import legacy targets: {error}",
                    timestamp_now()
                ));
            }
        }
    }}
impl ChatbotAutoApp {
    pub(super) fn apply_import_preview(&mut self) {
        let Some(preview) = self.import_preview.take() else {
            return;
        };

        let summary = merge_imported_targets(&mut self.config.targets, preview.imported_targets);
        self.sync_statuses();
        self.provider_data_dirs = super::discover_provider_data_dirs();
        self.import_preview_filter = ImportPreviewFilter::All;
        self.import_preview_sort = ImportPreviewSort::KindThenName;
        self.import_preview_search.clear();
        self.logs.push(format!(
            "{} applied import from {} (processed {}, added {}, updated {}, unchanged {})",
            timestamp_now(),
            preview.source_path,
            summary.processed,
            summary.added,
            summary.updated,
            summary.unchanged
        ));
        self.record_legacy_import_result(legacy_import_result(
            "applied",
            &preview.source_path,
            summary,
            "import applied".to_owned(),
        ));
    }}
impl ChatbotAutoApp {
    pub(super) fn dismiss_import_preview(&mut self) {
        if let Some(preview) = self.import_preview.take() {
            self.import_preview_filter = ImportPreviewFilter::All;
            self.import_preview_sort = ImportPreviewSort::KindThenName;
            self.import_preview_search.clear();
            self.record_legacy_import_result(legacy_import_result(
                "dismissed",
                &preview.source_path,
                preview.summary,
                "import preview dismissed".to_owned(),
            ));
            self.logs.push(format!(
                "{} dismissed import preview from {}",
                timestamp_now(),
                preview.source_path
            ));
        }
    }}
impl ChatbotAutoApp {
    pub(super) fn record_legacy_import_result(&mut self, result: LegacyImportResult) {
        record_legacy_import_result(&mut self.config, result);
    }}
pub(super) fn merge_imported_targets(
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
pub(super) fn build_import_preview(
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
pub(super) fn filtered_import_preview_entries(
    entries: &[ImportPreviewEntry],
    filter: ImportPreviewFilter,
) -> Vec<&ImportPreviewEntry> {
    entries
        .iter()
        .filter(|entry| filter.matches(entry.kind))
        .collect()
}
pub(super) fn searched_sorted_filtered_import_preview_entries<'a>(
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
pub(super) fn import_preview_kind_color(kind: ImportPreviewKind) -> egui::Color32 {
    match kind {
        ImportPreviewKind::Add => egui::Color32::LIGHT_GREEN,
        ImportPreviewKind::Update => egui::Color32::YELLOW,
        ImportPreviewKind::Unchanged => egui::Color32::LIGHT_GRAY,
    }
}
pub(super) fn empty_import_preview_message(filter: ImportPreviewFilter, search: &str) -> String {
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
pub(super) fn record_legacy_import_result(config: &mut AppConfig, result: LegacyImportResult) {
    config.last_legacy_import_result = Some(result.clone());
    config.legacy_import_history.insert(0, result);
    config
        .legacy_import_history
        .truncate(LEGACY_IMPORT_HISTORY_LIMIT);
}
pub(super) fn clear_legacy_import_status(config: &mut AppConfig) {
    config.last_legacy_import_result = None;
    config.legacy_import_history.clear();
}
pub(super) fn last_legacy_import_result_lines(result: Option<&LegacyImportResult>) -> Vec<String> {
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
pub(super) fn legacy_import_history_lines(
    history: &[LegacyImportResult],
    limit: usize,
) -> Vec<String> {
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
pub(super) fn find_target_index_by_identity(
    targets: &[TargetConfig],
    provider: &str,
    name: &str,
) -> Option<usize> {
    targets
        .iter()
        .position(|target| target.provider == provider && target.name == name)
}
pub(super) fn preview_entry_identity_text(entry: &ImportPreviewEntry) -> String {
    format!("{} / {}", entry.provider, entry.name)
}
fn preview_entry_stable_id(entry: &ImportPreviewEntry) -> String {
    format!("{}::{}", entry.provider, entry.name)
}
pub(super) fn preview_entry_stable_id_label(entry: &ImportPreviewEntry) -> String {
    format!("id={}", preview_entry_stable_id(entry))
}
