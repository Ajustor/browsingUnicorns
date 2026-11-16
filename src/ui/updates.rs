//! Update banner and the update commands. A bottom panel and not a popup: it
//! must not be hidden under the page's native view.

use egui::{Spinner, TopBottomPanel};
use egui_phosphor::regular as icon;
use updater::{UpdateEvent, UpdateMemory, UpdateState, Updater};

use super::command::Command;

/// The updater plus what it remembers across runs.
pub struct Updates {
    pub updater: Updater,
    /// `None` in a private window: no automatic check, nothing written.
    memory: Option<UpdateMemory>,
    /// The running check is the automatic one, to record once done.
    auto_check: bool,
}

/// A short message for the status line.
pub struct Notice {
    pub error: bool,
    pub text: String,
    /// A downloaded file, to open or show in its folder.
    pub file: Option<std::path::PathBuf>,
}

impl Updates {
    pub fn new(updater: Updater, memory: Option<UpdateMemory>) -> Self {
        Self {
            updater,
            memory,
            auto_check: false,
        }
    }

    /// The daily automatic check, unless one ran in the last 24 h.
    pub fn check_if_due(&mut self) {
        let Some(memory) = &self.memory else {
            return;
        };
        if updater::should_check(memory.last_check(), updater::unix_now()) {
            self.auto_check = true;
            let skipped = memory.skipped_version();
            self.updater.check(false, skipped.as_deref());
        }
    }

    pub fn check_now(&mut self) {
        self.updater.check(true, None);
    }

    pub fn skip(&mut self) {
        if let Some(version) = self.updater.skip()
            && let Some(memory) = &self.memory
        {
            memory.skip_version(&version);
        }
    }

    /// Results of the background work, as messages to show.
    pub fn poll(&mut self) -> Option<Notice> {
        let event = self.updater.poll();
        if self.auto_check && !matches!(self.updater.state, UpdateState::Checking) {
            self.auto_check = false;
            if let Some(memory) = &self.memory {
                memory.record_check(updater::unix_now());
            }
        }
        let (error, text) = match event? {
            UpdateEvent::UpToDate => (false, "browsingUnicorns is up to date.".to_owned()),
            UpdateEvent::Error(e) => (true, format!("Update: {e}")),
            // The banner shows these.
            UpdateEvent::Available(_) | UpdateEvent::Ready => return None,
        };
        Some(Notice {
            error,
            text,
            file: None,
        })
    }

    pub fn banner(&self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        let state = &self.updater.state;
        let shown = match state {
            UpdateState::Available(_) | UpdateState::Ready(_) => !self.updater.dismissed,
            UpdateState::Downloading(_) => true,
            _ => false,
        };
        if !shown {
            return;
        }
        TopBottomPanel::bottom("update").show(ctx, |ui| {
            ui.horizontal(|ui| match state {
                UpdateState::Available(info) => {
                    ui.label(format!(
                        "{} browsingUnicorns {} is available.",
                        icon::ARROW_CIRCLE_UP,
                        info.version
                    ));
                    if ui.button("Install").clicked() {
                        commands.push(Command::InstallUpdate);
                    }
                    if ui.button("Later").clicked() {
                        commands.push(Command::UpdateLater);
                    }
                    if ui.button("Skip this version").clicked() {
                        commands.push(Command::SkipUpdate);
                    }
                    ui.hyperlink_to("What's new", &info.page_url);
                }
                UpdateState::Downloading(info) => {
                    ui.add(Spinner::new());
                    ui.label(format!("Downloading browsingUnicorns {}…", info.version));
                }
                UpdateState::Ready(info) => {
                    ui.label(format!(
                        "{} browsingUnicorns {} is ready.",
                        icon::CHECK_CIRCLE,
                        info.version
                    ));
                    if ui.button("Restart now").clicked() {
                        commands.push(Command::RestartToUpdate);
                    }
                    if ui.button("Later").clicked() {
                        commands.push(Command::UpdateLater);
                    }
                }
                _ => {}
            });
        });
    }
}
