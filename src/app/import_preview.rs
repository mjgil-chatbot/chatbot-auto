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
    pub(super) fn record_legacy_import_result(&mut self, result: LegacyImportResult) {
        record_legacy_import_result(&mut self.config, result);
    }}
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
pub(super) fn record_legacy_import_result(config: &mut AppConfig, result: LegacyImportResult) {
    config.last_legacy_import_result = Some(result.clone());
    config.legacy_import_history.insert(0, result);
    config
        .legacy_import_history
        .truncate(LEGACY_IMPORT_HISTORY_LIMIT);
}
