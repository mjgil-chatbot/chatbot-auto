# chatbot-auto

`chatbot-auto` is a native Rust desktop app for monitoring chatbot sessions and automatically sending a follow-up `continue` prompt when the latest assistant message clearly indicates work remains.

It replaces ad hoc shell-based polling with an inspectable `egui` UI, persistent config, per-target diagnostics, and an import path for legacy watcher definitions.

## Features

- Desktop UI built with `eframe` and `egui`
- Persistent JSON configuration
- Background polling with manual `Check` and `Send` controls
- Session inspection through `sessions.json` and transcript files
- Legacy watcher import with preview, filtering, sorting, and snapshot export
- Bootstrap prompts for build-docs-first workflows

## Requirements

- Rust toolchain with `cargo`
- `chatbot-ctl` available on `PATH`, or set explicitly with the `RUST_CHATBOT_CTL` environment variable
- Access to the provider session data directories you want to monitor

On Linux, `eframe` may require native development packages. The upstream `egui` docs list these Ubuntu packages for native builds: `libclang-dev`, `libgtk-3-dev`, `libxcb-render0-dev`, `libxcb-shape0-dev`, `libxcb-xfixes0-dev`, `libxkbcommon-dev`, and `libssl-dev`.

## Getting Started

1. Run `cargo +1.95.0 run --release`.
2. Configure `chatbot-ctl` if it is not already on `PATH`.
3. Add one or more targets in the UI.
4. If you need to import an older shell watcher, set `legacy_watcher_script_path` explicitly before running the import.

The app stores config and logs in the standard platform-specific application directories resolved by the runtime.

## License

Released under the BSD Zero Clause License. See `LICENSE`.

## Development

- Build: `cargo +1.95.0 check --release --locked --all-targets`
- Test: `cargo +1.95.0 test --release --locked --bins --tests`

The architecture and implementation notes live in `docs/design.md` and `docs/egui-app-design.md`.

The checked release uses Rust 1.95.0. `chatbot-ctl` comes from the sibling
`rust-chatbot` repository; configure its binary path before enabling sends.
Display automation and live provider sessions are separate runtime checks.

Invalid target actions are reported as errors without terminating the background
engine before its completion event. A regression test covers both manual check
and force-send actions with an out-of-range index.
