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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_temp_dir() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("chatbot-auto-import-tests-{nanos}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn parse_import_stream_builds_targets_from_nul_delimited_fields() {
        let stream = b"ziglua\0minimax\0/tmp/minimax\0continue\0";
        let targets = parse_import_stream(stream).expect("import stream should parse");
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "ziglua");
        assert_eq!(targets[0].provider, "minimax");
        assert_eq!(targets[0].data_dir, PathBuf::from("/tmp/minimax"));
        assert_eq!(targets[0].continue_text, "continue");
    }

    #[test]
    fn import_targets_from_watcher_script_extracts_specs_and_custom_text() {
        let dir = unique_temp_dir();
        let script_path = dir.join("watcher.sh");
        fs::write(
            &script_path,
            r#"#!/usr/bin/env bash
set -euo pipefail
declare -A TARGET_TEXT
TARGET_TEXT[ziglua]=$'continue --- commit\nand push'
TARGET_TEXT[default]="continue"
TARGET_SPECS=(
  "ziglua|minimax|/tmp/minimax"
  "chatbot-auto|codex|/tmp/codex"
)
main() { :; }
main "$@"
"#,
        )
        .expect("fixture should be written");

        let targets =
            import_targets_from_watcher_script(&script_path).expect("watcher import should work");
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].name, "ziglua");
        assert_eq!(targets[0].continue_text, "continue --- commit\nand push");
        assert_eq!(targets[1].name, "chatbot-auto");
        assert_eq!(targets[1].continue_text, "continue");
    }
}
