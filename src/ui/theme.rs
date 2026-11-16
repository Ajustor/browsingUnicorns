//! The browser's look: the icon's night purple and cyan. The chrome (tab
//! strip, toolbar, bookmarks bar) is always dark; panels and banners follow
//! the system's light or dark theme.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Frame, Margin, Painter, Rect,
    Stroke, Theme, ThemePreference, Visuals,
};

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The tab strip's gradient, left to right. Its cyan end is a little darker
/// than the icon's (#2d8fb3) so that inactive tab titles stay readable.
pub const GRADIENT: [Color32; 3] = [rgb(0x3a246e), rgb(0x5b3fa6), rgb(0x2a86a8)];
/// Active tab, toolbar, bookmarks bar.
pub const NIGHT: Color32 = rgb(0x241a3d);
/// The address bar's background, and its outline.
pub const NIGHT_DEEP: Color32 = rgb(0x171126);
pub const OMNIBOX_STROKE: Color32 = rgb(0x3f3270);
/// Hovered chrome buttons; the bookmarks bar's bottom line.
pub const NIGHT_HOVER: Color32 = rgb(0x3a2c63);
/// Text and icons on the chrome.
pub const CHROME_TEXT: Color32 = rgb(0xe9e4ff);
pub const CHROME_TEXT_WEAK: Color32 = rgb(0xb9b0d8);
/// Text of inactive tabs, on the gradient (white at 90 %).
pub const TAB_TEXT: Color32 = Color32::from_rgba_premultiplied(230, 230, 230, 230);
/// Lock, active star, focus.
pub const CYAN: Color32 = rgb(0x5adcff);

/// Side panel and banners, dark theme.
pub mod dark {
    use super::{Color32, rgb};
    pub const PANEL: Color32 = rgb(0x1c1430);
    pub const HOVER: Color32 = rgb(0x2a1f47);
    pub const BORDER: Color32 = rgb(0x2f2350);
    pub const FIELD: Color32 = rgb(0x140e24);
    pub const TEXT: Color32 = rgb(0xece7ff);
}

/// Side panel and banners, light theme.
pub mod light {
    use super::{Color32, rgb};
    pub const PANEL: Color32 = rgb(0xfaf8ff);
    pub const HOVER: Color32 = rgb(0xefeaff);
    pub const BORDER: Color32 = rgb(0xe6e0f5);
    pub const FIELD: Color32 = Color32::WHITE;
    pub const FIELD_BORDER: Color32 = rgb(0xe0d9f3);
    pub const TEXT: Color32 = rgb(0x241b3d);
    pub const ACCENT: Color32 = rgb(0x5b3fa6);
}

/// Relative luminance (WCAG 2), alpha ignored.
#[cfg(test)]
fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG contrast ratio between two opaque colors (1 to 21).
#[cfg(test)]
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Fonts and both themes' visuals; egui follows the system's theme.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.set_visuals_of(Theme::Dark, visuals(Theme::Dark));
    ctx.set_visuals_of(Theme::Light, visuals(Theme::Light));
    ctx.set_theme(ThemePreference::System);
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    match super::system_font::load() {
        Some((path, bytes)) => {
            log::info!("font: {}", path.display());
            fonts.font_data.insert(
                "system".into(),
                std::sync::Arc::new(FontData::from_owned(bytes)),
            );
            // First choice; egui's own font stays as a fallback for missing glyphs.
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, "system".into());
        }
        None => log::warn!("font: no system interface font, egui's own is used"),
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    fonts
}

fn visuals(theme: Theme) -> Visuals {
    let (mut v, panel, hover, border, field, text, accent) = match theme {
        Theme::Dark => (
            Visuals::dark(),
            dark::PANEL,
            dark::HOVER,
            dark::BORDER,
            dark::FIELD,
            dark::TEXT,
            CYAN,
        ),
        Theme::Light => (
            Visuals::light(),
            light::PANEL,
            light::HOVER,
            light::BORDER,
            light::FIELD,
            light::TEXT,
            light::ACCENT,
        ),
    };
    let radius = CornerRadius::same(8);
    v.panel_fill = panel;
    v.window_fill = panel;
    v.extreme_bg_color = field;
    v.faint_bg_color = hover;
    v.window_stroke = Stroke::new(1.0, border);
    v.override_text_color = Some(text);
    v.hyperlink_color = accent;
    v.selection.bg_fill = accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, accent);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = radius;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.inactive.weak_bg_fill = hover;
    v.widgets.inactive.bg_fill = hover;
    v.widgets.hovered.weak_bg_fill = hover;
    v.widgets.hovered.bg_fill = hover;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, accent.gamma_multiply(0.6));
    v.widgets.active.weak_bg_fill = accent.gamma_multiply(0.5);
    v.widgets.active.bg_fill = accent.gamma_multiply(0.5);
    if theme == Theme::Light {
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, light::FIELD_BORDER);
    }
    v
}

/// Paints the tab strip's gradient over `rect` (left to right).
pub fn gradient(painter: &Painter, rect: Rect) {
    let mut mesh = Mesh::default();
    let stops = GRADIENT.len() - 1;
    for (i, color) in GRADIENT.iter().enumerate() {
        let x = egui::lerp(rect.left()..=rect.right(), i as f32 / stops as f32);
        mesh.vertices.push(Vertex {
            pos: egui::pos2(x, rect.top()),
            uv: WHITE_UV,
            color: *color,
        });
        mesh.vertices.push(Vertex {
            pos: egui::pos2(x, rect.bottom()),
            uv: WHITE_UV,
            color: *color,
        });
    }
    for i in 0..stops as u32 {
        let (tl, bl, tr, br) = (2 * i, 2 * i + 1, 2 * i + 2, 2 * i + 3);
        mesh.add_triangle(tl, bl, tr);
        mesh.add_triangle(tr, bl, br);
    }
    painter.add(mesh);
}

/// The dark frame of the toolbar and the bookmarks bar.
pub fn chrome_frame() -> Frame {
    Frame::NONE
        .fill(NIGHT)
        .inner_margin(Margin::symmetric(8, 5))
}

/// Chrome widgets (toolbar, tab strip, bookmarks bar): always dark, lavender
/// text, rounded hover, cyan focus. Call at the start of their `ui`.
pub fn chrome_style(ui: &mut egui::Ui) {
    let v = ui.visuals_mut();
    *v = visuals(Theme::Dark);
    v.override_text_color = Some(CHROME_TEXT);
    v.panel_fill = NIGHT;
    v.extreme_bg_color = NIGHT_DEEP;
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = NIGHT_DEEP;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, OMNIBOX_STROKE);
    v.widgets.hovered.weak_bg_fill = NIGHT_HOVER;
    v.widgets.hovered.bg_fill = NIGHT_DEEP;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, OMNIBOX_STROKE);
    v.widgets.active.bg_stroke = Stroke::new(1.0, CYAN);
    v.selection.stroke = Stroke::new(1.0, CYAN);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, CHROME_TEXT_WEAK);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contrast_of_known_pairs() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        assert!((contrast(CYAN, CYAN) - 1.0).abs() < 0.01);
    }

    #[test]
    fn text_is_readable_everywhere() {
        assert!(contrast(CHROME_TEXT, NIGHT) >= 4.5);
        assert!(contrast(CHROME_TEXT, NIGHT_DEEP) >= 4.5);
        assert!(contrast(dark::TEXT, dark::PANEL) >= 4.5);
        assert!(contrast(light::TEXT, light::PANEL) >= 4.5);
        assert!(contrast(Color32::WHITE, light::ACCENT) >= 4.5);
        assert!(contrast(NIGHT, CYAN) >= 4.5);
        // Inactive tab titles on the gradient's lightest end.
        let lightest = GRADIENT[2];
        assert!(
            contrast(TAB_TEXT, lightest) >= 3.0,
            "{}",
            contrast(TAB_TEXT, lightest)
        );
    }
}
