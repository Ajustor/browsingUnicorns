//! The side panel's building blocks, shared by its views: cards, switches,
//! segmented choices, list rows, buttons, the search field.

use egui::{Color32, CornerRadius, Margin, Response, RichText, Sense, Stroke, TextEdit, UiBuilder};
use egui_phosphor::regular as icon;

use super::theme;

/// The violet of the gradient's middle: badges, switches, the choice made.
pub const ACCENT: Color32 = theme::GRADIENT[1];

/// Space between two cards.
const CARD_GAP: f32 = 10.0;

/// The fill and border of a card.
fn card_colors(ui: &egui::Ui) -> (Color32, Color32) {
    if ui.visuals().dark_mode {
        (theme::NIGHT, theme::dark::BORDER)
    } else {
        (Color32::WHITE, theme::light::BORDER)
    }
}

/// The accent as text: lighter on the dark theme, to be read.
pub fn accent_text(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(0xc4, 0xb5, 0xff)
    } else {
        theme::light::ACCENT
    }
}

/// The cyan as text: darker on the light theme.
pub fn cyan_text(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        theme::CYAN
    } else {
        Color32::from_rgb(0x0b, 0x6e, 0x8c)
    }
}

/// Green, for what went well.
pub fn ok_text(ui: &egui::Ui) -> Color32 {
    readable(ui, theme::group_color(domain::GroupColor::Green))
}

/// A light color made dark enough to be read on the light theme.
pub fn readable(ui: &egui::Ui, color: Color32) -> Color32 {
    if ui.visuals().dark_mode {
        color
    } else {
        let [r, g, b, _] = color.to_array();
        Color32::from_rgb(r / 2, g / 2, b / 2)
    }
}

/// A row under the pointer.
pub fn hover_color(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        theme::NIGHT_HOVER
    } else {
        theme::light::HOVER
    }
}

/// A rounded card around `body`, its buttons and menus standing out from
/// it.
pub fn card_frame<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let (fill, border) = card_colors(ui);
    let inner = egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(theme::HAIRLINE, border))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let dark = ui.visuals().dark_mode;
            let widgets = &mut ui.visuals_mut().widgets;
            let (fill, hovered, stroke) = if dark {
                (
                    Color32::from_rgb(0x33, 0x28, 0x5a),
                    Color32::from_rgb(0x3d, 0x30, 0x68),
                    theme::OMNIBOX_STROKE,
                )
            } else {
                (
                    theme::light::HOVER,
                    Color32::from_rgb(0xe4, 0xdc, 0xff),
                    theme::light::FIELD_BORDER,
                )
            };
            widgets.inactive.weak_bg_fill = fill;
            widgets.inactive.bg_stroke = Stroke::new(theme::HAIRLINE, stroke);
            widgets.hovered.weak_bg_fill = hovered;
            // As tall as the text fields.
            ui.spacing_mut().interact_size.y = 28.0;
            ui.spacing_mut().button_padding = egui::vec2(10.0, 5.0);
            body(ui)
        })
        .inner;
    ui.add_space(CARD_GAP);
    inner
}

/// A section: a card with its icon in a violet badge, its title and hint,
/// then its controls.
pub fn card(
    ui: &mut egui::Ui,
    glyph: &str,
    title: &str,
    hint: Option<&str>,
    body: impl FnOnce(&mut egui::Ui),
) {
    card_frame(ui, |ui| {
        card_title(ui, glyph, title, hint);
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing.y = 8.0;
        body(ui);
    });
}

/// A card's head: the badge, the title and the hint under it.
pub fn card_title(ui: &mut egui::Ui, glyph: &str, title: &str, hint: Option<&str>) {
    ui.horizontal(|ui| {
        badge(ui, glyph, 28.0, ACCENT);
        ui.add_space(4.0);
        let title = RichText::new(title).strong().size(15.0);
        match hint {
            None => {
                ui.label(title);
            }
            Some(hint) => {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 1.0;
                    ui.label(title);
                    ui.add(egui::Label::new(RichText::new(hint).weak().size(12.5)).wrap());
                });
            }
        }
    });
}

/// A glyph, white on a rounded square of `fill`.
pub fn badge(ui: &mut egui::Ui, glyph: &str, side: f32, fill: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same((side * 0.28) as u8), fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        egui::FontId::proportional(side * 0.57),
        Color32::WHITE,
    );
    response
}

/// A site's initial on a rounded square of a color its host picks: where
/// there is no icon to show.
pub fn site_badge(ui: &mut egui::Ui, host: &str, side: f32) {
    let host = host.trim_start_matches("www.");
    let colors = &domain::GroupColor::ALL[1..];
    let hash = host
        .bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    let fill = theme::group_color(colors[hash as usize % colors.len()]);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same((side * 0.3) as u8), fill);
    let initial = host
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_else(|| "·".into());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(side * 0.55),
        theme::NIGHT_DEEP,
    );
}

/// A row of a list: `body` laid out across the width, the row lit under
/// the pointer. Its response is for clicks outside the buttons in `body`.
pub fn list_row(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) -> Response {
    let background = ui.painter().add(egui::Shape::Noop);
    let response = ui
        .scope_builder(UiBuilder::new().sense(Sense::click()), |ui| {
            egui::Frame::NONE
                .inner_margin(Margin::symmetric(8, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    // Rows as tall as their badge and two lines of text,
                    // everything centered in them.
                    ui.spacing_mut().interact_size.y = 32.0;
                    body(ui);
                });
        })
        .response;
    if ui.rect_contains_pointer(response.rect) {
        ui.painter().set(
            background,
            egui::Shape::rect_filled(response.rect, CornerRadius::same(8), hover_color(ui)),
        );
    }
    response
}

/// A small heading over a part of a list.
pub fn list_heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(6.0);
    ui.label(
        RichText::new(text.to_uppercase())
            .size(11.0)
            .strong()
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(2.0);
}

/// A button filled with the accent: the main thing to do.
pub fn primary_button(ui: &mut egui::Ui, text: impl Into<String>, enabled: bool) -> Response {
    let text = RichText::new(text.into()).color(Color32::WHITE).strong();
    ui.add_enabled(
        enabled,
        egui::Button::new(text)
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8)),
    )
}

/// A button in red: removes or clears something for good.
pub fn danger_button(ui: &mut egui::Ui, text: impl Into<String>, enabled: bool) -> Response {
    let red = ui.visuals().error_fg_color;
    let text = RichText::new(text.into()).color(red);
    ui.add_enabled(
        enabled,
        egui::Button::new(text)
            .fill(red.gamma_multiply(0.12))
            .stroke(Stroke::new(theme::HAIRLINE, red.gamma_multiply(0.5)))
            .corner_radius(CornerRadius::same(8)),
    )
}

/// An icon button without a frame, lit under the pointer. `glyph` is a
/// plain glyph, or [`RichText`] for its size and colour.
pub fn icon_button(ui: &mut egui::Ui, glyph: impl Into<egui::WidgetText>, tip: &str) -> Response {
    theme::hover_fill(ui, |ui| ui.add(egui::Button::new(glyph).frame(false))).on_hover_text(tip)
}

/// A text field as tall as the buttons, rounded.
pub fn field<'t>(text: &'t mut dyn egui::TextBuffer, hint: &str) -> TextEdit<'t> {
    TextEdit::singleline(text)
        .hint_text(hint)
        .desired_width(f32::INFINITY)
        .margin(Margin::symmetric(8, 6))
}

/// The field that filters a list: a magnifier, the text, a cross to empty
/// it.
pub fn search_field(ui: &mut egui::Ui, text: &mut String, hint: &str) -> Response {
    let (fill, border) = if ui.visuals().dark_mode {
        (theme::dark::FIELD, theme::OMNIBOX_STROKE)
    } else {
        (theme::light::FIELD, theme::light::FIELD_BORDER)
    };
    egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(theme::HAIRLINE, border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(10, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon::MAGNIFYING_GLASS).weak());
                let clear = if text.is_empty() { 0.0 } else { 22.0 };
                let response = ui.add(
                    TextEdit::singleline(text)
                        .hint_text(hint)
                        .frame(false)
                        .desired_width(ui.available_width() - clear),
                );
                if !text.is_empty()
                    && theme::hover_fill(ui, |ui| {
                        ui.add(egui::Button::new(icon::X).frame(false).small())
                    })
                    .clicked()
                {
                    text.clear();
                }
                response
            })
            .inner
        })
        .inner
}

/// A big faint icon and a sentence: a list with nothing in it.
pub fn empty_state(ui: &mut egui::Ui, glyph: &str, text: &str) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(
            RichText::new(glyph)
                .size(40.0)
                .color(ui.visuals().weak_text_color().gamma_multiply(0.7)),
        );
        ui.add_space(6.0);
        ui.add(egui::Label::new(RichText::new(text).weak()).wrap());
    });
    ui.add_space(24.0);
}

/// A small rounded label: a state, a count.
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) -> Response {
    let color = readable(ui, color);
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.5), color);
    let size = galley.size() + egui::vec2(14.0, 6.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(255), color.gamma_multiply(0.16));
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    response
}

/// An on/off switch, drawn alone. Returns whether it was clicked.
pub fn switch(ui: &mut egui::Ui, on: bool) -> Response {
    const SWITCH: egui::Vec2 = egui::vec2(36.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(SWITCH, Sense::click());
    let how_on = ui.ctx().animate_bool(response.id, on);
    let off_fill = if ui.visuals().dark_mode {
        theme::OMNIBOX_STROKE
    } else {
        Color32::from_rgb(0xcf, 0xc6, 0xea)
    };
    let fill = egui::lerp(
        egui::Rgba::from(off_fill)..=egui::Rgba::from(ACCENT),
        how_on,
    );
    let painter = ui.painter();
    painter.rect(
        rect,
        CornerRadius::same(255),
        Color32::from(fill),
        ui.visuals().widgets.inactive.bg_stroke,
        egui::StrokeKind::Inside,
    );
    let knob = egui::pos2(
        egui::lerp(rect.left() + 10.0..=rect.right() - 10.0, how_on),
        rect.center().y,
    );
    painter.circle_filled(knob, 7.0, Color32::WHITE);
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// A setting on or off: its text, a switch at the right (the whole row
/// toggles it). Returns whether it changed.
pub fn switch_row(ui: &mut egui::Ui, text: &str, tip: Option<&str>, on: &mut bool) -> bool {
    let row = ui.horizontal(|ui| {
        let label_width = ui.available_width() - 36.0 - ui.spacing().item_spacing.x;
        let label = ui
            .allocate_ui(egui::vec2(label_width, 0.0), |ui| {
                ui.set_width(label_width);
                ui.add(egui::Label::new(text).wrap().sense(Sense::click()))
            })
            .inner;
        label | switch(ui, *on)
    });
    let response = row.inner;
    let response = match tip {
        Some(tip) => response.on_hover_text(tip),
        None => response,
    };
    if response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        *on = !*on;
        return true;
    }
    false
}

/// Options side by side in one rounded bar, the chosen one in violet.
/// Returns the one clicked, if another.
pub fn segmented(ui: &mut egui::Ui, options: &[(&str, &str)], chosen: usize) -> Option<usize> {
    let mut picked = None;
    egui::Frame::NONE
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            let width =
                (ui.available_width() - 3.0 * (options.len() - 1) as f32) / options.len() as f32;
            ui.horizontal(|ui| {
                for (i, (glyph, text)) in options.iter().enumerate() {
                    let label = if text.is_empty() {
                        (*glyph).to_owned()
                    } else {
                        format!("{glyph} {text}")
                    };
                    if segment(ui, width, &label, i == chosen).clicked() && i != chosen {
                        picked = Some(i);
                    }
                }
            });
        });
    picked
}

/// One option of a segmented bar.
fn segment(ui: &mut egui::Ui, width: f32, label: &str, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 28.0), Sense::click());
    let fill = if selected {
        ACCENT
    } else if response.hovered() {
        ui.visuals().widgets.hovered.weak_bg_fill
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
    let color = if selected {
        Color32::WHITE
    } else {
        ui.visuals().text_color()
    };
    let galley = ui.painter().layout(
        label.to_owned(),
        egui::FontId::proportional(13.5),
        color,
        width - 8.0,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, color);
    if response.hovered() && !selected {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// The panel's views as icons in one bar, each named in its tooltip.
/// Returns the one clicked, if another.
pub fn icon_tabs(ui: &mut egui::Ui, tabs: &[(&str, &str, bool)], chosen: usize) -> Option<usize> {
    let mut picked = None;
    egui::Frame::NONE
        .fill(ui.visuals().extreme_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let width = (ui.available_width() - 2.0 * (tabs.len() - 1) as f32) / tabs.len() as f32;
            ui.horizontal(|ui| {
                for (i, (glyph, name, dot)) in tabs.iter().enumerate() {
                    let response = segment(ui, width, glyph, i == chosen).on_hover_text(*name);
                    if *dot {
                        let at = response.rect.right_top() + egui::vec2(-7.0, 7.0);
                        ui.painter().circle_filled(at, 3.5, theme::CYAN);
                    }
                    if response.clicked() && i != chosen {
                        picked = Some(i);
                    }
                }
            });
        });
    picked
}
