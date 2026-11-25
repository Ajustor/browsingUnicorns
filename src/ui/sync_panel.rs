//! The "Sync" view of the side panel.

use application::Target;
use application::sync::RemoteDevice;
use domain::Timestamp;
use egui::{Button, Color32, RichText, Sense, Vec2};
use egui_phosphor::regular as icon;
use infrastructure::sync::SyncConfig;
use qrcode::{Color, QrCode};

use super::command::Command;
use super::panel_kit::{self, card, field, list_row, primary_button, site_badge};
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
        card(
            ui,
            icon::ARROWS_CLOCKWISE,
            i18n::sync::title(),
            Some(i18n::sync::intro()),
            |ui| {
                if primary_button(
                    ui,
                    format!("{} {}", icon::POWER, i18n::sync::turn_on()),
                    true,
                )
                .clicked()
                {
                    commands.push(Command::SyncTurnOn);
                }
            },
        );
        card(
            ui,
            icon::LINK,
            i18n::sync::join_title(),
            Some(i18n::sync::join_prompt()),
            |ui| {
                let field = ui.add(field(&mut self.join_code, "ABCD-EFGH-…"));
                let submitted = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let can = !self.join_code.trim().is_empty();
                if (ui
                    .add_enabled(
                        can,
                        Button::new(format!("{} {}", icon::SIGN_IN, i18n::sync::join())),
                    )
                    .clicked()
                    || submitted)
                    && can
                {
                    commands.push(Command::SyncJoin(std::mem::take(&mut self.join_code)));
                }
            },
        );
    }

    fn on(
        &mut self,
        ui: &mut egui::Ui,
        config: &SyncConfig,
        view: &SyncView<'_>,
        now: Timestamp,
        commands: &mut Vec<Command>,
    ) {
        let this_device = i18n::sync::this_device(&config.device_name);
        card(
            ui,
            icon::ARROWS_CLOCKWISE,
            i18n::sync::title(),
            Some(&this_device),
            |ui| {
                ui.horizontal(|ui| {
                    match view.state {
                        RoundState::Never => {
                            ui.label(RichText::new(icon::CIRCLE_DASHED).size(16.0).weak());
                            ui.weak(i18n::sync::not_synced());
                        }
                        RoundState::Running => {
                            ui.spinner();
                            ui.label(i18n::sync::syncing());
                        }
                        RoundState::Done(at) => {
                            ui.label(
                                RichText::new(icon::CHECK_CIRCLE)
                                    .size(16.0)
                                    .color(panel_kit::ok_text(ui)),
                            );
                            ui.label(i18n::sync::synced(&super::library::ago_secs(
                                at.elapsed().as_secs(),
                            )));
                        }
                        RoundState::Failed(e) => {
                            let red = ui.visuals().error_fg_color;
                            ui.label(RichText::new(icon::WARNING).size(16.0).color(red));
                            ui.add(egui::Label::new(RichText::new(e).color(red)).wrap());
                        }
                    };
                });
                ui.horizontal(|ui| {
                    let running = *view.state == RoundState::Running;
                    if primary_button(
                        ui,
                        format!("{} {}", icon::ARROWS_CLOCKWISE, i18n::sync::sync_now()),
                        !running,
                    )
                    .clicked()
                    {
                        commands.push(Command::SyncNow);
                    }
                    if ui
                        .button(format!("{} {}", icon::POWER, i18n::sync::turn_off()))
                        .on_hover_text(i18n::sync::turn_off_tip())
                        .clicked()
                    {
                        commands.push(Command::SyncTurnOff);
                    }
                });
            },
        );

        card(
            ui,
            icon::QR_CODE,
            i18n::sync::add_device(),
            Some(i18n::sync::add_device_how()),
            |ui| {
                if !self.show_code {
                    if ui
                        .button(format!("{} {}", icon::EYE, i18n::sync::show_code()))
                        .clicked()
                    {
                        self.show_code = true;
                    }
                    return;
                }
                ui.vertical_centered(|ui| qr(ui, &config.key.qr_text()));
                egui::Frame::NONE
                    .fill(ui.visuals().extreme_bg_color)
                    .corner_radius(egui::CornerRadius::same(8))
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(
                            egui::Label::new(RichText::new(config.key.code()).monospace()).wrap(),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui
                        .button(format!("{} {}", icon::COPY, i18n::sync::copy()))
                        .clicked()
                    {
                        ui.ctx().copy_text(config.key.code());
                    }
                    if ui
                        .button(format!("{} {}", icon::EYE_SLASH, i18n::sync::hide()))
                        .clicked()
                    {
                        self.show_code = false;
                    }
                });
            },
        );

        card(ui, icon::DEVICES, i18n::sync::other_devices(), None, |ui| {
            if view.devices.is_empty() {
                ui.weak(i18n::sync::no_other_devices());
            }
            for device in view.devices {
                let id = ui.make_persistent_id(("synced device", &device.name));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    ui.label(RichText::new(icon::DEVICES).color(panel_kit::accent_text(ui)));
                    ui.add(
                        egui::Label::new(
                            RichText::new(i18n::sync::device_summary(
                                &device.name,
                                device.tabs.len(),
                                &super::library::ago_secs(now.saturating_sub(device.updated_at)),
                            ))
                            .strong(),
                        )
                        .truncate(),
                    );
                })
                .body(|ui| {
                    for tab in &device.tabs {
                        let title = if tab.title.is_empty() {
                            tab.address.as_str()
                        } else {
                            &tab.title
                        };
                        let response = list_row(ui, |ui| {
                            ui.horizontal(|ui| {
                                site_badge(ui, tab.address.host().unwrap_or("?"), 20.0);
                                ui.add(egui::Label::new(title).truncate());
                            });
                        })
                        .on_hover_text(tab.address.as_str());
                        if response.clicked() {
                            commands.push(Command::Open(tab.address.clone(), Target::NewTab));
                        }
                    }
                });
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
