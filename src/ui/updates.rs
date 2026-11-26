//! Update banner and the update commands. A bottom panel and not a popup: it
//! must not be hidden under the page's native view.

use egui::{Button, Color32, CornerRadius, RichText, Spinner, Stroke, TopBottomPanel};
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
            UpdateEvent::UpToDate => (false, i18n::updates::up_to_date().to_owned()),
            UpdateEvent::Error(e) => (true, i18n::updates::failed(&e)),
            // The banner shows these.
            UpdateEvent::Available(_) | UpdateEvent::Ready => return None,
        };
        Some(Notice { error, text })
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
        // In the accent, white on violet: it must not be missed.
        let frame = egui::Frame::NONE
            .fill(super::panel_kit::ACCENT)
            .inner_margin(egui::Margin::symmetric(12, 8));
        TopBottomPanel::bottom("update")
            .frame(frame)
            .show_separator_line(false)
            .show(ctx, |ui| {
                ui.visuals_mut().override_text_color = Some(Color32::WHITE);
                ui.horizontal_wrapped(|ui| match state {
                    UpdateState::Available(info) => {
                        ui.label(RichText::new(icon::ARROW_CIRCLE_UP).size(18.0));
                        ui.label(RichText::new(i18n::updates::available(&info.version)).strong());
                        if main_button(ui, i18n::updates::install()).clicked() {
                            commands.push(Command::InstallUpdate);
                        }
                        if quiet_button(ui, i18n::updates::whats_new()).clicked() {
                            commands.push(Command::ShowUpdateNotes);
                        }
                        if quiet_button(ui, i18n::updates::later()).clicked() {
                            commands.push(Command::UpdateLater);
                        }
                        if quiet_button(ui, i18n::updates::skip_version()).clicked() {
                            commands.push(Command::SkipUpdate);
                        }
                    }
                    UpdateState::Downloading(info) => {
                        ui.add(Spinner::new().color(Color32::WHITE));
                        ui.label(RichText::new(i18n::updates::downloading(&info.version)).strong());
                    }
                    UpdateState::Ready(info) => {
                        ui.label(RichText::new(icon::CHECK_CIRCLE).size(18.0));
                        ui.label(RichText::new(i18n::updates::ready(&info.version)).strong());
                        if main_button(ui, i18n::updates::restart_now()).clicked() {
                            commands.push(Command::RestartToUpdate);
                        }
                        if quiet_button(ui, i18n::updates::later()).clicked() {
                            commands.push(Command::UpdateLater);
                        }
                    }
                    _ => {}
                });
            });
    }
}

/// The banner's main button: white, the text in the accent.
fn main_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(text).strong().color(super::panel_kit::ACCENT))
            .fill(Color32::WHITE)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8)),
    )
}

/// The banner's other buttons: outlined in white.
fn quiet_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(text).color(Color32::WHITE))
            .fill(Color32::from_white_alpha(20))
            .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(90)))
            .corner_radius(CornerRadius::same(8)),
    )
}

/// What to do with an update that is there: its button's glyph, text and
/// tip, and its command (`None` while it downloads). `None`: no update.
pub fn update_action(
    state: &UpdateState,
) -> Option<(&'static str, &'static str, String, Option<Command>)> {
    Some(match state {
        UpdateState::Available(i) => (
            icon::ARROW_CIRCLE_UP,
            i18n::updates::update_button(),
            i18n::updates::available(&i.version),
            Some(Command::InstallUpdate),
        ),
        UpdateState::Downloading(i) => (
            icon::DOWNLOAD_SIMPLE,
            i18n::updates::downloading_button(),
            i18n::updates::downloading(&i.version),
            None,
        ),
        UpdateState::Ready(i) => (
            icon::ARROW_CLOCKWISE,
            i18n::updates::restart_button(),
            i18n::updates::ready(&i.version),
            Some(Command::RestartToUpdate),
        ),
        _ => return None,
    })
}

/// The toolbar's button while an update is there (even with its banner put
/// off): filled, hard to miss. Installs it, or restarts on it.
pub fn toolbar_button(ui: &mut egui::Ui, state: &UpdateState, commands: &mut Vec<Command>) {
    let Some((glyph, text, tip, command)) = update_action(state) else {
        return;
    };
    let button = Button::new(
        RichText::new(format!("{glyph} {text}"))
            .strong()
            .size(13.0)
            .color(Color32::WHITE),
    )
    .fill(super::panel_kit::ACCENT)
    .stroke(Stroke::new(1.0_f32, super::theme::CYAN))
    .corner_radius(CornerRadius::same(255));
    let response = ui.add(button).on_hover_text(tip);
    if response.hovered() && command.is_some() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked()
        && let Some(command) = command
    {
        commands.push(command);
    }
}
