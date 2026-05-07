mod app;
mod chatbot_ctl;
mod config;
mod engine;
mod heuristics;
mod legacy_import;
mod session_store;

use std::sync::Arc;

use anyhow::Result;
use app::ChatbotAutoApp;
use config::AppPaths;

fn main() -> Result<()> {
    let paths = Arc::new(AppPaths::discover()?);
    let config = config::load_or_default(paths.as_ref())?;
    let native_options = eframe::NativeOptions::default();

    eframe::run_native(
        "chatbot-auto",
        native_options,
        Box::new(move |_cc| Ok(Box::new(ChatbotAutoApp::new(config.clone(), paths.clone())))),
    )
    .map_err(|error| anyhow::anyhow!("failed to start native app: {error}"))
}
