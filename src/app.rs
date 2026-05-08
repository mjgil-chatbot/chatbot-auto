use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, Utc};
use eframe::egui;

use crate::chatbot_ctl;
use crate::config::{self, AppConfig, AppPaths, LegacyImportResult, TargetConfig};
use crate::engine::{self, EngineAction, EngineEvent, TargetDecision, TargetUpdate};
use crate::legacy_import;
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
    }
}

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
    }

    fn matches(self, kind: ImportPreviewKind) -> bool {
        match self {
            Self::All => true,
            Self::Add => kind == ImportPreviewKind::Add,
            Self::Update => kind == ImportPreviewKind::Update,
            Self::Unchanged => kind == ImportPreviewKind::Unchanged,
        }
    }
}

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
    }
}

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
    }

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
    }

    fn refresh_ctl_health(&mut self) {
        self.status_message = match chatbot_ctl::health_check(&self.config.chatbot_ctl_path) {
            Ok(()) => format!(
                "chatbot-ctl ready: {}",
                self.config.chatbot_ctl_path.display()
            ),
            Err(error) => format!("chatbot-ctl error: {error}"),
        };
    }

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
    }

    fn drain_engine_events(&mut self) {
        let mut finished = false;
        let mut pending = Vec::new();

        if let Some(receiver) = &self.engine_receiver {
            while let Ok(event) = receiver.try_recv() {
                pending.push(event);
            }
        }

        for event in pending {
            match event {
                EngineEvent::Log(line) => {
                    if let Err(error) = config::append_log_line(&self.paths.log_path, &line) {
                        self.logs
                            .push(format!("{} failed to write log: {error}", timestamp_now()));
                    }
                    self.logs.push(line);
                    if self.logs.len() > 500 {
                        let drain = self.logs.len() - 500;
                        self.logs.drain(0..drain);
                    }
                }
                EngineEvent::TargetUpdated(update) => self.apply_update(update),
                EngineEvent::Finished => finished = true,
            }
        }

        if finished {
            self.engine_running = false;
            self.engine_receiver = None;
        }
    }

    fn apply_update(&mut self, update: TargetUpdate) {
        let build_docs_completed = update.build_docs_completed;
        let target_override = update.target_override;
        let Some(state) = self.statuses.get_mut(update.index) else {
            return;
        };

        state.session_id = update.session_id;
        state.worker_running = update.worker_running;
        state.last_message_role = update.last_message_role;
        state.last_message_preview = update.last_message_preview;
        state.last_checked_at = Some(update.checked_at);
        state.last_decision = match update.decision {
            TargetDecision::Sent => {
                state.last_sent_at = Some(update.checked_at);
                "sent continue".to_owned()
            }
            TargetDecision::Skipped(reason) => reason,
            TargetDecision::Error(error) => format!("error: {error}"),
        };

        if let Some(completed) = build_docs_completed
            && let Some(target) = self.config.targets.get_mut(update.index)
        {
            target.build_docs_completed = completed;
        }
        if let Some(target_override) = target_override
            && let Some(target) = self.config.targets.get_mut(update.index)
        {
            *target = target_override;
        }
    }

    fn maybe_run_poll(&mut self) {
        if !self.config.automation_enabled || self.engine_running {
            return;
        }

        let interval = Duration::from_secs(self.config.poll_interval_seconds.max(5));
        if self.last_poll.elapsed() >= interval {
            self.last_poll = Instant::now();
            self.run_engine(EngineAction::CheckAll);
        }
    }

    fn add_target(&mut self) {
        self.config.targets.push(TargetConfig::default());
        self.sync_statuses();
        self.selected_target = self.config.targets.len().checked_sub(1);
    }

    fn delete_selected_target(&mut self) {
        let Some(index) = self.selected_target else {
            return;
        };
        if index < self.config.targets.len() {
            self.config.targets.remove(index);
            self.sync_statuses();
        }
    }

    fn import_legacy_targets(&mut self) {
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
    }

    fn apply_import_preview(&mut self) {
        let Some(preview) = self.import_preview.take() else {
            return;
        };

        let summary = merge_imported_targets(&mut self.config.targets, preview.imported_targets);
        self.sync_statuses();
        self.provider_data_dirs = discover_provider_data_dirs();
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
    }

    fn dismiss_import_preview(&mut self) {
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
    }

    fn persist_config_if_needed(&mut self) {
        normalized_config_in_place(&mut self.config);
        let snapshot = config_snapshot(&self.config);
        if snapshot == self.last_saved_config_snapshot {
            return;
        }

        if config::save(self.paths.as_ref(), &self.config).is_ok() {
            self.last_saved_config_snapshot = snapshot;
        }
    }

    fn record_legacy_import_result(&mut self, result: LegacyImportResult) {
        record_legacy_import_result(&mut self.config, result);
    }
}

impl eframe::App for ChatbotAutoApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain_engine_events();
        self.maybe_run_poll();
        self.refresh_ctl_health();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut self.config.automation_enabled, "Automation");
                ui.label("Poll interval (seconds)");
                ui.add(
                    egui::DragValue::new(&mut self.config.poll_interval_seconds).range(5..=3600),
                );
                if ui.button("Check Now").clicked() {
                    self.last_poll = Instant::now();
                    self.run_engine(EngineAction::CheckAll);
                }
                if ui.button("Save Config").clicked() {
                    match config::save(self.paths.as_ref(), &self.config) {
                        Ok(()) => {
                            self.last_saved_config_snapshot = config_snapshot(&self.config);
                            self.logs.push(format!(
                                "{} saved config to {}",
                                timestamp_now(),
                                self.paths.config_path.display()
                            ))
                        }
                        Err(error) => self.logs.push(format!(
                            "{} failed to save config: {error}",
                            timestamp_now()
                        )),
                    }
                }
                if ui.button("Reload Config").clicked() {
                    match config::load_or_default(self.paths.as_ref()) {
                        Ok(config) => {
                            self.config = config;
                            self.sync_statuses();
                            self.provider_data_dirs = discover_provider_data_dirs();
                            self.last_saved_config_snapshot = config_snapshot(&self.config);
                            self.logs.push(format!(
                                "{} reloaded config from {}",
                                timestamp_now(),
                                self.paths.config_path.display()
                            ));
                        }
                        Err(error) => self.logs.push(format!(
                            "{} failed to reload config: {error}",
                            timestamp_now()
                        )),
                    }
                }
                if ui.button("Add Target").clicked() {
                    self.add_target();
                }
                if ui.button("Delete Target").clicked() {
                    self.delete_selected_target();
                }
            });
            ui.label(&self.status_message);
            egui::CollapsingHeader::new("Global defaults")
                .default_open(false)
                .show(ui, |ui| {
                    let mut build_docs_prompt_changed = false;
                    ui.columns(2, |columns| {
                        columns[0].label("Default continue text");
                        columns[0].add(
                            egui::TextEdit::multiline(&mut self.config.default_continue_text)
                                .desired_rows(6),
                        );
                        columns[1].label("Default build design + task-list prompt");
                        build_docs_prompt_changed = columns[1]
                            .add(
                                egui::TextEdit::multiline(
                                    &mut self.config.default_build_docs_prompt,
                                )
                                .desired_rows(6),
                            )
                            .changed();
                    });
                    if build_docs_prompt_changed {
                        reset_bootstrap_completion_for_targets(&mut self.config.targets);
                    }
                });
            egui::CollapsingHeader::new("Diagnostics")
                .default_open(false)
                .show(ui, |ui| {
                    ui.monospace(format!("Config: {}", self.paths.config_path.display()));
                    ui.monospace(format!("Log file: {}", self.paths.log_path.display()));
                    ui.monospace(format!("Config dir: {}", self.paths.config_dir.display()));
                    ui.monospace(format!("State dir: {}", self.paths.log_dir.display()));
                });
        });

        egui::SidePanel::right("target_details")
            .resizable(true)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.heading("Target Details");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let provider_data_dirs = self.provider_data_dirs.clone();
                    if let Some(index) = self.selected_target {
                        if let Some(target) = self.config.targets.get_mut(index) {
                            let status = &self.statuses[index];
                            if self.preview_focused_target == Some(index) {
                                ui.colored_label(
                                    egui::Color32::LIGHT_BLUE,
                                    "Focused from import preview",
                                );
                                ui.separator();
                            }
                            ui.label(format!(
                                "Last decision: {}",
                                blank_to_dash(&status.last_decision)
                            ));
                            ui.label(format!(
                                "Last checked: {}",
                                status
                                    .last_checked_at
                                    .map(format_timestamp)
                                    .unwrap_or_else(|| "-".to_owned())
                            ));
                            ui.label(format!(
                                "Last sent: {}",
                                status
                                    .last_sent_at
                                    .map(format_timestamp)
                                    .unwrap_or_else(|| "-".to_owned())
                            ));
                            ui.label(format!(
                                "Worker running: {}",
                                status
                                    .worker_running
                                    .map(|value| if value { "yes" } else { "no" })
                                    .unwrap_or("-")
                            ));
                            ui.label(format!(
                                "Last message role: {}",
                                status.last_message_role.as_deref().unwrap_or("-")
                            ));
                            ui.label(format!(
                                "Last message preview: {}",
                                status.last_message_preview.as_deref().unwrap_or("-")
                            ));

                            ui.separator();
                            ui.checkbox(&mut target.enabled, "Enabled");
                            ui.horizontal(|ui| {
                                ui.label("Provider");
                                let selected_text = if target.provider.is_empty() {
                                    "Select provider".to_owned()
                                } else {
                                    target.provider.clone()
                                };
                                egui::ComboBox::from_id_salt(("provider", index))
                                    .selected_text(selected_text)
                                    .show_ui(ui, |ui| {
                                        for option in &provider_data_dirs {
                                            if ui
                                                .selectable_label(
                                                    target.provider == option.provider,
                                                    &option.provider,
                                                )
                                                .clicked()
                                            {
                                                target.provider = option.provider.clone();
                                                target.data_dir = option.path.clone();
                                                target.build_docs_completed = false;
                                            }
                                        }
                                    });
                            });
                            let recent_sessions =
                                recent_sessions_for_provider(&provider_data_dirs, &target.provider);
                            ui.horizontal(|ui| {
                                ui.label("Name");
                                let selected_text = if target.name.trim().is_empty() {
                                    "Select recent chat".to_owned()
                                } else {
                                    target.name.clone()
                                };
                                egui::ComboBox::from_id_salt(("target_name", index))
                                    .selected_text(selected_text)
                                    .show_ui(ui, |ui| {
                                        for session in &recent_sessions {
                                            if ui
                                                .selectable_label(
                                                    target.name == session.name,
                                                    &session.name,
                                                )
                                                .clicked()
                                            {
                                                apply_session_selection(
                                                    target,
                                                    &provider_data_dirs,
                                                    session,
                                                );
                                            }
                                        }
                                    });
                            });
                            if !target.repo_path.as_os_str().is_empty() {
                                ui.label(format!("Repo: {}", target.repo_path.display()));
                            }
                            let build_docs_toggled = ui
                                .checkbox(
                                    &mut target.build_docs_before_continue,
                                    "Build design + task-list before continue loop",
                                )
                                .changed();
                            if build_docs_toggled {
                                target.build_docs_completed = false;
                            }
                            if target.build_docs_before_continue
                                && (target.design_document.is_empty()
                                    || target.task_document.is_empty())
                            {
                                config::apply_default_doc_paths(target);
                            }
                            ui.label(format!(
                                "Build-doc bootstrap sent: {}",
                                if target.build_docs_completed {
                                    "yes"
                                } else {
                                    "no"
                                }
                            ));
                            ui.checkbox(
                                &mut target.auto_increment_iterations,
                                "Auto increment design/task iteration after completion",
                            );
                            ui.horizontal(|ui| {
                                ui.label("Current iteration");
                                ui.monospace(target.current_iteration.to_string());
                            });
                            ui.horizontal(|ui| {
                                ui.label("Max iterations");
                                ui.add(
                                    egui::DragValue::new(&mut target.max_iterations)
                                        .range(target.current_iteration..=1000),
                                );
                            });
                            let bootstrap_issues = config::validate_bootstrap_target(target);
                            if !bootstrap_issues.is_empty() {
                                ui.colored_label(
                                    egui::Color32::YELLOW,
                                    "Bootstrap validation issues:",
                                );
                                for issue in bootstrap_issues {
                                    ui.label(format!("- {issue}"));
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.label("Cooldown");
                                ui.add(
                                    egui::DragValue::new(&mut target.cooldown_seconds)
                                        .range(0..=86400),
                                );
                            });
                            ui.label("Continue text");
                            ui.add(
                                egui::TextEdit::multiline(&mut target.continue_text)
                                    .desired_rows(5),
                            );
                            if ui.button("Use Default Continue Text").clicked() {
                                target.continue_text = self.config.default_continue_text.clone();
                            }
                            ui.label("Design document");
                            let previous_design_document = target.design_document.clone();
                            let design_document_changed = ui
                                .text_edit_singleline(&mut target.design_document)
                                .changed();
                            ui.label("Task document");
                            let previous_task_document = target.task_document.clone();
                            let task_document_changed =
                                ui.text_edit_singleline(&mut target.task_document).changed();
                            if build_docs_paths_changed(
                                design_document_changed,
                                task_document_changed,
                                &previous_design_document,
                                &target.design_document,
                                &previous_task_document,
                                &target.task_document,
                            ) {
                                target.build_docs_completed = false;
                            }
                            ui.label("Composed continue text");
                            let mut preview = config::build_continue_text(
                                &self.config.default_continue_text,
                                target,
                            );
                            if ui.button("Copy composed continue text").clicked() {
                                ui.ctx().copy_text(preview.clone());
                            }
                            ui.add(
                                egui::TextEdit::multiline(&mut preview)
                                    .desired_rows(8)
                                    .interactive(false),
                            );
                            ui.label("Notes");
                            ui.add(egui::TextEdit::multiline(&mut target.notes).desired_rows(4));
                        }
                    } else {
                        ui.label("No target selected.");
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Targets");
            ui.separator();
            let logs_height = 180.0;
            let targets_height = (ui.available_height() - logs_height - 48.0).max(140.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), targets_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("targets_scroll")
                        .show(ui, |ui| {
                            let mut queued_action = None;
                            egui::Grid::new("targets_grid")
                                .num_columns(6)
                                .min_col_width(70.0)
                                .striped(true)
                                .show(ui, |ui| {
                                    ui.strong("Enabled");
                                    ui.strong("Target");
                                    ui.strong("Session");
                                    ui.strong("Decision");
                                    ui.strong("Checked");
                                    ui.strong("Actions");
                                    ui.end_row();

                                    for (index, target) in
                                        self.config.targets.iter_mut().enumerate()
                                    {
                                        let status = &self.statuses[index];
                                        ui.checkbox(&mut target.enabled, "");
                                        let label = target_list_label(
                                            &target.provider,
                                            &target.name,
                                            self.preview_focused_target == Some(index),
                                        );
                                        if ui
                                            .selectable_label(
                                                self.selected_target == Some(index),
                                                label,
                                            )
                                            .clicked()
                                        {
                                            if self.preview_focused_target != Some(index) {
                                                self.preview_focused_target = None;
                                            }
                                            self.selected_target = Some(index);
                                        }
                                        ui.label(status.session_id.as_deref().unwrap_or("-"));
                                        ui.label(if status.last_decision.is_empty() {
                                            "-"
                                        } else {
                                            &status.last_decision
                                        });
                                        ui.label(
                                            status
                                                .last_checked_at
                                                .map(format_timestamp)
                                                .unwrap_or_else(|| "-".to_owned()),
                                        );
                                        ui.horizontal(|ui| {
                                            if ui.button("Check").clicked() {
                                                queued_action =
                                                    Some(EngineAction::CheckTarget(index));
                                            }
                                            if ui.button("Send").clicked() {
                                                queued_action =
                                                    Some(EngineAction::ForceSend(index));
                                            }
                                        });
                                        ui.end_row();
                                    }
                                });
                            if let Some(action) = queued_action {
                                self.run_engine(action);
                            }
                        });
                },
            );
            ui.separator();
            ui.heading("Recent Logs");
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), logs_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("recent_logs_scroll")
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.monospace(line);
                            }
                        });
                },
            );
        });

        self.persist_config_if_needed();
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

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

fn apply_session_selection(
    target: &mut TargetConfig,
    provider_data_dirs: &[ProviderDataDir],
    session: &RecentSession,
) {
    target.name = session.name.clone();
    target.build_docs_completed = false;

    let Some(data_dir) = provider_path(provider_data_dirs, &target.provider) else {
        return;
    };
    target.data_dir = data_dir.to_path_buf();

    let session_path = data_dir
        .join("sessions")
        .join(format!("{}.json", session.id));
    if let Ok(Some(launched_from)) = session_store::load_launched_from(&session_path) {
        let repo_path = std::path::PathBuf::from(launched_from);
        if target.repo_path != repo_path {
            target.repo_path = repo_path;
            if target.build_docs_before_continue {
                config::apply_default_doc_paths(target);
            }
        } else if target.build_docs_before_continue
            && (target.design_document.is_empty() || target.task_document.is_empty())
        {
            config::apply_default_doc_paths(target);
        }
    }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::AppPaths;

    fn temp_app_paths() -> AppPaths {
        let root = std::env::temp_dir().join(format!(
            "chatbot-auto-app-tests-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time should move forward")
                .as_nanos()
        ));
        let config_dir = root.join("config");
        let log_dir = root.join("state");
        std::fs::create_dir_all(&config_dir).expect("config dir should be created");
        std::fs::create_dir_all(&log_dir).expect("state dir should be created");
        AppPaths {
            config_path: config_dir.join("config.json"),
            log_path: log_dir.join("app.log"),
            config_dir,
            log_dir,
        }
    }

    #[test]
    fn build_docs_paths_changed_detects_actual_document_edits() {
        assert!(build_docs_paths_changed(
            true,
            false,
            "/tmp/design.md",
            "/tmp/design-2.md",
            "/tmp/task.md",
            "/tmp/task.md",
        ));
        assert!(build_docs_paths_changed(
            false,
            true,
            "/tmp/design.md",
            "/tmp/design.md",
            "/tmp/task.md",
            "/tmp/task-2.md",
        ));
        assert!(!build_docs_paths_changed(
            true,
            false,
            "/tmp/design.md",
            "/tmp/design.md",
            "/tmp/task.md",
            "/tmp/task.md",
        ));
    }

    #[test]
    fn normalized_config_clone_expands_paths_and_fills_provider_dirs() {
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .expect("home dir should exist");
        let mut config = AppConfig {
            legacy_watcher_script_path: "~/git/bash-snippets/codex-rust-chatbot-continue.sh"
                .to_owned(),
            targets: vec![TargetConfig {
                provider: "codex".to_owned(),
                data_dir: std::path::PathBuf::new(),
                repo_path: "~/git/demo".into(),
                design_document: "~/git/demo/docs/design.md".to_owned(),
                task_document: "~/git/demo/docs/task.md".to_owned(),
                ..TargetConfig::default()
            }],
            ..AppConfig::default()
        };

        let normalized = normalized_config_clone(&mut config);

        assert_eq!(
            normalized.legacy_watcher_script_path,
            home.join("git/bash-snippets/codex-rust-chatbot-continue.sh")
                .display()
                .to_string()
        );
        assert_eq!(
            normalized.targets[0].data_dir,
            home.join(".rust-chatbot/.codex-desktop")
        );
        assert_eq!(normalized.targets[0].repo_path, home.join("git/demo"));
        assert_eq!(config.targets[0].repo_path, home.join("git/demo"));
    }

    #[test]
    fn persist_config_if_needed_normalizes_before_save() {
        let paths = temp_app_paths();
        let config_path = paths.config_path.clone();
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .expect("home dir should exist");
        let config = AppConfig {
            preview_show_stable_ids: true,
            legacy_watcher_script_path: "~/git/bash-snippets/codex-rust-chatbot-continue.sh"
                .to_owned(),
            targets: vec![TargetConfig {
                provider: "codex".to_owned(),
                data_dir: std::path::PathBuf::new(),
                repo_path: "~/git/demo".into(),
                design_document: "~/git/demo/docs/design.md".to_owned(),
                task_document: "~/git/demo/docs/task.md".to_owned(),
                ..TargetConfig::default()
            }],
            ..AppConfig::default()
        };
        let mut app = ChatbotAutoApp::new(config, Arc::new(paths));

        app.persist_config_if_needed();

        assert_eq!(app.config.targets[0].repo_path, home.join("git/demo"));
        assert!(app.config.preview_show_stable_ids);
        let saved = std::fs::read_to_string(&config_path).expect("saved config should be readable");
        assert!(saved.contains(&home.join("git/demo").display().to_string()));
        assert!(saved.contains(".rust-chatbot/.codex-desktop"));
        assert!(saved.contains("\"preview_show_stable_ids\": true"));
    }

    #[test]
    fn reset_bootstrap_completion_only_resets_bootstrap_targets() {
        let mut targets = vec![
            TargetConfig {
                build_docs_before_continue: true,
                build_docs_completed: true,
                ..TargetConfig::default()
            },
            TargetConfig {
                build_docs_before_continue: false,
                build_docs_completed: true,
                ..TargetConfig::default()
            },
        ];

        reset_bootstrap_completion_for_targets(&mut targets);

        assert!(!targets[0].build_docs_completed);
        assert!(targets[1].build_docs_completed);
    }

    #[test]
    fn merge_imported_targets_updates_matching_target_but_preserves_local_fields() {
        let mut existing = vec![TargetConfig {
            name: "ziglua".to_owned(),
            provider: "minimax".to_owned(),
            data_dir: "/old/minimax".into(),
            continue_text: "old".to_owned(),
            enabled: false,
            cooldown_seconds: 42,
            notes: "keep me".to_owned(),
            repo_path: "/repo".into(),
            design_document: "/repo/docs/design.md".to_owned(),
            task_document: "/repo/docs/task.md".to_owned(),
            build_docs_before_continue: true,
            build_docs_completed: true,
            auto_increment_iterations: true,
            current_iteration: 2,
            max_iterations: 5,
            ..TargetConfig::default()
        }];

        let summary = merge_imported_targets(
            &mut existing,
            vec![TargetConfig {
                name: "ziglua".to_owned(),
                provider: "minimax".to_owned(),
                data_dir: "/new/minimax".into(),
                continue_text: "new".to_owned(),
                ..TargetConfig::default()
            }],
        );

        assert_eq!(
            summary,
            ImportSummary {
                processed: 1,
                added: 0,
                updated: 1,
                unchanged: 0,
            }
        );
        assert_eq!(existing.len(), 1);
        assert_eq!(
            existing[0].data_dir,
            std::path::PathBuf::from("/new/minimax")
        );
        assert_eq!(existing[0].continue_text, "new");
        assert!(!existing[0].enabled);
        assert_eq!(existing[0].cooldown_seconds, 42);
        assert_eq!(existing[0].notes, "keep me");
        assert_eq!(existing[0].repo_path, std::path::PathBuf::from("/repo"));
        assert_eq!(existing[0].design_document, "/repo/docs/design.md");
        assert_eq!(existing[0].task_document, "/repo/docs/task.md");
        assert!(existing[0].build_docs_before_continue);
        assert!(existing[0].build_docs_completed);
        assert!(existing[0].auto_increment_iterations);
        assert_eq!(existing[0].current_iteration, 2);
        assert_eq!(existing[0].max_iterations, 5);
    }

    #[test]
    fn merge_imported_targets_appends_new_target() {
        let mut existing = Vec::new();
        let summary = merge_imported_targets(
            &mut existing,
            vec![TargetConfig {
                name: "chatbot-auto".to_owned(),
                provider: "codex".to_owned(),
                data_dir: "/tmp/codex".into(),
                continue_text: "continue".to_owned(),
                ..TargetConfig::default()
            }],
        );

        assert_eq!(
            summary,
            ImportSummary {
                processed: 1,
                added: 1,
                updated: 0,
                unchanged: 0,
            }
        );
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].name, "chatbot-auto");
    }

    #[test]
    fn merge_imported_targets_counts_unchanged_matching_target() {
        let mut existing = vec![TargetConfig {
            name: "ziglua".to_owned(),
            provider: "minimax".to_owned(),
            data_dir: "/tmp/minimax".into(),
            continue_text: "continue".to_owned(),
            ..TargetConfig::default()
        }];

        let summary = merge_imported_targets(
            &mut existing,
            vec![TargetConfig {
                name: "ziglua".to_owned(),
                provider: "minimax".to_owned(),
                data_dir: "/tmp/minimax".into(),
                continue_text: "continue".to_owned(),
                ..TargetConfig::default()
            }],
        );

        assert_eq!(
            summary,
            ImportSummary {
                processed: 1,
                added: 0,
                updated: 0,
                unchanged: 1,
            }
        );
    }

    #[test]
    fn build_import_preview_classifies_add_update_and_unchanged_targets() {
        let existing = vec![
            TargetConfig {
                name: "ziglua".to_owned(),
                provider: "minimax".to_owned(),
                data_dir: "/old/minimax".into(),
                continue_text: "old".to_owned(),
                ..TargetConfig::default()
            },
            TargetConfig {
                name: "chatbot-auto".to_owned(),
                provider: "codex".to_owned(),
                data_dir: "/tmp/codex".into(),
                continue_text: "continue".to_owned(),
                ..TargetConfig::default()
            },
        ];

        let preview = build_import_preview(
            &existing,
            vec![
                TargetConfig {
                    name: "ziglua".to_owned(),
                    provider: "minimax".to_owned(),
                    data_dir: "/new/minimax".into(),
                    continue_text: "new".to_owned(),
                    ..TargetConfig::default()
                },
                TargetConfig {
                    name: "chatbot-auto".to_owned(),
                    provider: "codex".to_owned(),
                    data_dir: "/tmp/codex".into(),
                    continue_text: "continue".to_owned(),
                    ..TargetConfig::default()
                },
                TargetConfig {
                    name: "new-target".to_owned(),
                    provider: "claude".to_owned(),
                    data_dir: "/tmp/claude".into(),
                    continue_text: "continue".to_owned(),
                    ..TargetConfig::default()
                },
            ],
            "/tmp/watcher.sh".to_owned(),
        );

        assert_eq!(
            preview.summary,
            ImportSummary {
                processed: 3,
                added: 1,
                updated: 1,
                unchanged: 1,
            }
        );
        assert_eq!(preview.entries.len(), 3);
        assert_eq!(preview.entries[0].kind, ImportPreviewKind::Update);
        assert_eq!(preview.entries[1].kind, ImportPreviewKind::Unchanged);
        assert_eq!(preview.entries[2].kind, ImportPreviewKind::Add);
    }

    #[test]
    fn filtered_import_preview_entries_respect_selected_filter() {
        let entries = vec![
            ImportPreviewEntry {
                name: "add-me".to_owned(),
                provider: "codex".to_owned(),
                kind: ImportPreviewKind::Add,
            },
            ImportPreviewEntry {
                name: "update-me".to_owned(),
                provider: "minimax".to_owned(),
                kind: ImportPreviewKind::Update,
            },
            ImportPreviewEntry {
                name: "leave-me".to_owned(),
                provider: "claude".to_owned(),
                kind: ImportPreviewKind::Unchanged,
            },
        ];

        assert_eq!(
            filtered_import_preview_entries(&entries, ImportPreviewFilter::All).len(),
            3
        );
        assert_eq!(
            filtered_import_preview_entries(&entries, ImportPreviewFilter::Add).len(),
            1
        );
        assert_eq!(
            filtered_import_preview_entries(&entries, ImportPreviewFilter::Update).len(),
            1
        );
        assert_eq!(
            filtered_import_preview_entries(&entries, ImportPreviewFilter::Unchanged).len(),
            1
        );
    }

    #[test]
    fn searched_sorted_filtered_import_preview_entries_support_multiple_sort_orders() {
        let entries = vec![
            ImportPreviewEntry {
                name: "beta".to_owned(),
                provider: "codex".to_owned(),
                kind: ImportPreviewKind::Update,
            },
            ImportPreviewEntry {
                name: "alpha".to_owned(),
                provider: "minimax".to_owned(),
                kind: ImportPreviewKind::Add,
            },
            ImportPreviewEntry {
                name: "gamma".to_owned(),
                provider: "claude".to_owned(),
                kind: ImportPreviewKind::Unchanged,
            },
        ];

        let kind_then_name = searched_sorted_filtered_import_preview_entries(
            &entries,
            ImportPreviewFilter::All,
            ImportPreviewSort::KindThenName,
            "",
        );
        assert_eq!(kind_then_name[0].name, "alpha");
        assert_eq!(kind_then_name[1].name, "beta");
        assert_eq!(kind_then_name[2].name, "gamma");

        let provider_then_name = searched_sorted_filtered_import_preview_entries(
            &entries,
            ImportPreviewFilter::All,
            ImportPreviewSort::ProviderThenName,
            "",
        );
        assert_eq!(provider_then_name[0].provider, "claude");
        assert_eq!(provider_then_name[1].provider, "codex");
        assert_eq!(provider_then_name[2].provider, "minimax");

        let name_sort = searched_sorted_filtered_import_preview_entries(
            &entries,
            ImportPreviewFilter::All,
            ImportPreviewSort::Name,
            "",
        );
        assert_eq!(name_sort[0].name, "alpha");
        assert_eq!(name_sort[1].name, "beta");
        assert_eq!(name_sort[2].name, "gamma");
    }

    #[test]
    fn searched_sorted_filtered_import_preview_entries_match_name_or_provider() {
        let entries = vec![
            ImportPreviewEntry {
                name: "ziglua".to_owned(),
                provider: "minimax".to_owned(),
                kind: ImportPreviewKind::Update,
            },
            ImportPreviewEntry {
                name: "chatbot-auto".to_owned(),
                provider: "codex".to_owned(),
                kind: ImportPreviewKind::Add,
            },
            ImportPreviewEntry {
                name: "rustbeam".to_owned(),
                provider: "claude".to_owned(),
                kind: ImportPreviewKind::Unchanged,
            },
        ];

        let by_name = searched_sorted_filtered_import_preview_entries(
            &entries,
            ImportPreviewFilter::All,
            ImportPreviewSort::Name,
            "zig",
        );
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].name, "ziglua");

        let by_provider = searched_sorted_filtered_import_preview_entries(
            &entries,
            ImportPreviewFilter::All,
            ImportPreviewSort::Name,
            "code",
        );
        assert_eq!(by_provider.len(), 1);
        assert_eq!(by_provider[0].provider, "codex");
    }

    #[test]
    fn empty_import_preview_message_distinguishes_search_from_filter_empty_states() {
        assert_eq!(
            empty_import_preview_message(ImportPreviewFilter::All, "zig"),
            "No preview entries match search `zig`."
        );
        assert_eq!(
            empty_import_preview_message(ImportPreviewFilter::Update, ""),
            "No update preview entries are available."
        );
    }

    #[test]
    fn last_legacy_import_result_lines_include_summary_and_detail() {
        let lines = last_legacy_import_result_lines(Some(&LegacyImportResult {
            recorded_at: "2026-05-05 09:00:00".to_owned(),
            source_path: "/tmp/watcher.sh".to_owned(),
            outcome: "applied".to_owned(),
            processed: 3,
            added: 1,
            updated: 1,
            unchanged: 1,
            detail: "import applied".to_owned(),
        }));

        assert_eq!(lines.len(), 4);
        assert!(lines[0].contains("2026-05-05 09:00:00"));
        assert!(lines[1].contains("/tmp/watcher.sh"));
        assert!(lines[2].contains("applied"));
        assert!(lines[3].contains("import applied"));
    }

    #[test]
    fn last_legacy_import_result_lines_show_empty_state_without_result() {
        let lines = last_legacy_import_result_lines(None);
        assert_eq!(lines, vec!["No legacy import has run yet.".to_owned()]);
    }

    #[test]
    fn legacy_import_history_lines_show_recent_outcomes() {
        let lines = legacy_import_history_lines(
            &[
                LegacyImportResult {
                    recorded_at: "2026-05-05 09:05:00".to_owned(),
                    source_path: "/tmp/a.sh".to_owned(),
                    outcome: "applied".to_owned(),
                    processed: 2,
                    added: 1,
                    updated: 1,
                    unchanged: 0,
                    detail: "import applied".to_owned(),
                },
                LegacyImportResult {
                    recorded_at: "2026-05-05 09:00:00".to_owned(),
                    source_path: "/tmp/b.sh".to_owned(),
                    outcome: "previewed".to_owned(),
                    processed: 3,
                    added: 1,
                    updated: 1,
                    unchanged: 1,
                    detail: "import preview prepared".to_owned(),
                },
            ],
            LEGACY_IMPORT_HISTORY_COMPACT_LIMIT,
        );

        assert_eq!(lines[0], "Recent import outcomes:");
        assert!(lines[1].contains("applied"));
        assert!(lines[2].contains("previewed"));
    }

    #[test]
    fn legacy_import_history_lines_respect_requested_limit() {
        let history = vec![
            LegacyImportResult {
                recorded_at: "2026-05-05 09:10:00".to_owned(),
                source_path: "/tmp/a.sh".to_owned(),
                outcome: "applied".to_owned(),
                processed: 1,
                added: 1,
                updated: 0,
                unchanged: 0,
                detail: "a".to_owned(),
            },
            LegacyImportResult {
                recorded_at: "2026-05-05 09:09:00".to_owned(),
                source_path: "/tmp/b.sh".to_owned(),
                outcome: "previewed".to_owned(),
                processed: 1,
                added: 0,
                updated: 1,
                unchanged: 0,
                detail: "b".to_owned(),
            },
            LegacyImportResult {
                recorded_at: "2026-05-05 09:08:00".to_owned(),
                source_path: "/tmp/c.sh".to_owned(),
                outcome: "failed".to_owned(),
                processed: 0,
                added: 0,
                updated: 0,
                unchanged: 0,
                detail: "c".to_owned(),
            },
        ];

        let lines = legacy_import_history_lines(&history, 2);

        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("/tmp/a.sh"));
        assert!(lines[2].contains("/tmp/b.sh"));
    }

    #[test]
    fn find_target_index_by_identity_returns_matching_target() {
        let targets = vec![
            TargetConfig {
                name: "ziglua".to_owned(),
                provider: "minimax".to_owned(),
                ..TargetConfig::default()
            },
            TargetConfig {
                name: "chatbot-auto".to_owned(),
                provider: "codex".to_owned(),
                ..TargetConfig::default()
            },
        ];

        assert_eq!(
            find_target_index_by_identity(&targets, "codex", "chatbot-auto"),
            Some(1)
        );
        assert_eq!(
            find_target_index_by_identity(&targets, "codex", "missing"),
            None
        );
    }

    #[test]
    fn preview_entry_identity_text_formats_provider_and_name() {
        let entry = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Add,
        };

        assert_eq!(preview_entry_identity_text(&entry), "codex / chatbot-auto");
    }

    #[test]
    fn preview_entry_stable_id_uses_provider_and_name() {
        let entry = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Add,
        };

        assert_eq!(preview_entry_stable_id(&entry), "codex::chatbot-auto");
    }

    #[test]
    fn preview_entry_stable_id_label_formats_display_text() {
        let entry = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Add,
        };

        assert_eq!(
            preview_entry_stable_id_label(&entry),
            "id=codex::chatbot-auto"
        );
    }

    #[test]
    fn preview_entry_summary_text_includes_kind_identity_and_detail() {
        let entry = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Update,
        };

        assert_eq!(
            preview_entry_summary_text(&entry),
            "[Update] codex / chatbot-auto - managed fields changed"
        );
    }

    #[test]
    fn visible_preview_summary_text_joins_visible_rows() {
        let first = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Add,
        };
        let second = ImportPreviewEntry {
            name: "ziglua".to_owned(),
            provider: "minimax".to_owned(),
            kind: ImportPreviewKind::Update,
        };
        let summaries = visible_preview_summary_text(&[&first, &second]);

        assert_eq!(
            summaries,
            "[Add] codex / chatbot-auto - new target\n[Update] minimax / ziglua - managed fields changed"
        );
    }

    #[test]
    fn preview_review_snapshot_text_includes_source_counts_and_visible_rows() {
        let first = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Add,
        };
        let second = ImportPreviewEntry {
            name: "ziglua".to_owned(),
            provider: "minimax".to_owned(),
            kind: ImportPreviewKind::Update,
        };
        let text = preview_review_snapshot_text(
            "/tmp/watcher.sh",
            ImportSummary {
                processed: 3,
                added: 1,
                updated: 1,
                unchanged: 1,
            },
            ImportPreviewFilter::Update,
            ImportPreviewSort::ProviderThenName,
            "zig",
            &[&first, &second],
        );

        assert!(text.contains("Source: /tmp/watcher.sh"));
        assert!(text.contains("Filter: Update"));
        assert!(text.contains("Sort: Provider then Name"));
        assert!(text.contains("Search: zig"));
        assert!(text.contains("Processed 3 targets: 1 add, 1 update, 1 unchanged"));
        assert!(text.contains("[Add] codex / chatbot-auto - new target | id=codex::chatbot-auto"));
        assert!(
            text.contains(
                "[Update] minimax / ziglua - managed fields changed | id=minimax::ziglua"
            )
        );
    }

    #[test]
    fn preview_review_snapshot_markdown_includes_structured_sections() {
        let first = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Add,
        };
        let text = preview_review_snapshot_markdown(
            "/tmp/watcher.sh",
            ImportSummary {
                processed: 1,
                added: 1,
                updated: 0,
                unchanged: 0,
            },
            ImportPreviewFilter::All,
            ImportPreviewSort::Name,
            "",
            &[&first],
        );

        assert!(text.contains("# Legacy Import Review Snapshot"));
        assert!(text.contains("- Source: `/tmp/watcher.sh`"));
        assert!(text.contains("- Search: `(none)`"));
        assert!(text.contains("## Visible Entries"));
        assert!(text.contains("- [Add] codex / chatbot-auto - new target (`codex::chatbot-auto`)"));
    }

    #[test]
    fn preview_review_snapshot_json_includes_machine_readable_fields() {
        let first = ImportPreviewEntry {
            name: "chatbot-auto".to_owned(),
            provider: "codex".to_owned(),
            kind: ImportPreviewKind::Add,
        };
        let text = preview_review_snapshot_json(
            "/tmp/watcher.sh",
            ImportSummary {
                processed: 1,
                added: 1,
                updated: 0,
                unchanged: 0,
            },
            ImportPreviewFilter::All,
            ImportPreviewSort::Name,
            "",
            &[&first],
        );

        assert!(text.contains("\"source_path\": \"/tmp/watcher.sh\""));
        assert!(text.contains("\"filter\": \"All\""));
        assert!(text.contains("\"sort\": \"Name\""));
        assert!(text.contains("\"search\": \"(none)\""));
        assert!(text.contains("\"id\": \"codex::chatbot-auto\""));
        assert!(text.contains("\"kind\": \"Add\""));
        assert!(text.contains("\"detail\": \"new target\""));
        assert!(text.contains("\"provider\": \"codex\""));
        assert!(text.contains("\"identity\": \"codex / chatbot-auto\""));
        assert!(text.contains("\"summary\": \"[Add] codex / chatbot-auto - new target\""));
    }

    #[test]
    fn target_list_label_marks_preview_focused_targets() {
        assert_eq!(
            target_list_label("codex", "chatbot-auto", true),
            "codex / chatbot-auto [focused]"
        );
        assert_eq!(
            target_list_label("codex", "chatbot-auto", false),
            "codex / chatbot-auto"
        );
    }

    #[test]
    fn import_preview_kind_metadata_describes_review_state() {
        assert_eq!(ImportPreviewKind::Add.metadata(), ("Add", "new target"));
        assert_eq!(
            ImportPreviewKind::Update.metadata(),
            ("Update", "managed fields changed")
        );
        assert_eq!(
            ImportPreviewKind::Unchanged.metadata(),
            ("Unchanged", "already matches import")
        );
    }

    #[test]
    fn record_legacy_import_result_keeps_latest_first_and_caps_history() {
        let mut config = AppConfig::default();
        for index in 0..(LEGACY_IMPORT_HISTORY_LIMIT + 2) {
            record_legacy_import_result(
                &mut config,
                LegacyImportResult {
                    recorded_at: format!("2026-05-05 09:{index:02}:00"),
                    source_path: format!("/tmp/{index}.sh"),
                    outcome: "previewed".to_owned(),
                    processed: index,
                    added: 0,
                    updated: 0,
                    unchanged: 0,
                    detail: format!("result {index}"),
                },
            );
        }

        assert_eq!(
            config.legacy_import_history.len(),
            LEGACY_IMPORT_HISTORY_LIMIT
        );
        assert_eq!(
            config.legacy_import_history[0].source_path,
            format!("/tmp/{}.sh", LEGACY_IMPORT_HISTORY_LIMIT + 1)
        );
        assert_eq!(
            config
                .last_legacy_import_result
                .as_ref()
                .expect("last result should exist")
                .source_path,
            format!("/tmp/{}.sh", LEGACY_IMPORT_HISTORY_LIMIT + 1)
        );
    }

    #[test]
    fn clear_legacy_import_status_resets_latest_and_history() {
        let mut config = AppConfig {
            last_legacy_import_result: Some(LegacyImportResult {
                recorded_at: "2026-05-05 09:10:00".to_owned(),
                source_path: "/tmp/a.sh".to_owned(),
                outcome: "applied".to_owned(),
                processed: 1,
                added: 1,
                updated: 0,
                unchanged: 0,
                detail: "import applied".to_owned(),
            }),
            legacy_import_history: vec![LegacyImportResult {
                recorded_at: "2026-05-05 09:10:00".to_owned(),
                source_path: "/tmp/a.sh".to_owned(),
                outcome: "applied".to_owned(),
                processed: 1,
                added: 1,
                updated: 0,
                unchanged: 0,
                detail: "import applied".to_owned(),
            }],
            ..AppConfig::default()
        };

        clear_legacy_import_status(&mut config);

        assert!(config.last_legacy_import_result.is_none());
        assert!(config.legacy_import_history.is_empty());
    }

    #[test]
    fn dismiss_import_preview_resets_filter_to_all() {
        let paths = temp_app_paths();
        let mut app = ChatbotAutoApp::new(AppConfig::default(), Arc::new(paths));
        app.import_preview = Some(build_import_preview(
            &[],
            vec![TargetConfig {
                name: "chatbot-auto".to_owned(),
                provider: "codex".to_owned(),
                data_dir: "/tmp/codex".into(),
                continue_text: "continue".to_owned(),
                ..TargetConfig::default()
            }],
            "/tmp/watcher.sh".to_owned(),
        ));
        app.import_preview_filter = ImportPreviewFilter::Update;

        app.dismiss_import_preview();

        assert_eq!(app.import_preview_filter, ImportPreviewFilter::All);
        assert_eq!(app.import_preview_sort, ImportPreviewSort::KindThenName);
        assert!(app.import_preview_search.is_empty());
    }

    #[test]
    fn import_legacy_targets_skips_invalid_watcher_path() {
        let paths = temp_app_paths();
        let mut config = AppConfig::default();
        config.legacy_watcher_script_path = "/definitely/missing/watcher.sh".to_owned();
        let mut app = ChatbotAutoApp::new(config, Arc::new(paths));

        app.import_legacy_targets();

        assert!(app.import_preview.is_none());
        let last_log = app.logs.last().expect("import failure should be logged");
        assert!(last_log.contains("legacy watcher import skipped"));
        assert!(last_log.contains("does not exist"));
        let result = app
            .config
            .last_legacy_import_result
            .as_ref()
            .expect("last import result should be recorded");
        assert_eq!(result.outcome, "skipped");
        assert_eq!(result.processed, 0);
        assert!(result.detail.contains("does not exist"));
        assert_eq!(app.config.legacy_import_history.len(), 1);
    }

    #[test]
    fn apply_import_preview_records_last_import_result() {
        let paths = temp_app_paths();
        let mut app = ChatbotAutoApp::new(AppConfig::default(), Arc::new(paths));
        app.import_preview = Some(build_import_preview(
            &[],
            vec![TargetConfig {
                name: "chatbot-auto".to_owned(),
                provider: "codex".to_owned(),
                data_dir: "/tmp/codex".into(),
                continue_text: "continue".to_owned(),
                ..TargetConfig::default()
            }],
            "/tmp/watcher.sh".to_owned(),
        ));

        app.apply_import_preview();

        let result = app
            .config
            .last_legacy_import_result
            .as_ref()
            .expect("last import result should be recorded");
        assert_eq!(result.outcome, "applied");
        assert_eq!(result.source_path, "/tmp/watcher.sh");
        assert_eq!(result.processed, 1);
        assert_eq!(result.added, 1);
        assert_eq!(result.updated, 0);
        assert_eq!(result.unchanged, 0);
        assert_eq!(app.config.legacy_import_history.len(), 1);
    }
}
