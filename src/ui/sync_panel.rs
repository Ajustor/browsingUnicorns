//! The "Sync" view of the side panel.

use application::Target;
use application::sync::RemoteDevice;
use domain::Timestamp;
use egui::{Button, Color32, RichText, ScrollArea, Sense, TextEdit, Vec2};
use egui_phosphor::regular as icon;
use infrastructure::sync::SyncConfig;
use qrcode::{Color, QrCode};

use super::command::Command;
use super::sync::RoundState;

/// What the view shows, borrowed for one frame.
pub struct SyncView<'a> {
    pub config: Option<&'a SyncConfig>,
    pub state: &'a RoundState,
    pub devices: &'a [RemoteDevice],
}

/// What the view keeps between frames.
#[derive(Default)]
pub struct SyncPanel {
    join_code: String,
    show_code: bool,
}

impl SyncPanel {
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        view: SyncView<'_>,
        now: Timestamp,
        commands: &mut Vec<Command>,
    ) {
        match view.config {
            None => self.off(ui, commands),
            Some(config) => self.on(ui, config, &view, now, commands),
        }
    }

    fn off(&mut self, ui: &mut egui::Ui, commands: &mut Vec<Command>) {
        self.show_code = false;
        ui.label(i18n::sync::intro());
        ui.add_space(8.0);
        if ui
            .button(format!(
                "{} {}",
                icon::ARROWS_CLOCKWISE,
                i18n::sync::turn_on()
            ))
            .clicked()
        {
            commands.push(Command::SyncTurnOn);
        }
        ui.add_space(12.0);
        ui.label(i18n::sync::join_prompt());
        ui.add(
            TextEdit::singleline(&mut self.join_code)
                .hint_text("ABCD-EFGH-…")
                .desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(
                !self.join_code.trim().is_empty(),
                Button::new(i18n::sync::join()),
            )
            .clicked()
        {
            commands.push(Command::SyncJoin(std::mem::take(&mut self.join_code)));
        }
    }

    fn on(
        &mut self,
        ui: &mut egui::Ui,
        config: &SyncConfig,
        view: &SyncView<'_>,
        now: Timestamp,
        commands: &mut Vec<Command>,
    ) {
        ui.label(i18n::sync::this_device(&config.device_name));
        ui.horizontal(|ui| {
            match view.state {
                RoundState::Never => ui.weak(i18n::sync::not_synced()),
                RoundState::Running => {
                    ui.spinner();
                    ui.label(i18n::sync::syncing())
                }
                RoundState::Done(at) => ui.weak(i18n::sync::synced(&super::library::ago_secs(
                    at.elapsed().as_secs(),
                ))),
                RoundState::Failed(e) => ui.colored_label(ui.visuals().error_fg_color, e),
            };
        });
        ui.horizontal(|ui| {
            let running = *view.state == RoundState::Running;
            if ui
                .add_enabled(
                    !running,
                    Button::new(format!(
                        "{} {}",
                        icon::ARROWS_CLOCKWISE,
                        i18n::sync::sync_now()
                    )),
                )
                .clicked()
            {
                commands.push(Command::SyncNow);
            }
            if ui
                .button(i18n::sync::turn_off())
                .on_hover_text(i18n::sync::turn_off_tip())
                .clicked()
            {
                commands.push(Command::SyncTurnOff);
            }
        });

        ui.add_space(8.0);
        ui.collapsing(i18n::sync::add_device(), |ui| {
            ui.label(i18n::sync::add_device_how());
            if !self.show_code {
                if ui
                    .button(format!("{} {}", icon::EYE, i18n::sync::show_code()))
                    .clicked()
                {
                    self.show_code = true;
                }
                return;
            }
            qr(ui, &config.key.qr_text());
            ui.add(egui::Label::new(RichText::new(config.key.code()).monospace()).wrap());
            ui.horizontal(|ui| {
                if ui
                    .button(format!("{} {}", icon::COPY, i18n::sync::copy()))
                    .clicked()
                {
                    ui.ctx().copy_text(config.key.code());
                }
                if ui.button(i18n::sync::hide()).clicked() {
                    self.show_code = false;
                }
            });
        });

        ui.add_space(8.0);
        ui.strong(i18n::sync::other_devices());
        if view.devices.is_empty() {
            ui.weak(i18n::sync::no_other_devices());
        }
        ScrollArea::vertical()
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for device in view.devices {
                    ui.collapsing(
                        format!(
                            "{} {}",
                            icon::DEVICES,
                            i18n::sync::device_summary(
                                &device.name,
                                device.tabs.len(),
                                &super::library::ago_secs(now.saturating_sub(device.updated_at)),
                            )
                        ),
                        |ui| {
                            for tab in &device.tabs {
                                let title = if tab.title.is_empty() {
                                    tab.address.as_str()
                                } else {
                                    &tab.title
                                };
                                if super::theme::hover_fill(ui, |ui| {
                                    ui.add(Button::new(title).frame(false).truncate())
                                })
                                .on_hover_text(tab.address.as_str())
                                .clicked()
                                {
                                    commands
                                        .push(Command::Open(tab.address.clone(), Target::NewTab));
                                }
                            }
                        },
                    );
                }
            });
    }
}

/// Draws `text` as a QR code, dark on white whatever the theme (scanners
/// expect it).
fn qr(ui: &mut egui::Ui, text: &str) {
    let Ok(code) = QrCode::new(text.as_bytes()) else {
        return;
    };
    let width = code.width();
    let quiet = 2;
    let cells = width + 2 * quiet;
    let side = (ui.available_width().min(240.0) / cells as f32)
        .floor()
        .max(2.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side * cells as f32), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    for (i, color) in code.to_colors().iter().enumerate() {
        if *color == Color::Dark {
            let (x, y) = ((i % width + quiet) as f32, (i / width + quiet) as f32);
            let min = rect.min + Vec2::new(x * side, y * side);
            painter.rect_filled(
                egui::Rect::from_min_size(min, Vec2::splat(side)),
                0.0,
                Color32::BLACK,
            );
        }
    }
}
