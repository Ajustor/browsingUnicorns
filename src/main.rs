//! Browsing Unicorns: a light and fast web browser.
//!
//! This crate is the presentation layer (egui) and the composition root that
//! wires the infrastructure adapters into the application use cases.
//!
//! Usage: `browsingUnicorns [address or search]`

// No console window on Windows, debug builds included. Launched from a
// terminal (`cargo run`), the inherited handles still show the logs.
#![windows_subsystem = "windows"]

mod bootstrap;
mod ui;

use updater::AppInfo;

/// What the self-updater needs to know about this app.
pub static APP: AppInfo = AppInfo {
    name: "browsingUnicorns",
    version: env!("CARGO_PKG_VERSION"),
    manifest_url: "https://ajustor.github.io/browsingUnicorns/latest.json",
};

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    updater::cleanup_stale_staging(&APP);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(ui::APP_NAME)
            .with_app_id("browsingUnicorns")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            .with_icon(window_icon()),
        ..Default::default()
    };
    let exit_slot = ui::ExitSlot::default();
    let slot = exit_slot.clone();
    let start_page = std::env::args().nth(1);
    eframe::run_native(
        ui::APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(bootstrap::build(cc, slot, start_page)?))),
    )?;

    // The window is gone: apply a downloaded update if one was scheduled.
    let action = exit_slot
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .take();
    if let Some(action) = action
        && let Err(e) = updater::run_exit_action(&action, &[])
    {
        log::error!("could not apply the update ({action:?}): {e}");
    }
    Ok(())
}

/// Title bar and taskbar icon (the .exe's own icon comes from build.rs).
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("assets/icon.png is a valid PNG")
}
