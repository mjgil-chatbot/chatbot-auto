use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, bail};
pub fn health_check(path: &Path) -> Result<()> {
    if !path.is_file() {
        bail!("{} does not exist", path.display());
    }
    Ok(())
}
pub fn worker_running_for_chat(path: &Path, chat_id: &str) -> Result<bool> {
    let output = Command::new(path)
        .arg("workers")
        .output()
        .with_context(|| format!("failed to execute {} workers", path.display()))?;
    if !output.status.success() {
        bail!(
            "{} workers failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .any(|line| line.contains(chat_id) && line.contains("RUNNING")))
}
pub fn send_continue(path: &Path, provider: &str, chat_id: &str, text: &str) -> Result<()> {
    let output = Command::new(path)
        .arg("send")
        .arg("--provider")
        .arg(provider)
        .arg("--chat-id")
        .arg(chat_id)
        .arg("--text")
        .arg(text)
        .output()
        .with_context(|| format!("failed to execute {} send", path.display()))?;

    if !output.status.success() {
        bail!(
            "{} send failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}
pub fn create_chat(path: &Path, provider: &str, name: &str) -> Result<String> {
    let output = Command::new(path)
        .arg("chat")
        .arg("create")
        .arg("--provider")
        .arg(provider)
        .arg("--name")
        .arg(name)
        .output()
        .with_context(|| format!("failed to execute {} chat create", path.display()))?;

    if !output.status.success() {
        bail!(
            "{} chat create failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let chat_id = stdout.trim().to_owned();
    if chat_id.is_empty() {
        bail!("{} chat create returned empty chat id", path.display());
    }
    Ok(chat_id)
}
pub fn rename_chat(path: &Path, chat_id: &str, name: &str) -> Result<()> {
    let output = Command::new(path)
        .arg("chat")
        .arg("rename")
        .arg("--chat-id")
        .arg(chat_id)
        .arg("--name")
        .arg(name)
        .output()
        .with_context(|| format!("failed to execute {} chat rename", path.display()))?;

    if !output.status.success() {
        bail!(
            "{} chat rename failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}
