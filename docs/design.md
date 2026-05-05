# `chatbot-auto` Design

## Goal

Build a desktop `egui` app that watches chatbot sessions and automatically
sends `continue` only when the latest assistant message clearly indicates more
work remains and no worker is already active.

## Current Product Shape

- Native Rust desktop UI with `eframe` / `egui`
- Persistent JSON config
- Background polling loop instead of cron
- Manual controls for `Check`, `Check Now`, and `Send`
- Session inspection through `sessions.json` and transcript files
- `chatbot-ctl` integration for worker checks and sends

## Build-Docs Bootstrap

Some targets need a one-time bootstrap before the normal continue loop should
resume. For those targets the app should:

1. Detect that `build_docs_before_continue` is enabled.
2. Send a dedicated bootstrap prompt before any normal continue prompt.
3. Include the repo path plus design/task document paths in that prompt.
4. Mark the bootstrap as completed only after a successful send.
5. Reset the completion flag whenever bootstrap inputs materially change.

Bootstrap inputs are:

- repo path
- design document path
- task document path
- global default bootstrap prompt

## Persistence Rules

- Config changes should persist automatically.
- Manual `Save Config` remains available.
- The app must not keep stale `build_docs_completed=true` state after bootstrap
  inputs change.

## Current Status

The current bootstrap feature set now includes:

- config validation for missing or inconsistent bootstrap paths
- engine-side skip behavior when bootstrap configuration is invalid
- operator-facing diagnostics in the target details pane
- config normalization for provider data directories and `~/...` bootstrap paths
- runtime/autosave normalization so engine runs and persisted config snapshots use expanded paths and provider defaults consistently

The app also now supports one-shot import from the legacy watcher script:

- reads `TARGET_SPECS` from the existing shell-script watcher
- imports per-target custom continue text from `TARGET_TEXT`
- merges imported targets into the desktop config while preserving local-only UI state such as notes, cooldowns, and bootstrap flags
- lets the operator override the watcher script path in config/UI instead of relying on a hard-coded workstation path
- reports how many targets were added vs updated during import
- previews imported targets before config mutation, including add/update/unchanged classification
- lets operators filter preview entries by `all`, `add`, `update`, or `unchanged`
- lets operators sort preview entries by kind/name, provider/name, or name
- lets operators search preview entries by target name or provider
- distinguishes zero-match preview searches from empty preview states with explicit messaging
- validates the configured watcher import path before import and surfaces those issues in the UI/logs
- persists the last legacy import outcome so operators can still inspect it after reload
- shows the latest legacy import outcome in an always-visible main-panel status box instead of hiding it inside collapsed settings
- keeps a compact recent legacy-import history so operators can distinguish recent preview/apply/dismiss/skip/fail outcomes without scanning logs
- lets operators clear persisted legacy-import status/history from the main panel
- lets operators expand the compact recent-history view to inspect the full retained import history
- lets operators jump from a preview entry to the matching configured target when one already exists
- lets operators copy preview entry identity text (`provider / target`) directly from the import review UI
- lets operators copy a full one-line preview summary for a row, including kind and detail text
- lets operators copy all currently visible preview summaries in one shot after filter/search/sort have been applied
- lets operators copy a higher-level review snapshot that includes source path, counts, and the visible preview rows
- includes filter/search/sort context in the copied review snapshot so exports reflect how the visible set was derived
- offers a markdown-formatted review snapshot for pasting into external trackers or docs
- offers a JSON-formatted review snapshot for downstream tooling or machine-readable workflows
- includes explicit machine-readable entry fields in JSON exports, such as kind, detail, identity, provider, and name
- includes stable preview entry ids in JSON exports so downstream tooling can correlate repeated imports
- includes stable preview entry ids in the plain-text and markdown review snapshot exports as well
- lets operators show stable preview entry ids inline in the review UI without exporting first
- persists the stable-id display preference so the preview UI comes back in the same state after reload
- keeps visible feedback after preview-to-target navigation so the selected target is clearly marked as preview-focused
- shows richer inline preview metadata so add/update/unchanged rows communicate meaning at a glance

## Verification

Bootstrap behavior is now covered by engine-level tests for:

- bootstrap send path
- bootstrap skip when repo path is missing
- bootstrap skip when design document path is missing
- bootstrap error propagation when `chatbot-ctl send` fails

Config behavior is now covered by config-level tests for:

- bootstrap path validation failures
- normalization of provider data directories
- expansion of `~/...` repo, design, and task document paths
- normalization before runtime config cloning and autosave persistence
- persisting preview-display preferences through config save/load

Legacy-import behavior is now covered by tests for:

- parsing nul-delimited imported target records
- importing `TARGET_SPECS` plus per-target `TARGET_TEXT` from a watcher fixture
- merging imported targets without losing local-only target settings
- normalizing `~/...` watcher-script paths in config
- summarizing added vs updated targets during import
- classifying import preview entries as add, update, or unchanged
- filtering preview entries by classification during review
- sorting preview entries in multiple review-friendly orders
- searching preview entries by target name or provider
- distinguishing zero-match search results from filter-based empty states
- validating empty, missing, and non-file watcher import paths before import is attempted
- persisting and reloading the last structured legacy import result
- formatting a stable always-visible summary for the latest legacy import outcome
- retaining and rendering a capped recent history of legacy import outcomes
- clearing persisted legacy import status/history
- switching between compact and expanded legacy import history views
- locating configured targets by provider/name for preview-to-target navigation
- formatting stable preview entry identity text for quick copy actions
- formatting stable one-line preview summaries for quick copy/export actions
- exporting the currently visible preview-review set as joined one-line summaries
- exporting a fuller preview review snapshot with source path and count context
- exporting review-state context alongside the preview snapshot
- exporting preview review snapshots in a structured markdown shape
- exporting preview review snapshots in a machine-readable JSON shape
- exporting richer machine-readable entry fields in JSON review snapshots
- exporting stable machine-readable preview entry ids
- exporting stable preview entry ids consistently across text, markdown, and JSON snapshot formats
- formatting stable preview entry ids for optional inline display in the review UI
- persisting the inline stable-id display preference across reloads
- formatting target labels and details feedback for preview-focused selections
- mapping preview row kinds to stable badge/detail metadata
