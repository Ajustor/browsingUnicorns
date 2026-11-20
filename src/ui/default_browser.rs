//! Being the system's default browser: the bar that offers it, and the
//! links the OS opens with it, handed by their launch to the profile's open
//! window (see [`crate::bootstrap::hand_over`]).

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use egui::TopBottomPanel;
use egui_phosphor::regular as icon;
use infrastructure::default_browser;

use super::command::Command;

/// Written once the user said "Don't ask again".
const NEVER_ASK: &str = "default-browser-never-ask";

/// How long the known status stays trusted (the user may change it in the
/// system's settings meanwhile).
const STATUS_TTL: Duration = Duration::from_secs(3);

/// The answers to the bar, and the About view's button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultBrowserCommand {
    Make,
    NotNow,
    NeverAsk,
}

pub struct DefaultBrowser {
    requests: Option<Receiver<String>>,
    prompt: bool,
    never_ask: Option<PathBuf>,
    status: Option<bool>,
    checked: Option<Instant>,
}

impl DefaultBrowser {
    /// For a window that neither holds its profile (private, tab window)
    /// nor offers anything.
    pub fn inert() -> Self {
        Self {
            requests: None,
            prompt: false,
            never_ask: None,
            status: None,
            checked: None,
        }
    }

    /// For the profile's window: it takes the links other launches hand
    /// over, and offers to become the default browser unless told not to.
    pub fn for_profile_window(profile_dir: &Path, wake: impl Fn() + Send + 'static) -> Self {
        default_browser::refresh_registration();
        let never_ask = profile_dir.join(NEVER_ASK);
        let mut this = Self {
            requests: Some(infrastructure::profiles::open_requests(profile_dir, wake)),
            prompt: false,
            never_ask: Some(never_ask.clone()),
            status: None,
            checked: None,
        };
        this.prompt = !never_ask.exists() && this.status() == Some(false);
        this
    }

    /// The addresses handed over since the last call.
    pub fn take_requests(&self) -> Vec<String> {
        self.requests
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default()
    }

    /// Whether it is the default browser (`None`: can't tell, or not
    /// available on this system).
    pub fn status(&mut self) -> Option<bool> {
        if !default_browser::supported() {
            return None;
        }
        if self.checked.is_none_or(|t| t.elapsed() > STATUS_TTL) {
            self.status = default_browser::is_default();
            self.checked = Some(Instant::now());
        }
        self.status
    }

    pub fn run(&mut self, command: DefaultBrowserCommand) -> Result<(), String> {
        self.prompt = false;
        match command {
            DefaultBrowserCommand::Make => {
                self.checked = None;
                default_browser::make_default()
            }
            DefaultBrowserCommand::NotNow => Ok(()),
            DefaultBrowserCommand::NeverAsk => {
                if let Some(path) = &self.never_ask
                    && let Err(e) = std::fs::write(path, "")
                {
                    log::warn!("{}: {e}", path.display());
                }
                Ok(())
            }
        }
    }

    /// The bar offering it, at the window's bottom.
    pub fn banner(&self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        if !self.prompt {
            return;
        }
        TopBottomPanel::bottom("default_browser").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} {}",
                    icon::BROWSER,
                    i18n::default_browser::offer()
                ));
                let mut answer = |label: &str, command| {
                    if ui.button(label).clicked() {
                        commands.push(Command::DefaultBrowser(command));
                    }
                };
                answer(
                    i18n::default_browser::set_as_default(),
                    DefaultBrowserCommand::Make,
                );
                answer(
                    i18n::default_browser::not_now(),
                    DefaultBrowserCommand::NotNow,
                );
                answer(
                    i18n::default_browser::never_ask(),
                    DefaultBrowserCommand::NeverAsk,
                );
            });
        });
    }
}
