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
