# `chatbot-auto` `egui` App Design

This document preserves the original product design and migration rationale.
The current implementation and operating instructions are described in
[`design.md`](design.md) and the [README](../README.md). Proposed scheduler
features below are design requirements, not release verification claims.

## Goal

Replace these shell scripts with a Rust desktop app built with `egui`:

- `codex-rust-chatbot-continue.sh`
- `toggle-codex-rust-chatbot-continue-cron.sh`

The app should automatically monitor configured chatbot sessions and send a follow-up `continue` prompt when a chat is idle and the latest assistant message clearly indicates more work remains.

## Current Script Behavior

The existing automation does four things:

1. Resolves the `chatbot-ctl` executable.
2. Maintains a configured list of watched targets in `name|provider|data_dir` form.
3. Periodically inspects session metadata and session transcript JSON files.
4. Sends text into a chat only when:
   - the target session exists,
   - no worker is currently running for that chat,
   - the latest message is from the assistant,
   - and the assistant message looks like it contains next-step guidance.

The cron helper script only enables, disables, or reports status for a managed crontab block that runs the watcher every minute.

## Product Direction

The Rust app should absorb both scripts into a single desktop application with:

- a persistent config file instead of hard-coded Bash arrays,
- a background polling loop instead of cron as the default mode,
- a visible status dashboard for watched chats,
- manual controls for pause/resume/check-now/send-now,
- logs and diagnostics in the UI,
- optional export/install of a system scheduler entry only if needed.

The app should make the automation inspectable and editable without reopening shell scripts.

## Scope

### In Scope

- Native desktop app using `eframe`/`egui`
- Load/save watched targets from config
- Run periodic checks on a timer
- Parse `sessions.json` and per-session transcript JSON files
- Invoke `chatbot-ctl workers` and `chatbot-ctl send`
- Show per-target state and recent actions
- Persist app settings and logs
- Support manual one-shot checks
- Support enable/disable automation from the UI

### Out of Scope for V1

- Editing chat history directly
- Replacing `chatbot-ctl`
- Multi-user sync
- Remote control or web UI
- AI-driven prompt generation beyond configured text templates

## Functional Requirements

### Target Configuration

Each watched target should contain:

- `name`: logical session name
- `provider`: `codex`, `claude`, `minimax`, or future provider string
- `data_dir`: provider data directory
- `continue_text`: text to send when continuation is triggered
- `enabled`: whether this target participates in polling
- `cooldown_seconds`: minimum delay between sends for this target
- `notes`: optional operator notes

### Runtime Checks

For each enabled target, the app should:

1. Resolve the visible session ID for `name` from `sessions.json`.
2. Check whether a worker is already running for that chat via `chatbot-ctl workers`.
3. Load the latest transcript JSON from `sessions/<chat_id>.json`.
4. Confirm the last message is from the assistant.
5. Evaluate the message content with the same next-step heuristic used by the script.
6. Send the configured continuation text only when all checks pass.
7. Record the result in the event log and target status.

### Controls

The UI should provide:

- global automation on/off
- configurable polling interval
- `Check Now` for all targets
- `Check` for a single target
- `Send Continue Now` for a single target
- add/edit/remove target
- view resolved session ID and last decision
- open log file / config file paths

## Non-Functional Requirements

- Safe by default: no send unless the same gating logic passes
- Observable: every decision is logged with a reason
- Responsive UI: all file IO and process execution happen off the UI thread
- Durable config: invalid config should not destroy the prior file
- Portable enough for Linux desktop first

## Proposed Architecture

## Crates

- `eframe` / `egui` for desktop UI
- `serde`, `serde_json` for config and transcript parsing
- `tokio` for async runtime and timers
- `anyhow` for error propagation
- `tracing`, `tracing-subscriber` for logs
- `directories` for config/data/log locations
- `chrono` for timestamps

## Internal Modules

### `config`

Owns:

- app config schema
- load/save/validate logic
- default config generation

Suggested types:

```rust
struct AppConfig {
    chatbot_ctl_path: PathBuf,
    poll_interval_seconds: u64,
    automation_enabled: bool,
    targets: Vec<TargetConfig>,
}

struct TargetConfig {
    name: String,
    provider: String,
    data_dir: PathBuf,
    continue_text: String,
    enabled: bool,
    cooldown_seconds: u64,
    notes: String,
}
```

### `chatbot_ctl`

Wraps process execution:

- `workers() -> Vec<WorkerStatus>`
- `send(provider, chat_id, text) -> Result<()>`
- executable resolution and health checks

This should isolate shelling out so the rest of the app is testable.

### `session_store`

Owns session inspection logic:

- locate `sessions.json`
- resolve current visible session ID by name
- load transcript JSON
- extract latest message

### `heuristics`

Owns continuation decision rules. Start by preserving current Bash behavior:

- reject messages containing completion language like `no more work`
- accept messages containing signals like `next steps`, `follow-up`, `remaining`, `continue with`

This should be pure and unit tested so the Rust app matches the script.

### `engine`

Background coordinator that:

- runs periodic checks
- applies cooldowns
- emits events/status updates
- serializes or bounds concurrent checks

### `ui`

Renders:

- summary header
- target table
- detail pane
- config editor modal
- logs/diagnostics pane

## Data Flow

1. App starts.
2. Config is loaded.
3. `chatbot-ctl` path and target directories are validated.
4. Background engine starts if automation is enabled.
5. Timer fires.
6. Engine checks enabled targets.
7. Results update in-memory app state.
8. UI repaints with latest statuses.

## UI Design

## Main Window

Top bar:

- automation toggle
- polling interval
- `Check Now`
- `Reload Config`
- `Save Config`

Target table columns:

- enabled
- name
- provider
- resolved session ID
- last message role
- last decision
- last send time
- worker running
- actions

Detail pane for selected target:

- configured continuation text
- recent log lines for that target
- raw paths
- last assistant message preview
- manual action buttons

Bottom diagnostics pane:

- app log
- `chatbot-ctl` health
- config path
- log path

## Scheduler Strategy

V1 should not require cron.

Preferred behavior:

- when the app is open and automation is enabled, the internal timer performs checks
- when the app is closed, automation stops

Optional later enhancement:

- install/remove a user `systemd --user` service and timer on Linux

This is better than cron for a GUI app because it gives explicit lifecycle control and better logging. If background continuation while the window is closed is required, split the project into:

- `chatbot-auto-gui`
- `chatbot-auto-agent`

For now, a single-process desktop app is sufficient.

## Config File

Suggested location:

- `~/.config/chatbot-auto/config.json`

Example:

```json
{
  "chatbot_ctl_path": "/path/to/chatbot-ctl",
  "poll_interval_seconds": 30,
  "automation_enabled": true,
  "targets": [
    {
      "name": "ziglua",
      "provider": "minimax",
      "data_dir": "/path/to/provider-session-data",
      "continue_text": "continue --- commit, push, and update the doc after every feature/fix",
      "enabled": true,
      "cooldown_seconds": 120,
      "notes": ""
    }
  ]
}
```

## Matching Existing Bash Logic

The first Rust version should preserve these decisions exactly or near-exactly:

- session chosen by most recent visible `updated_at`
- skip when worker is `RUNNING`
- skip unless last message is assistant-authored
- skip unless content appears to request more work
- default continuation text is `continue`

This keeps migration risk low. After parity is reached, heuristics can become configurable.

## Error Handling

Per-target failures should be non-fatal:

- missing `sessions.json`
- malformed transcript JSON
- missing session file
- `chatbot-ctl` execution failure
- permission issues

The app should:

- mark the target unhealthy,
- show the last error inline,
- keep checking other targets,
- and allow manual retry.

## Logging

Persist structured logs to something like:

- `~/.local/state/chatbot-auto/app.log`

Each check should log:

- timestamp
- target
- resolved session ID
- worker state
- heuristic decision
- whether a send occurred
- error details if present

## Testing Plan

### Unit Tests

- heuristic keyword acceptance/rejection
- session resolution from sample `sessions.json`
- transcript parsing for last-message extraction
- config validation and defaults

### Integration Tests

- fake `chatbot-ctl` executable fixture for `workers` and `send`
- engine skips when worker is running
- engine sends when assistant message has actionable next steps
- engine does not send when message indicates completion

### Manual Tests

- start app with real data directories
- verify target table state matches current sessions
- confirm `Check Now` sends only when expected
- disable automation and verify timer stops sending

## Implementation Plan

### Phase 1

- Create Rust app skeleton with `eframe`
- Add config load/save
- Add target list UI
- Add `chatbot-ctl` health check

### Phase 2

- Implement session parsing
- Port next-step heuristic
- Implement one-shot `Check Now`
- Add event log view

### Phase 3

- Add periodic engine
- Add per-target cooldowns
- Add manual send/check controls
- Add stronger error reporting

### Phase 4

- Add import path from current shell-script target list
- Add optional scheduler install for Linux
- Add packaging and desktop entry

## Open Decisions

- Whether background execution while the window is closed is a hard requirement
- Whether provider values should be free-form or enum-backed
- Whether continuation heuristics should be globally shared or per-target
- Whether logs should store assistant message excerpts or only decisions

## Recommended First Deliverable

Build the GUI as a thin orchestrator around current behavior, not a redesign of the automation policy.

That means:

- keep using `chatbot-ctl`,
- keep reading the existing session JSON files,
- preserve current continuation heuristics,
- move hard-coded Bash config into editable JSON,
- and replace cron with an in-app timer first.

This gets a usable operator tool quickly and leaves room for a later background daemon if needed.
