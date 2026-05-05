use std::path::Path;
use std::process::Command;
use anyhow::{Context, Result, bail};
use crate::config::TargetConfig;
pub fn import_targets_from_watcher_script(script_path: &Path) -> Result<Vec<TargetConfig>> {
    let command = r#"
set -euo pipefail
script_path="$1"
source <(sed '/^main "\$@"$/d' "$script_path")
for spec in "${TARGET_SPECS[@]}"; do
  IFS='|' read -r name provider data_dir <<<"$spec"
  text="${TARGET_TEXT[$name]:-continue}"
  printf '%s\0%s\0%s\0%s\0' "$name" "$provider" "$data_dir" "$text"
done
"#;

    let output = Command::new("bash")
        .arg("-lc")
        .arg(command)
        .arg("bash")
        .arg(script_path)
        .output()
        .with_context(|| format!("failed to execute legacy watcher {}", script_path.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "legacy watcher import failed for {}: {}",
            script_path.display(),
            stderr.trim()
        );
    }

    parse_import_stream(&output.stdout)
}
fn parse_import_stream(stdout: &[u8]) -> Result<Vec<TargetConfig>> {
    let fields: Vec<String> = stdout
        .split(|byte| *byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| String::from_utf8(chunk.to_vec()))
        .collect::<std::result::Result<_, _>>()
        .context("legacy watcher import emitted non-utf8 data")?;

    if fields.is_empty() {
        return Ok(Vec::new());
    }
    if fields.len() % 4 != 0 {
        bail!("legacy watcher import emitted an incomplete target record");
    }

    let mut targets = Vec::new();
    for record in fields.chunks_exact(4) {
        targets.push(TargetConfig {
            name: record[0].clone(),
            provider: record[1].clone(),
            data_dir: record[2].clone().into(),
            continue_text: record[3].clone(),
            ..TargetConfig::default()
        });
    }
    Ok(targets)
}
