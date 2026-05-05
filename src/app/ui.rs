use std::time::{Duration, Instant};
use eframe::egui;
use super::import_preview::{
    LEGACY_IMPORT_HISTORY_COMPACT_LIMIT, clear_legacy_import_status, empty_import_preview_message,
    find_target_index_by_identity, import_preview_kind_color, last_legacy_import_result_lines,
    legacy_import_history_lines, preview_entry_identity_text, preview_entry_stable_id_label,
    preview_entry_summary_text, preview_review_snapshot_json, preview_review_snapshot_markdown,
    preview_review_snapshot_text, searched_sorted_filtered_import_preview_entries,
    visible_preview_summary_text,
};
use super::*;
use crate::config::{self, PreviewExportFormat};
use crate::engine::EngineAction;
impl ChatbotAutoApp {
    fn save_config_manually(&mut self) {
        match config::save(self.paths.as_ref(), &self.config) {
            Ok(()) => {
                self.last_saved_config_snapshot = config_snapshot(&self.config);
                self.logs.push(format!(
                    "{} saved config to {}",
                    timestamp_now(),
                    self.paths.config_path.display()
                ));
            }
            Err(error) => self.logs.push(format!(
                "{} failed to save config: {error}",
                timestamp_now()
            )),
        }
    }}
impl ChatbotAutoApp {
    fn reload_config(&mut self) {
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
    }}
impl ChatbotAutoApp {
    fn render_top_panel(&mut self, ctx: &egui::Context) {
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
                    self.save_config_manually();
                }
                if ui.button("Reload Config").clicked() {
                    self.reload_config();
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

            self.render_legacy_import_panel(ui);

            egui::CollapsingHeader::new("Diagnostics")
                .default_open(false)
                .show(ui, |ui| {
                    ui.monospace(format!("Config: {}", self.paths.config_path.display()));
                    ui.monospace(format!("Log file: {}", self.paths.log_path.display()));
                    ui.monospace(format!("Config dir: {}", self.paths.config_dir.display()));
                    ui.monospace(format!("State dir: {}", self.paths.log_dir.display()));
                });
        });
    }}
impl ChatbotAutoApp {
    fn render_legacy_import_panel(&mut self, ui: &mut egui::Ui) {
        let default_open = self.import_preview.is_some();
        egui::CollapsingHeader::new("Legacy watcher import")
            .default_open(default_open)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Watcher script");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.config.legacy_watcher_script_path)
                            .desired_width(420.0),
                    );
                    if ui.button("Preview Import").clicked() {
                        self.import_legacy_targets();
                    }
                    if ui
                        .add_enabled(
                            self.import_preview.is_some(),
                            egui::Button::new("Apply Preview"),
                        )
                        .clicked()
                    {
                        self.apply_import_preview();
                    }
                    if ui
                        .add_enabled(
                            self.import_preview.is_some(),
                            egui::Button::new("Dismiss Preview"),
                        )
                        .clicked()
                    {
                        self.dismiss_import_preview();
                    }
                    if ui.button("Clear Status").clicked() {
                        clear_legacy_import_status(&mut self.config);
                        self.logs
                            .push(format!("{} cleared legacy import status", timestamp_now()));
                    }
                });

                for line in
                    last_legacy_import_result_lines(self.config.last_legacy_import_result.as_ref())
                {
                    ui.monospace(line);
                }

                let history_limit = if self.import_history_expanded {
                    self.config.legacy_import_history.len()
                } else {
                    LEGACY_IMPORT_HISTORY_COMPACT_LIMIT
                };
                let history_lines =
                    legacy_import_history_lines(&self.config.legacy_import_history, history_limit);
                if !history_lines.is_empty() {
                    ui.horizontal(|ui| {
                        let button_text = if self.import_history_expanded {
                            "Collapse History"
                        } else {
                            "Expand History"
                        };
                        if ui.button(button_text).clicked() {
                            self.import_history_expanded = !self.import_history_expanded;
                        }
                    });
                    for line in history_lines {
                        ui.monospace(line);
                    }
                }

                let Some(preview) = self.import_preview.as_ref() else {
                    return;
                };

                ui.separator();
                ui.label(format!(
                    "Prepared preview from {} (processed {}, {} add, {} update, {} unchanged)",
                    preview.source_path,
                    preview.summary.processed,
                    preview.summary.added,
                    preview.summary.updated,
                    preview.summary.unchanged
                ));

                ui.horizontal_wrapped(|ui| {
                    ui.label("Filter");
                    for filter in [
                        ImportPreviewFilter::All,
                        ImportPreviewFilter::Add,
                        ImportPreviewFilter::Update,
                        ImportPreviewFilter::Unchanged,
                    ] {
                        ui.selectable_value(
                            &mut self.import_preview_filter,
                            filter,
                            filter.label(),
                        );
                    }
                });

                ui.horizontal_wrapped(|ui| {
                    ui.label("Sort");
                    egui::ComboBox::from_id_salt("import_preview_sort")
                        .selected_text(self.import_preview_sort.label())
                        .show_ui(ui, |ui| {
                            for sort in [
                                ImportPreviewSort::KindThenName,
                                ImportPreviewSort::ProviderThenName,
                                ImportPreviewSort::Name,
                            ] {
                                ui.selectable_value(
                                    &mut self.import_preview_sort,
                                    sort,
                                    sort.label(),
                                );
                            }
                        });

                    ui.label("Snapshot");
                    egui::ComboBox::from_id_salt("preview_export_format")
                        .selected_text(self.config.preferred_preview_export_format.label())
                        .show_ui(ui, |ui| {
                            for format in [
                                PreviewExportFormat::Text,
                                PreviewExportFormat::Markdown,
                                PreviewExportFormat::Json,
                            ] {
                                ui.selectable_value(
                                    &mut self.config.preferred_preview_export_format,
                                    format,
                                    format.label(),
                                );
                            }
                        });

                    ui.checkbox(&mut self.config.preview_show_stable_ids, "Show stable IDs");
                });

                ui.horizontal_wrapped(|ui| {
                    ui.label("Search");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.import_preview_search)
                            .desired_width(220.0),
                    );
                });

                let visible_entries = searched_sorted_filtered_import_preview_entries(
                    &preview.entries,
                    self.import_preview_filter,
                    self.import_preview_sort,
                    &self.import_preview_search,
                );

                ui.horizontal_wrapped(|ui| {
                    if ui.button("Copy Visible Summaries").clicked() {
                        ui.ctx()
                            .copy_text(visible_preview_summary_text(&visible_entries));
                    }
                    if ui.button("Copy Review Snapshot").clicked() {
                        let snapshot = match self.config.preferred_preview_export_format {
                            PreviewExportFormat::Text => preview_review_snapshot_text(
                                &preview.source_path,
                                preview.summary,
                                self.import_preview_filter,
                                self.import_preview_sort,
                                &self.import_preview_search,
                                &visible_entries,
                            ),
                            PreviewExportFormat::Markdown => preview_review_snapshot_markdown(
                                &preview.source_path,
                                preview.summary,
                                self.import_preview_filter,
                                self.import_preview_sort,
                                &self.import_preview_search,
                                &visible_entries,
                            ),
                            PreviewExportFormat::Json => preview_review_snapshot_json(
                                &preview.source_path,
                                preview.summary,
                                self.import_preview_filter,
                                self.import_preview_sort,
                                &self.import_preview_search,
                                &visible_entries,
                            ),
                        };
                        ui.ctx().copy_text(snapshot);
                    }
                });

                if visible_entries.is_empty() {
                    ui.label(empty_import_preview_message(
                        self.import_preview_filter,
                        &self.import_preview_search,
                    ));
                    return;
                }

                egui::ScrollArea::vertical()
                    .id_salt("import_preview_scroll")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for entry in visible_entries {
                            let (badge, detail) = entry.kind.metadata();
                            let target_index = find_target_index_by_identity(
                                &self.config.targets,
                                &entry.provider,
                                &entry.name,
                            );
                            ui.horizontal_wrapped(|ui| {
                                ui.colored_label(import_preview_kind_color(entry.kind), badge);
                                ui.label(preview_entry_identity_text(entry));
                                ui.small(detail);
                                if self.config.preview_show_stable_ids {
                                    ui.monospace(preview_entry_stable_id_label(entry));
                                }
                                if ui.button("Copy").clicked() {
                                    ui.ctx().copy_text(preview_entry_summary_text(entry));
                                }
                                if let Some(index) = target_index {
                                    if ui.button("Focus").clicked() {
                                        self.selected_target = Some(index);
                                        self.preview_focused_target = Some(index);
                                    }
                                }
                            });
                        }
                    });
            });
    }}
impl ChatbotAutoApp {
    fn render_target_details_panel(&mut self, ctx: &egui::Context) {
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
                            if target.awaiting_verification {
                                ui.label("Verification state: awaiting");
                            }
                            if let Some(verification_session_id) =
                                target.verification_session_id.as_deref()
                            {
                                ui.label(format!(
                                    "Verification session: {}",
                                    verification_session_id
                                ));
                            }

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
    }}
