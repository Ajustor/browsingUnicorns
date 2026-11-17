//! Browsing Unicorns: a light and fast web browser.
//!
//! This crate is the presentation layer (egui) and the composition root that
//! wires the infrastructure adapters into the application use cases.
//!
//! Usage: `browsingUnicorns [--profile <id>] [--private | --tab-window]
//! [--geometry=x,y,w,h] [address or search]`

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

    let args = Args::parse(std::env::args().skip(1));
    if bootstrap::hand_over(&args) {
        return Ok(());
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(ui::APP_NAME)
        .with_app_id("browsingUnicorns")
        .with_inner_size([1280.0, 800.0])
        .with_min_inner_size([480.0, 320.0])
        .with_icon(window_icon());
    // The tab strip is the title bar: it draws the window's buttons (macOS
    // keeps its own, over the strip).
    viewport = if cfg!(target_os = "macos") {
        viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        viewport.with_decorations(false)
    };
    if let Some([x, y, w, h]) = args.geometry {
        viewport = viewport.with_position([x, y]).with_inner_size([w, h]);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    let exit_slot = ui::ExitSlot::default();
    let slot = exit_slot.clone();
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
    /// `--tab-window`: a tab moved to its own window, the profile being
    /// open in another one.
    pub tab_window: bool,
    /// `--geometry=x,y,w,h`: where the window opens, in points.
    pub geometry: Option<[f32; 4]>,
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
            } else if arg == "--tab-window" {
                parsed.tab_window = true;
            } else if let Some(g) = arg.strip_prefix("--geometry=") {
                parsed.geometry = parse_geometry(g);
            } else if parsed.start_page.is_none() {
                parsed.start_page = Some(arg);
            }
        }
        parsed
    }
}

/// `x,y,w,h`; `None` unless four numbers with a positive size.
fn parse_geometry(text: &str) -> Option<[f32; 4]> {
    let values: Vec<f32> = text
        .split(',')
        .map(|v| v.trim().parse().ok())
        .collect::<Option<_>>()?;
    let geometry: [f32; 4] = values.try_into().ok()?;
    (geometry[2] > 0.0 && geometry[3] > 0.0).then_some(geometry)
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
    fn tab_window() {
        let args = parse(&["--tab-window", "--geometry=10,-20,800,600", "a.com"]);
        assert!(args.tab_window && !args.private);
        assert_eq!(args.geometry, Some([10.0, -20.0, 800.0, 600.0]));
        assert_eq!(args.start_page.as_deref(), Some("a.com"));
        assert_eq!(parse(&["--geometry=1,2,0,4"]).geometry, None);
        assert_eq!(parse(&["--geometry=1,2,3"]).geometry, None);
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
