//! Composition root: the only place that knows every layer.

use std::sync::Arc;
use std::sync::mpsc;

use anyhow::Context as _;
use application::Browser;
use application::ports::SettingsRepository;
use domain::Settings;
use infrastructure::{ParentWindow, TomlSettingsRepository, WryEngine};

use crate::ui::BrowserApp;

pub fn build(cc: &eframe::CreationContext<'_>) -> anyhow::Result<BrowserApp> {
    crate::ui::install_fonts(&cc.egui_ctx);

    let settings = load_settings();

    // Safety: the web views live in the app, which eframe drops before the
    // window.
    let parent = unsafe { ParentWindow::new(cc) }.context("no native window to embed pages in")?;
    let (sender, receiver) = mpsc::channel();
    let ctx = cc.egui_ctx.clone();
    let engine = WryEngine::new(parent, sender, Arc::new(move || ctx.request_repaint()));

    let mut browser = Browser::new(engine, settings);
    browser
        .open_tab(None)
        .context("could not open the first tab")?;
    Ok(BrowserApp::new(browser, receiver))
}

fn load_settings() -> Settings {
    let Some(repo) = TomlSettingsRepository::default_location() else {
        return Settings::default();
    };
    repo.load().unwrap_or_else(|e| {
        log::warn!("settings ignored: {e}");
        Settings::default()
    })
}
