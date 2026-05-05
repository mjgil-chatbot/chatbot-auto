use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use directories::{BaseDirs, ProjectDirs};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub log_dir: PathBuf,
    pub config_path: PathBuf,
    pub log_path: PathBuf,
}
impl AppPaths {
    pub fn discover() -> Result<Self> {
        let dirs = ProjectDirs::from("rs", "m", "chatbot-auto")
            .context("failed to resolve project directories")?;
        let config_dir = dirs.config_dir().to_path_buf();
        let data_dir = dirs.data_dir().to_path_buf();
        let log_dir = dirs.state_dir().unwrap_or(dirs.data_dir()).to_path_buf();
        let config_path = config_dir.join("config.json");
        let log_path = log_dir.join("app.log");

        fs::create_dir_all(&config_dir)
            .with_context(|| format!("failed to create config dir {}", config_dir.display()))?;
        fs::create_dir_all(&data_dir)
            .with_context(|| format!("failed to create data dir {}", data_dir.display()))?;
        fs::create_dir_all(&log_dir)
            .with_context(|| format!("failed to create log dir {}", log_dir.display()))?;

        Ok(Self {
            config_dir,
            log_dir,
            config_path,
            log_path,
        })
    }}
const DEFAULT_CHATBOT_CTL_BINARY: &str = "chatbot-ctl";
const DEFAULT_WATCHER_SCRIPT_NAME: &str = "codex-rust-chatbot-continue.sh";
const DEFAULT_RUST_CHATBOT_SHARED_ROOT_DIR: &str = ".rust-chatbot";
const RUST_CHATBOT_CTL_RELATIVE_CANDIDATES: &[&str] = &[
    "../rust-chatbot/target/release/chatbot-ctl",
    "../rust-chatbot/target/debug/chatbot-ctl",
    "rust-chatbot/target/release/chatbot-ctl",
    "rust-chatbot/target/debug/chatbot-ctl",
    "git/rust-chatbot/target/release/chatbot-ctl",
    "git/rust-chatbot/target/debug/chatbot-ctl",
];
const LEGACY_WATCHER_RELATIVE_CANDIDATES: &[&str] = &[
    "../bash-snippets/codex-rust-chatbot-continue.sh",
    "bash-snippets/codex-rust-chatbot-continue.sh",
    "../git/bash-snippets/codex-rust-chatbot-continue.sh",
    "git/bash-snippets/codex-rust-chatbot-continue.sh",
];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AppConfig {
    pub chatbot_ctl_path: PathBuf,
    pub poll_interval_seconds: u64,
    pub automation_enabled: bool,
    pub default_continue_text: String,
    pub default_build_docs_prompt: String,
    pub legacy_watcher_script_path: String,
    pub preview_show_stable_ids: bool,
    pub preferred_preview_export_format: PreviewExportFormat,
    pub last_legacy_import_result: Option<LegacyImportResult>,
    pub legacy_import_history: Vec<LegacyImportResult>,
    pub targets: Vec<TargetConfig>,
}
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            chatbot_ctl_path: default_chatbot_ctl_path(),
            poll_interval_seconds: 30,
            automation_enabled: true,
            default_continue_text: "continue".to_owned(),
            default_build_docs_prompt: "Use sequential thinking. Analyze the repository, write the design document and task list in one shot, and leave the project ready for the normal continue loop.".to_owned(),
            legacy_watcher_script_path: default_legacy_watcher_script_path(),
            preview_show_stable_ids: false,
            preferred_preview_export_format: PreviewExportFormat::Text,
            last_legacy_import_result: None,
            legacy_import_history: Vec::new(),
            targets: Vec::new(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PreviewExportFormat {
    #[default]
    Text,
    Markdown,
    Json,
}
impl PreviewExportFormat {
    #[allow(dead_code)]
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Markdown => "Markdown",
            Self::Json => "JSON",
        }
    }}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyImportResult {
    pub recorded_at: String,
    pub source_path: String,
    pub outcome: String,
    pub processed: usize,
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TargetConfig {
    pub name: String,
    pub provider: String,
    pub data_dir: PathBuf,
    pub continue_text: String,
    pub repo_path: PathBuf,
    pub design_document: String,
    pub task_document: String,
    pub build_docs_before_continue: bool,
    pub build_docs_completed: bool,
    pub auto_increment_iterations: bool,
    pub current_iteration: u32,
    pub max_iterations: u32,
    pub enabled: bool,
    pub cooldown_seconds: u64,
    pub verification_session_id: Option<String>,
    pub awaiting_verification: bool,
    pub verification_started_at: Option<DateTime<Utc>>,
    pub notes: String,
}
impl Default for TargetConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            provider: "codex".to_owned(),
            data_dir: PathBuf::new(),
            continue_text: String::new(),
            repo_path: PathBuf::new(),
            design_document: String::new(),
            task_document: String::new(),
            build_docs_before_continue: false,
            build_docs_completed: false,
            auto_increment_iterations: false,
            current_iteration: 1,
            max_iterations: u32::MAX,
            enabled: true,
            cooldown_seconds: 120,
            verification_session_id: None,
            awaiting_verification: false,
            verification_started_at: None,
            notes: String::new(),
        }
    }
}
fn default_chatbot_ctl_path() -> PathBuf {
    if let Ok(path) = env::var("RUST_CHATBOT_CTL") {
        return PathBuf::from(path);
    }

    if let Some(path) = find_in_path("chatbot-ctl") {
        return path;
    }

    find_existing_relative_path(RUST_CHATBOT_CTL_RELATIVE_CANDIDATES)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CHATBOT_CTL_BINARY))
}
fn default_legacy_watcher_script_path() -> String {
    if let Ok(path) = env::var("RUST_CHATBOT_LEGACY_WATCHER_SCRIPT") {
        return path;
    }

    if let Some(path) = find_in_path(DEFAULT_WATCHER_SCRIPT_NAME) {
        return path.display().to_string();
    }

    find_existing_relative_path(LEGACY_WATCHER_RELATIVE_CANDIDATES)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_WATCHER_SCRIPT_NAME))
        .display()
        .to_string()
}
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for directory in env::split_paths(&path_var) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
pub fn home_dir() -> Option<PathBuf> {
    BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .or_else(|| env::var_os("HOME").map(PathBuf::from))
}
pub fn rust_chatbot_shared_root() -> Option<PathBuf> {
    env::var_os("RUST_CHATBOT_SHARED_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("RUST_CHATBOT_CONFIG_HOME")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .or_else(|| home_dir().map(|home| home.join(DEFAULT_RUST_CHATBOT_SHARED_ROOT_DIR)))
}
fn find_existing_relative_path(relative_candidates: &[&str]) -> Option<PathBuf> {
    for search_root in search_roots() {
        for relative in relative_candidates {
            let candidate = search_root.join(relative);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}
fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(cwd) = env::current_dir() {
        roots.push(cwd);
    }

    if let Ok(exe_path) = env::current_exe()
        && let Some(parent) = exe_path.parent()
    {
        roots.push(parent.to_path_buf());
    }

    if let Some(home) = home_dir() {
        roots.push(home);
    }

    roots
}
pub fn normalize_config(config: &mut AppConfig) {
    if !config.legacy_watcher_script_path.trim().is_empty() {
        config.legacy_watcher_script_path = expand_home_str(&config.legacy_watcher_script_path);
    }
    for target in &mut config.targets {
        normalize_target(target);
    }
}
fn normalize_target(target: &mut TargetConfig) {
    if target.data_dir.as_os_str().is_empty()
        && let Some(data_dir) = provider_data_dir(&target.provider)
    {
        target.data_dir = data_dir;
    }

    target.repo_path = expand_home_path(&target.repo_path);
    if !target.design_document.trim().is_empty() {
        target.design_document = expand_home_str(&target.design_document);
    }
    if !target.task_document.trim().is_empty() {
        target.task_document = expand_home_str(&target.task_document);
    }
    if target.current_iteration == 0 {
        target.current_iteration = 1;
    }
    if target.max_iterations == 0 {
        target.max_iterations = 1;
    }
    if target.max_iterations < target.current_iteration {
        target.max_iterations = target.current_iteration;
    }
}
pub fn provider_data_dir(provider: &str) -> Option<PathBuf> {
    let provider = provider.trim();
    if provider.is_empty() {
        return None;
    }

    let root = rust_chatbot_shared_root()?;
    Some(root.join(format!(".{provider}-desktop")))
}
fn expand_home_path(path: &Path) -> PathBuf {
    PathBuf::from(expand_home_str(&path.display().to_string()))
}
fn expand_home_str(path: &str) -> String {
    if let Some(stripped) = path.strip_prefix("~/")
        && let Some(home) = home_dir()
    {
        return home.join(stripped).display().to_string();
    }
    path.to_owned()
}
