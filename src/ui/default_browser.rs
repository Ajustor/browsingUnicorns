//! Being the system's default browser: the bar that offers it, and the
//! links the OS opens with it, handed by their launch to the profile's open
//! window (see [`crate::bootstrap::hand_over`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use egui::TopBottomPanel;
use egui_phosphor::regular as icon;
use infrastructure::default_browser;

use super::command::Command;

/// Written once the user said "Don't ask again".
const NEVER_ASK: &str = "default-browser-never-ask";

/// How long the known status stays trusted (the user may change it in the
/// system's settings meanwhile: it is looked at again when the window gets
/// the focus back).
const STATUS_TTL: Duration = Duration::from_secs(60);

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
    /// A look running in the background (`xdg-settings` is a process).
    checking: Option<Receiver<Option<bool>>>,
    /// The first answer decides the offer.
    offer_on_answer: bool,
    /// Wakes the window when a look ends.
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    /// The window had the focus last frame.
    focused: bool,
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
            checking: None,
            offer_on_answer: false,
            wake: None,
            focused: true,
        }
    }

    /// For the profile's window: it takes the links other launches hand
    /// over, and offers to become the default browser unless told not to.
    pub fn for_profile_window(profile_dir: &Path, wake: impl Fn() + Send + Sync + 'static) -> Self {
        default_browser::refresh_registration();
        let never_ask = profile_dir.join(NEVER_ASK);
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(wake);
        let requests_wake = wake.clone();
        let mut this = Self {
            requests: Some(infrastructure::profiles::open_requests(
                profile_dir,
                move || requests_wake(),
            )),
            prompt: false,
            offer_on_answer: !never_ask.exists(),
            never_ask: Some(never_ask),
            status: None,
            checked: None,
            checking: None,
            wake: Some(wake),
            focused: true,
        };
        this.look();
        this
    }

    /// The addresses handed over since the last call.
    pub fn take_requests(&self) -> Vec<String> {
        self.requests
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default()
    }

    /// Starts a look in the background, unless one runs or it isn't
    /// available here.
    fn look(&mut self) {
        if !default_browser::supported() || self.checking.is_some() {
            return;
        }
        let Some(wake) = self.wake.clone() else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.checking = Some(rx);
        self.checked = Some(Instant::now());
        std::thread::spawn(move || {
            let _ = tx.send(default_browser::is_default());
            wake();
        });
    }

    /// Takes the answer of a finished look.
    fn poll(&mut self) {
        let Some(answer) = self.checking.as_ref().map(Receiver::try_recv) else {
            return;
        };
        match answer {
            Ok(status) => {
                self.status = status;
                self.checking = None;
                if std::mem::take(&mut self.offer_on_answer) {
                    self.prompt = status == Some(false);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => self.checking = None,
        }
    }

    /// Whether it is the default browser, as last known (`None`: can't
    /// tell yet, or not available on this system). An old answer is looked
    /// at again in the background.
    pub fn status(&mut self) -> Option<bool> {
        if !default_browser::supported() {
            return None;
        }
        self.poll();
        if self.checked.is_none_or(|t| t.elapsed() > STATUS_TTL) {
            self.look();
        }
        self.status
    }

    pub fn run(&mut self, command: DefaultBrowserCommand) -> Result<(), String> {
        self.prompt = false;
        self.offer_on_answer = false;
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
    pub fn banner(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        self.poll();
        // Back from elsewhere (the system's settings): the status may have
        // changed there.
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.focused {
            self.checked = None;
        }
        self.focused = focused;
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
