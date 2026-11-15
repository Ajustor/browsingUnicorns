//! Browsing Unicorns: a light and fast web browser.
//!
//! This crate is the presentation layer (egui) and the composition root that
//! wires the infrastructure adapters into the application use cases.

// No console window on Windows, debug builds included. Launched from a
// terminal (`cargo run`), the inherited handles still show the logs.
#![windows_subsystem = "windows"]

mod bootstrap;
mod ui;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(ui::APP_NAME)
            .with_app_id("browsing-unicorns")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            .with_icon(window_icon()),
        ..Default::default()
    };
    eframe::run_native(
        ui::APP_NAME,
        options,
        Box::new(|cc| Ok(Box::new(bootstrap::build(cc)?))),
    )
}

/// Title bar and taskbar icon (the .exe's own icon comes from build.rs).
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("assets/icon.png is a valid PNG")
}
