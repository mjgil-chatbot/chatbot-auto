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
