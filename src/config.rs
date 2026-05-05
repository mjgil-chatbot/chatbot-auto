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
pub fn load_or_default(paths: &AppPaths) -> Result<AppConfig> {
    if !paths.config_path.exists() {
        let config = AppConfig::default();
        save(paths, &config)?;
        return Ok(config);
    }

    let contents = fs::read_to_string(&paths.config_path)
        .with_context(|| format!("failed to read {}", paths.config_path.display()))?;
    let mut config = serde_json::from_str::<AppConfig>(&contents)
        .with_context(|| format!("failed to parse {}", paths.config_path.display()))?;
    normalize_config(&mut config);
    Ok(config)
}
pub fn save(paths: &AppPaths, config: &AppConfig) -> Result<()> {
    let mut normalized = config.clone();
    normalize_config(&mut normalized);
    let json = serde_json::to_string_pretty(&normalized).context("failed to serialize config")?;
    let tmp_path = paths.config_path.with_extension("json.tmp");
    fs::write(&tmp_path, json)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, &paths.config_path)
        .with_context(|| format!("failed to move {}", paths.config_path.display()))?;
    Ok(())
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
pub fn append_log_line(log_path: &Path, line: &str) -> Result<()> {
    use std::io::Write;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .with_context(|| format!("failed to open {}", log_path.display()))?;
    writeln!(file, "{line}").with_context(|| format!("failed to append {}", log_path.display()))
}
pub fn build_continue_text(default_continue_text: &str, target: &TargetConfig) -> String {
    let base_text = if target.continue_text.trim().is_empty() {
        default_continue_text.trim()
    } else {
        target.continue_text.trim()
    };
    let mut text = base_text.to_owned();

    if !target.design_document.trim().is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str("Design document: ");
        text.push_str(target.design_document.trim());
    }

    if !target.task_document.trim().is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str("Task document: ");
        text.push_str(target.task_document.trim());
    }

    append_completion_contract(&mut text);
    text
}
pub fn apply_default_doc_paths(target: &mut TargetConfig) {
    if target.repo_path.as_os_str().is_empty() {
        return;
    }

    let iteration = next_available_iteration(&target.repo_path);
    target.current_iteration = iteration;
    target.max_iterations = target.max_iterations.max(iteration);
    let (design_document, task_document) = iteration_doc_paths(&target.repo_path, iteration);
    target.design_document = design_document.display().to_string();
    target.task_document = task_document.display().to_string();
}
pub fn advance_iteration(target: &mut TargetConfig) {
    if target.repo_path.as_os_str().is_empty() {
        return;
    }

    let next_iteration =
        next_available_iteration_from(&target.repo_path, target.current_iteration + 1);
    target.current_iteration = next_iteration;
    target.max_iterations = target.max_iterations.max(next_iteration);
    let (design_document, task_document) = iteration_doc_paths(&target.repo_path, next_iteration);
    target.design_document = design_document.display().to_string();
    target.task_document = task_document.display().to_string();
}
pub fn build_docs_prompt(default_prompt: &str, target: &TargetConfig) -> String {
    let mut prompt = format!(
        "{default_prompt}\n\nRepository: {}\nDesign document path: {}\nTask list path: {}\n\nCreate both files under the repo docs folder in this run. Use sequential thinking. After finishing, reply with explicit next steps so the continue loop can resume.",
        target.repo_path.display(),
        target.design_document,
        target.task_document
    );
    append_completion_contract(&mut prompt);
    prompt
}
fn append_completion_contract(text: &mut String) {
    const CONTRACT: &str = "If more work remains, you must end with an explicit \"Next Steps:\" section. If all work for this iteration is complete, reply with exactly \"Design Doc Fully Implemented\".";

    if text.contains("Design Doc Fully Implemented") && text.contains("Next Steps:") {
        return;
    }

    if !text.is_empty() {
        text.push_str("\n\n");
    }
    text.push_str(CONTRACT);
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
pub fn validate_bootstrap_target(target: &TargetConfig) -> Vec<String> {
    if !target.build_docs_before_continue {
        return Vec::new();
    }

    let mut issues = Vec::new();
    if target.repo_path.as_os_str().is_empty() {
        issues.push("repo path is empty".to_owned());
    }
    if target.design_document.trim().is_empty() {
        issues.push("design document path is empty".to_owned());
    }
    if target.task_document.trim().is_empty() {
        issues.push("task document path is empty".to_owned());
    }
    if !target.design_document.trim().is_empty()
        && target.design_document.trim() == target.task_document.trim()
    {
        issues.push("design and task document paths must differ".to_owned());
    }

    if !target.repo_path.as_os_str().is_empty() {
        let docs_dir = target.repo_path.join("docs");
        if !target.design_document.trim().is_empty()
            && !Path::new(target.design_document.trim()).starts_with(&docs_dir)
        {
            issues.push("design document is outside the repo docs directory".to_owned());
        }
        if !target.task_document.trim().is_empty()
            && !Path::new(target.task_document.trim()).starts_with(&docs_dir)
        {
            issues.push("task document is outside the repo docs directory".to_owned());
        }
    }

    if target.build_docs_completed {
        if !target.design_document.trim().is_empty()
            && !Path::new(target.design_document.trim()).is_file()
        {
            issues.push("design document does not exist".to_owned());
        }
        if !target.task_document.trim().is_empty()
            && !Path::new(target.task_document.trim()).is_file()
        {
            issues.push("task document does not exist".to_owned());
        }
    }

    issues
}
pub fn validate_legacy_watcher_script_path(path: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let trimmed = path.trim();
    if trimmed.is_empty() {
        issues.push("legacy watcher import path is empty".to_owned());
        return issues;
    }

    let path = Path::new(trimmed);
    if !path.exists() {
        issues.push("legacy watcher import path does not exist".to_owned());
    } else if !path.is_file() {
        issues.push("legacy watcher import path is not a file".to_owned());
    }

    issues
}
fn next_available_iteration(repo_path: &Path) -> u32 {
    next_available_iteration_from(repo_path, 1)
}
fn next_available_iteration_from(repo_path: &Path, start: u32) -> u32 {
    let mut iteration = start.max(1);
    loop {
        let (design, task) = iteration_doc_paths(repo_path, iteration);
        if !design.exists() && !task.exists() {
            return iteration;
        }
        iteration += 1;
    }
}
fn iteration_doc_paths(repo_path: &Path, iteration: u32) -> (PathBuf, PathBuf) {
    let docs_dir = repo_path.join("docs");
    if iteration <= 1 {
        return (docs_dir.join("design.md"), docs_dir.join("task.md"));
    }

    (
        docs_dir.join(format!("design-{iteration}.md")),
        docs_dir.join(format!("task-{iteration}.md")),
    )
}
pub fn provider_data_dir(provider: &str) -> Option<PathBuf> {
    let provider = provider.trim();
    if provider.is_empty() {
        return None;
    }

    let root = rust_chatbot_shared_root()?;
    Some(root.join(format!(".{provider}-desktop")))
}
pub fn discover_provider_data_dirs() -> Vec<(String, PathBuf)> {
    let Some(root) = rust_chatbot_shared_root() else {
        return Vec::new();
    };
    discover_provider_data_dirs_under(&root)
}
fn discover_provider_data_dirs_under(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    let mut options = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        if !name.ends_with("-desktop") {
            continue;
        }

        let provider = name
            .trim_start_matches('.')
            .trim_end_matches("-desktop")
            .to_owned();
        options.push((provider, path));
    }

    options.sort_by(|left, right| left.0.cmp(&right.0));
    options.dedup_by(|left, right| left.0 == right.0);
    options
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
