//! The updates: the window offering one found at start, the toolbar's
//! button, the update commands.

use egui::{Button, Color32, CornerRadius, RichText, Spinner, Stroke};
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
    /// The window offering the update is open (an update found by the
    /// automatic check, at start).
    modal: bool,
}

/// A short message for the status line.
pub struct Notice {
    pub error: bool,
    pub text: String,
}

/// Whether a check's `event` opens the update's window: an update the
/// automatic check found (at start), not one looked for by hand (the
/// About view shows it).
fn opens_window(event: &UpdateEvent, automatic: bool) -> bool {
    automatic && matches!(event, UpdateEvent::Available(_))
}

impl Updates {
    pub fn new(updater: Updater, memory: Option<UpdateMemory>) -> Self {
        Self {
            updater,
            memory,
            auto_check: false,
            modal: false,
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
        let automatic = self.auto_check;
        if self.auto_check && !matches!(self.updater.state, UpdateState::Checking) {
            self.auto_check = false;
            if let Some(memory) = &self.memory {
                memory.record_check(updater::unix_now());
            }
        }
        let event = event?;
        if opens_window(&event, automatic) {
            self.modal = true;
        }
        let (error, text) = match event {
            UpdateEvent::UpToDate => (false, i18n::updates::up_to_date().to_owned()),
            UpdateEvent::Error(e) => (true, i18n::updates::failed(&e)),
            // The window, the toolbar's button and the About view show these.
            UpdateEvent::Available(_) | UpdateEvent::Ready => return None,
        };
        Some(Notice { error, text })
    }

    /// Whether the update's window is open (the pages are hidden under it:
    /// their native views would cover it).
    pub fn window_open(&self) -> bool {
        self.modal
            && matches!(
                self.updater.state,
                UpdateState::Available(_) | UpdateState::Downloading(_) | UpdateState::Ready(_)
            )
    }

    /// The window offering the update found at start: what it brings, then
    /// install, later or skip; while it downloads, its progress; then
    /// restart. Closed (Escape, a click beside it), it is "later".
    pub fn window(&mut self, ctx: &egui::Context, commands: &mut Vec<Command>) {
        if !self.window_open() {
            return;
        }
        let state = self.updater.state.clone();
        let current = self.updater.current_version();
        let dark = ctx.style().visuals.dark_mode;
        let panel = if dark {
            super::theme::dark::PANEL
        } else {
            super::theme::light::PANEL
        };
        let frame = egui::Frame::NONE
            .fill(panel)
            .corner_radius(CornerRadius::same(16))
            .shadow(egui::Shadow {
                offset: [0, 8],
                blur: 32,
                spread: 0,
                color: Color32::from_black_alpha(120),
            });
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("update window"))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(150))
            .show(ctx, |ui| {
                ui.set_width(WINDOW_WIDTH);
                header(ui, &state, current);
                egui::Frame::NONE
                    .inner_margin(egui::Margin::symmetric(20, 14))
                    .show(ui, |ui| {
                        // Buttons big enough for a window that asks.
                        ui.spacing_mut().button_padding = egui::vec2(14.0, 7.0);
                        ui.spacing_mut().item_spacing.x = 8.0;
                        close = body(ui, &state, commands);
                    });
            });
        if close {
            self.modal = false;
        } else if response.should_close() {
            if !matches!(state, UpdateState::Downloading(_)) {
                commands.push(Command::UpdateLater);
            }
            self.modal = false;
        }
    }
}

/// The update window's width (points).
const WINDOW_WIDTH: f32 = 460.0;

/// The window's top: the browser's gradient, a big icon, the title and the
/// versions.
fn header(ui: &mut egui::Ui, state: &UpdateState, current: &str) {
    let (glyph, title, version) = match state {
        UpdateState::Available(i) => (icon::GIFT, i18n::updates::window_title(), &i.version),
        UpdateState::Downloading(i) => (
            icon::DOWNLOAD_SIMPLE,
            i18n::updates::window_downloading(),
            &i.version,
        ),
        UpdateState::Ready(i) => (
            icon::ROCKET_LAUNCH,
            i18n::updates::window_ready(),
            &i.version,
        ),
        _ => return,
    };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(WINDOW_WIDTH, 112.0), egui::Sense::hover());
    // The accent, rounded at the top like the window, two glows of the
    // gradient's other colours in it.
    let painter = ui.painter_at(rect);
    painter.rect_filled(
        rect,
        CornerRadius {
            nw: 16,
            ne: 16,
            sw: 0,
            se: 0,
        },
        super::panel_kit::ACCENT,
    );
    let [deep, _, cyan] = super::theme::GRADIENT;
    painter.circle_filled(
        egui::pos2(rect.right() - 70.0, rect.bottom() + 10.0),
        62.0,
        cyan.gamma_multiply(0.3),
    );
    painter.circle_filled(
        egui::pos2(rect.left() + 150.0, rect.bottom() + 40.0),
        70.0,
        deep.gamma_multiply(0.3),
    );
    let white = Color32::WHITE;
    painter.circle_filled(
        egui::pos2(rect.left() + 56.0, rect.center().y),
        30.0,
        Color32::from_white_alpha(40),
    );
    painter.text(
        egui::pos2(rect.left() + 56.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(30.0),
        white,
    );
    painter.text(
        egui::pos2(rect.left() + 100.0, rect.center().y - 12.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(20.0),
        white,
    );
    painter.text(
        egui::pos2(rect.left() + 100.0, rect.center().y + 14.0),
        egui::Align2::LEFT_CENTER,
        format!("{current}  {}  {version}", icon::ARROW_RIGHT),
        egui::FontId::proportional(14.0),
        Color32::from_white_alpha(220),
    );
}

/// What the update brings and its buttons. Returns whether the window
/// closes.
fn body(ui: &mut egui::Ui, state: &UpdateState, commands: &mut Vec<Command>) -> bool {
    let mut close = false;
    match state {
        UpdateState::Available(info) => {
            if !info.notes.trim().is_empty() {
                ui.label(RichText::new(i18n::whats_new::in_this_update(&info.version)).strong());
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let lines: Vec<&str> = info.notes.lines().collect();
                        super::whats_new::notes(ui, &super::whats_new::sections(&lines));
                    });
                ui.add_space(12.0);
            }
            ui.horizontal(|ui| {
                if super::panel_kit::primary_button(
                    ui,
                    format!(
                        "{} {}",
                        icon::ARROW_CIRCLE_UP,
                        i18n::updates::update_button()
                    ),
                    true,
                )
                .clicked()
                {
                    commands.push(Command::InstallUpdate);
                }
                if ui.button(i18n::updates::later()).clicked() {
                    commands.push(Command::UpdateLater);
                    close = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let skip = ui.add(
                        Button::new(RichText::new(i18n::updates::skip_version()).weak())
                            .frame(false),
                    );
                    if skip.clicked() {
                        commands.push(Command::SkipUpdate);
                        close = true;
                    }
                });
            });
        }
        UpdateState::Downloading(info) => {
            ui.horizontal(|ui| {
                ui.add(Spinner::new().size(18.0));
                ui.label(i18n::updates::downloading(&info.version));
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.weak(i18n::updates::downloading_hint());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(i18n::updates::hide()).clicked() {
                        close = true;
                    }
                });
            });
        }
        UpdateState::Ready(info) => {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(icon::CHECK_CIRCLE)
                        .size(18.0)
                        .color(super::panel_kit::ok_text(ui)),
                );
                ui.label(i18n::updates::ready(&info.version));
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if super::panel_kit::primary_button(
                    ui,
                    format!("{} {}", icon::ARROW_CLOCKWISE, i18n::updates::restart_now()),
                    true,
                )
                .clicked()
                {
                    commands.push(Command::RestartToUpdate);
                }
                if ui.button(i18n::updates::later()).clicked() {
                    commands.push(Command::UpdateLater);
                    close = true;
                }
            });
        }
        _ => close = true,
    }
    close
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_update_found_at_start_opens_the_window() {
        let found = UpdateEvent::Available("1.2.3".parse().unwrap());
        assert!(opens_window(&found, true));
        // Looked for by hand: the About view shows it.
        assert!(!opens_window(&found, false));
        assert!(!opens_window(&UpdateEvent::UpToDate, true));
        assert!(!opens_window(&UpdateEvent::Ready, true));
    }
}
