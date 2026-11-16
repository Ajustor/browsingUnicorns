//! Browsing Unicorns: a light and fast web browser.
//!
//! This crate is the presentation layer (egui) and the composition root that
//! wires the infrastructure adapters into the application use cases.
//!
//! Usage: `browsingUnicorns [--profile <id>] [--private] [address or search]`

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
    let args = Args::parse(std::env::args().skip(1));
    eframe::run_native(
        ui::APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(bootstrap::build(cc, slot, args)?))),
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

/// The command line.
#[derive(Debug, Default, PartialEq)]
pub struct Args {
    /// `--profile <id>`: the profile to run (the default one otherwise).
    pub profile: Option<String>,
    /// `--private`: a private window, which leaves nothing behind.
    pub private: bool,
    /// An address or a search to open (a terminal, the OS opening a link).
    pub start_page: Option<String>,
}

impl Args {
    fn parse(mut args: impl Iterator<Item = String>) -> Self {
        let mut parsed = Self::default();
        while let Some(arg) = args.next() {
            if arg == "--profile" {
                parsed.profile = args.next();
            } else if let Some(id) = arg.strip_prefix("--profile=") {
                parsed.profile = Some(id.to_owned());
            } else if arg == "--private" {
                parsed.private = true;
            } else if parsed.start_page.is_none() {
                parsed.start_page = Some(arg);
            }
        }
        parsed
    }
}

/// Title bar and taskbar icon (the .exe's own icon comes from build.rs).
fn window_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("assets/icon.png is a valid PNG")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn profile_and_address() {
        assert_eq!(parse(&[]), Args::default());
        assert_eq!(
            parse(&["https://example.com/"]).start_page.as_deref(),
            Some("https://example.com/")
        );
        let args = parse(&["--profile", "work", "example.com"]);
        assert_eq!(args.profile.as_deref(), Some("work"));
        assert_eq!(args.start_page.as_deref(), Some("example.com"));
        assert_eq!(parse(&["--profile=home"]).profile.as_deref(), Some("home"));
    }

    #[test]
    fn private_window() {
        assert!(!parse(&["example.com"]).private);
        let args = parse(&["--profile", "work", "--private", "example.com"]);
        assert!(args.private);
        assert_eq!(args.profile.as_deref(), Some("work"));
        assert_eq!(args.start_page.as_deref(), Some("example.com"));
    }
}
