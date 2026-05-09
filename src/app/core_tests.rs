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
        legacy_watcher_script_path: "~/git/bash-snippets/codex-rust-chatbot-continue.sh".to_owned(),
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
        legacy_watcher_script_path: "~/git/bash-snippets/codex-rust-chatbot-continue.sh".to_owned(),
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
