use std::sync::Arc;
use super::*;
use crate::config::AppPaths;
fn temp_app_paths() -> AppPaths {
    let root = std::env::temp_dir().join(format!(
        "chatbot-auto-import-preview-tests-{}-{}",
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
    let lines = last_legacy_import_result_lines(Some(&crate::config::LegacyImportResult {
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
            crate::config::LegacyImportResult {
                recorded_at: "2026-05-05 09:05:00".to_owned(),
                source_path: "/tmp/a.sh".to_owned(),
                outcome: "applied".to_owned(),
                processed: 2,
                added: 1,
                updated: 1,
                unchanged: 0,
                detail: "import applied".to_owned(),
            },
            crate::config::LegacyImportResult {
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
        crate::config::LegacyImportResult {
            recorded_at: "2026-05-05 09:10:00".to_owned(),
            source_path: "/tmp/a.sh".to_owned(),
            outcome: "applied".to_owned(),
            processed: 2,
            added: 1,
            updated: 1,
            unchanged: 0,
            detail: "import applied".to_owned(),
        },
        crate::config::LegacyImportResult {
            recorded_at: "2026-05-05 09:05:00".to_owned(),
            source_path: "/tmp/b.sh".to_owned(),
            outcome: "previewed".to_owned(),
            processed: 3,
            added: 1,
            updated: 1,
            unchanged: 1,
            detail: "import preview prepared".to_owned(),
        },
        crate::config::LegacyImportResult {
            recorded_at: "2026-05-05 09:00:00".to_owned(),
            source_path: "/tmp/c.sh".to_owned(),
            outcome: "dismissed".to_owned(),
            processed: 3,
            added: 1,
            updated: 1,
            unchanged: 1,
            detail: "import preview dismissed".to_owned(),
        },
    ];

    let lines = legacy_import_history_lines(&history, 2);

    assert_eq!(lines.len(), 3);
    assert!(lines[1].contains("/tmp/a.sh"));
    assert!(lines[2].contains("/tmp/b.sh"));
}
#[test]
fn preview_entry_identity_and_stable_id_helpers_format_expected_text() {
    let entry = ImportPreviewEntry {
        name: "chatbot-auto".to_owned(),
        provider: "codex".to_owned(),
        kind: ImportPreviewKind::Add,
    };

    assert_eq!(preview_entry_identity_text(&entry), "codex / chatbot-auto");
    assert_eq!(
        preview_entry_stable_id_label(&entry),
        "id=codex::chatbot-auto"
    );
    assert_eq!(
        preview_entry_summary_text(&entry),
        "[Add] codex / chatbot-auto - new target"
    );
}
