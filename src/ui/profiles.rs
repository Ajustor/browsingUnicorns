//! Profiles on the desktop: each runs in its own window (its own process);
//! the panel lists them, opens, adds, renames and removes them.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use application::{ProfileManager, ProfilesError};
use domain::{PROFILE_COLORS, Profile, ProfileId};
use egui::{Button, Color32, RichText, Sense, TextEdit, Vec2};
use egui_phosphor::regular as icon;
use infrastructure::profiles::{ProfileLock, open_window};

use super::command::Command;
use super::panel_kit::{
    self, card, card_frame, chip, danger_button, field, icon_button, list_row, primary_button,
};

/// The profile this window runs and the list, with which ones are open.
pub struct ProfilesRunner {
    manager: ProfileManager,
    /// Held while this window runs: other windows see the profile open.
    _lock: Option<ProfileLock>,
    open: HashSet<ProfileId>,
    checked: Option<Instant>,
}

/// How often the panel looks at what other windows changed.
const REFRESH_EVERY: Duration = Duration::from_secs(1);

impl ProfilesRunner {
    pub fn new(manager: ProfileManager, lock: Option<ProfileLock>) -> Self {
        Self {
            manager,
            _lock: lock,
            open: HashSet::new(),
            checked: None,
        }
    }

    pub fn current(&self) -> &Profile {
        self.manager.current()
    }

    /// Whether to name the profile (in the title): there are several.
    pub fn several(&self) -> bool {
        self.manager.profiles().items().len() > 1
    }

    /// Picks up the other windows' changes, while the panel shows.
    pub fn refresh(&mut self, ctx: &egui::Context) {
        if self.checked.is_some_and(|at| at.elapsed() < REFRESH_EVERY) {
            return;
        }
        self.checked = Some(Instant::now());
        if let Err(e) = self.manager.reload() {
            log::warn!("profiles: {e}");
        }
        self.open = self
            .manager
            .profiles()
            .items()
            .iter()
            .filter(|p| self.manager.is_open(&p.id))
            .map(|p| p.id.clone())
            .collect();
        ctx.request_repaint_after(REFRESH_EVERY);
    }

    pub fn run(&mut self, command: ProfileCommand) -> Result<(), String> {
        self.checked = None;
        let result = match command {
            ProfileCommand::Add(name) => self
                .manager
                .add(&name)
                .and_then(|id| open_window(&id).map_err(|e| storage(e.to_string()))),
            ProfileCommand::Open(id) => open_window(&id).map_err(|e| storage(e.to_string())),
            ProfileCommand::Rename(id, name) => self.manager.rename(&id, &name),
            ProfileCommand::SetColor(id, color) => self.manager.set_color(&id, color),
            ProfileCommand::Remove(id) => self.manager.remove(&id),
        };
        result.map_err(|e| e.to_string())
    }
}

fn storage(e: String) -> ProfilesError {
    ProfilesError::Storage(application::ports::RepositoryError(e))
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProfileCommand {
    /// Adds a profile and opens it.
    Add(String),
    /// Opens the profile in a new window.
    Open(ProfileId),
    Rename(ProfileId, String),
    SetColor(ProfileId, u8),
    /// Removes a profile and deletes its data.
    Remove(ProfileId),
}

/// The avatars' colors, by [`Profile::color`].
const COLORS: [Color32; PROFILE_COLORS as usize] = [
    Color32::from_rgb(0x7c, 0x5c, 0xd6),
    Color32::from_rgb(0xd6, 0x4f, 0x8c),
    Color32::from_rgb(0x2e, 0x9c, 0xca),
    Color32::from_rgb(0x3f, 0xa8, 0x6b),
    Color32::from_rgb(0xe0, 0x8a, 0x2c),
    Color32::from_rgb(0xc9, 0x48, 0x3c),
    Color32::from_rgb(0x4b, 0x6c, 0xd1),
    Color32::from_rgb(0x6b, 0x72, 0x80),
];

fn color(index: u8) -> Color32 {
    COLORS[index as usize % COLORS.len()]
}

/// A disc of the profile's color with its initial.
pub fn avatar(ui: &mut egui::Ui, profile: &Profile, size: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let mut fill = color(profile.color);
        if response.hovered() {
            fill = fill.gamma_multiply(0.85);
        }
        painter.circle_filled(rect.center(), size / 2.0, fill);
        let initial = profile
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect::<String>())
            .unwrap_or_default();
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            initial,
            egui::FontId::proportional(size * 0.55),
            Color32::WHITE,
        );
    }
    response
}

/// What the panel keeps between frames.
#[derive(Default)]
pub struct ProfilesPanel {
    /// The current profile's name being edited (`None`: not touched yet).
    name: Option<String>,
    new_name: String,
    confirm_remove: Option<ProfileId>,
}

impl ProfilesPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        runner: &ProfilesRunner,
        commands: &mut Vec<Command>,
    ) {
        let current = runner.current();
        ui.add(egui::Label::new(RichText::new(i18n::profiles::intro()).weak().size(12.5)).wrap());
        ui.add_space(8.0);

        card_frame(ui, |ui| {
            ui.horizontal(|ui| {
                avatar(ui, current, 44.0);
                ui.add_space(4.0);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.add(
                        egui::Label::new(RichText::new(&current.name).strong().size(17.0))
                            .truncate(),
                    );
                    ui.label(
                        RichText::new(format!(
                            "{} · {}",
                            i18n::profiles::this_profile(),
                            i18n::profiles::this_window()
                        ))
                        .weak()
                        .size(12.5),
                    );
                });
            });
            ui.add_space(6.0);
            let name = self.name.get_or_insert_with(|| current.name.clone());
            // The button first, from the right: the field takes the rest.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let changed = name.trim() != current.name && !name.trim().is_empty();
                    let clicked = ui
                        .add_enabled(
                            changed,
                            Button::new(format!(
                                "{} {}",
                                icon::PENCIL_SIMPLE,
                                i18n::profiles::rename()
                            )),
                        )
                        .clicked();
                    let field = ui.add(
                        TextEdit::singleline(name)
                            .margin(egui::Margin::symmetric(8, 6))
                            .desired_width(ui.available_width()),
                    );
                    let submitted =
                        field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if (clicked || submitted) && changed {
                        commands.push(Command::Profile(ProfileCommand::Rename(
                            current.id.clone(),
                            name.trim().to_owned(),
                        )));
                    }
                });
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for index in 0..PROFILE_COLORS {
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                    let painter = ui.painter();
                    let chosen = index == current.color;
                    painter.circle_filled(rect.center(), 10.0, color(index));
                    if chosen {
                        painter.circle_stroke(
                            rect.center(),
                            11.5,
                            egui::Stroke::new(2.0_f32, ui.visuals().strong_text_color()),
                        );
                        painter.text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            icon::CHECK,
                            egui::FontId::proportional(12.0),
                            Color32::WHITE,
                        );
                    } else if response.hovered() {
                        painter.circle_stroke(
                            rect.center(),
                            11.5,
                            egui::Stroke::new(1.0_f32, ui.visuals().weak_text_color()),
                        );
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if response
                        .on_hover_text(i18n::profiles::color_tip())
                        .clicked()
                        && !chosen
                    {
                        commands.push(Command::Profile(ProfileCommand::SetColor(
                            current.id.clone(),
                            index,
                        )));
                    }
                }
            });
        });

        let others: Vec<&Profile> = runner
            .manager
            .profiles()
            .items()
            .iter()
            .filter(|p| p.id != current.id)
            .collect();
        card(
            ui,
            icon::USERS_THREE,
            i18n::profiles::other_profiles(),
            None,
            |ui| {
                if others.is_empty() {
                    ui.weak(i18n::profiles::no_other());
                }
                for p in others {
                    self.other(ui, runner, p, commands);
                }
            },
        );

        card(
            ui,
            icon::USER_PLUS,
            i18n::profiles::new_profile_title(),
            Some(i18n::profiles::opens_in_new_window()),
            |ui| {
                let field = ui.add(field(&mut self.new_name, i18n::profiles::name_hint()));
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let can = !self.new_name.trim().is_empty();
                if (primary_button(ui, format!("{} {}", icon::PLUS, i18n::profiles::add()), can)
                    .clicked()
                    || submitted)
                    && can
                {
                    commands.push(Command::Profile(ProfileCommand::Add(
                        std::mem::take(&mut self.new_name).trim().to_owned(),
                    )));
                }
            },
        );
    }

    /// Another profile: open it, or remove it (once confirmed).
    fn other(
        &mut self,
        ui: &mut egui::Ui,
        runner: &ProfilesRunner,
        p: &Profile,
        commands: &mut Vec<Command>,
    ) {
        let open = runner.open.contains(&p.id);
        let row = list_row(ui, |ui| {
            ui.horizontal(|ui| {
                avatar(ui, p, 28.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if open {
                        chip(ui, i18n::profiles::is_open(), panel_kit::ok_text(ui));
                    } else if !p.id.is_default()
                        && icon_button(ui, icon::TRASH, i18n::profiles::remove()).clicked()
                    {
                        self.confirm_remove = Some(p.id.clone());
                    }
                    if !open
                        && ui
                            .small_button(format!(
                                "{} {}",
                                icon::ARROW_SQUARE_OUT,
                                i18n::profiles::open()
                            ))
                            .clicked()
                    {
                        commands.push(Command::Profile(ProfileCommand::Open(p.id.clone())));
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(&p.name).strong()).truncate());
                    });
                });
            });
        });
        if row
            .on_hover_text(i18n::profiles::open_in_new_window_tip())
            .clicked()
            && !open
        {
            commands.push(Command::Profile(ProfileCommand::Open(p.id.clone())));
        }
        if self.confirm_remove.as_ref() == Some(&p.id) {
            ui.add(
                egui::Label::new(
                    RichText::new(i18n::profiles::confirm_remove(&p.name))
                        .color(ui.visuals().warn_fg_color),
                )
                .wrap(),
            );
            ui.horizontal(|ui| {
                if danger_button(
                    ui,
                    format!("{} {}", icon::TRASH, i18n::profiles::remove()),
                    true,
                )
                .clicked()
                {
                    commands.push(Command::Profile(ProfileCommand::Remove(p.id.clone())));
                    self.confirm_remove = None;
                }
                if ui.button(i18n::common::cancel()).clicked() {
                    self.confirm_remove = None;
                }
            });
        }
    }

    /// The current profile was renamed (or the rename failed): show the
    /// saved name again.
    pub fn reset_name(&mut self) {
        self.name = None;
    }
}
