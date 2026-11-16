# Desktop « unicorn » theme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the desktop egui chrome the theme of `docs/superpowers/specs/2026-11-16-desktop-theme-design.md`: gradient tab strip, night toolbar and bookmarks bar, side panel and banners following the system's light/dark theme, the system's UI font.

**Architecture:** Two new modules in the binary crate: `src/ui/system_font.rs` (finds the OS UI font file) and `src/ui/theme.rs` (palette constants, contrast helper, `install(ctx)` setting fonts and the dark/light `Visuals`, drawing helpers: gradient, chrome frame). The chrome files (`tab_strip.rs`, `toolbar.rs`, `omnibox.rs`, `bookmarks_bar.rs`) draw with the always-dark chrome palette inside frames from `theme`; panels and banners use egui's current `Visuals`, which `install` sets per theme.

**Tech Stack:** Rust 2024, egui/eframe 0.31 (`CornerRadius`, `Frame::NONE`, `Context::set_visuals_of`, `ThemePreference::System`), epaint `Mesh` for the gradient. No new dependency.

**Conventions (read before starting):**
- Shell: Windows, PowerShell. `cargo` lives in PowerShell's PATH (the Bash tool lacks `sccache`): run cargo commands with the PowerShell tool.
- Commits: the user is the only author. **No `Co-Authored-By` trailer**, conventional subject (`feat(ui): …`), short English body. One commit per task. Do not push.
- Comments and doc comments: short English sentences, like the existing code.
- After every task: `cargo fmt`, `cargo build`, `cargo clippy --all-targets` (one known warning in `crates/infrastructure/src/user_scripts.rs:160` is pre-existing, ignore it), `cargo test -q --bin browsingUnicorns`.

---

## File structure

| File | Responsibility |
|---|---|
| `src/ui/system_font.rs` (new) | Path candidates of the OS UI font, reading the first that exists |
| `src/ui/theme.rs` (new) | Palette, WCAG contrast, `install(ctx)`, `gradient(painter, rect)`, `chrome_frame()`, `CHROME_*` text colors |
| `src/ui/mod.rs` | Declares the modules; `install_fonts` replaced by `theme::install` |
| `src/bootstrap.rs:34` | Calls `crate::ui::theme::install` |
| `src/ui/app.rs` | Top panels use theme frames; tab strip panel paints the gradient |
| `src/ui/tab_strip.rs` | Tabs drawn by hand: inactive on the gradient, active attached night tab |
| `src/ui/toolbar.rs`, `src/ui/omnibox.rs`, `src/ui/bookmarks_bar.rs` | Chrome colors (lavender icons, cyan accents, pill address bar) |
| `src/ui/library.rs` | Section selector as pills (active one in the gradient) |

---

### Task 1: System UI font

**Files:**
- Create: `src/ui/system_font.rs`
- Modify: `src/ui/mod.rs` (add `mod system_font;`)

- [ ] **Step 1: Write the module with its failing tests**

```rust
//! The operating system's interface font, read from its usual place: the
//! browser then looks like the system's other apps.

use std::path::{Path, PathBuf};

/// Where the system's UI font usually is, best first.
pub fn candidates() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        let windir = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from(r"C:\Windows"), PathBuf::from);
        vec![windir.join("Fonts").join("segoeui.ttf")]
    }
    #[cfg(target_os = "macos")]
    {
        vec![
            PathBuf::from("/System/Library/Fonts/SFNS.ttf"),
            PathBuf::from("/System/Library/Fonts/Helvetica.ttc"),
        ]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // fontconfig's choice for the default sans-serif.
        std::process::Command::new("fc-match")
            .args(["-f", "%{file}", "sans-serif"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|p| vec![PathBuf::from(p.trim())])
            .unwrap_or_default()
    }
}

/// The bytes of the first font of `paths` that can be read.
pub fn first_readable(paths: &[PathBuf]) -> Option<(PathBuf, Vec<u8>)> {
    paths.iter().find_map(|p| read(p).map(|bytes| (p.clone(), bytes)))
}

fn read(path: &Path) -> Option<Vec<u8>> {
    std::fs::read(path).ok().filter(|b| !b.is_empty())
}

/// The system's UI font, if found.
pub fn load() -> Option<(PathBuf, Vec<u8>)> {
    first_readable(&candidates())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_first_readable_font() {
        let dir = std::env::temp_dir().join(format!("bu-font-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let empty = dir.join("empty.ttf");
        let font = dir.join("font.ttf");
        std::fs::write(&empty, b"").unwrap();
        std::fs::write(&font, b"font bytes").unwrap();
        let missing = dir.join("missing.ttf");

        let (path, bytes) = first_readable(&[missing.clone(), empty, font.clone()]).unwrap();
        assert_eq!((path, bytes), (font, b"font bytes".to_vec()));
        assert!(first_readable(&[missing]).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn segoe_ui_on_windows() {
        assert!(candidates()[0].ends_with("Fonts\\segoeui.ttf"));
    }
}
```

Add `mod system_font;` to `src/ui/mod.rs` (alphabetical order, after `mod sync_panel;`).

- [ ] **Step 2: Run the tests**

Run (PowerShell): `cargo test -q --bin browsingUnicorns system_font`
Expected: 2 passed on Windows (an unused-function warning for `load` is expected until Task 2).

- [ ] **Step 3: Commit**

```bash
git add src/ui/system_font.rs src/ui/mod.rs
git commit -m "feat(ui): find the system's interface font"
```

---

### Task 2: Theme module: palette, contrast, visuals, fonts

**Files:**
- Create: `src/ui/theme.rs`
- Modify: `src/ui/mod.rs` (add `pub mod theme;`, delete `install_fonts`), `src/bootstrap.rs:34`

- [ ] **Step 1: Write the palette and the contrast tests first**

```rust
//! The browser's look: the icon's night purple and cyan. The chrome (tab
//! strip, toolbar, bookmarks bar) is always dark; panels and banners follow
//! the system's light or dark theme.

use egui::epaint::{Mesh, Vertex, WHITE_UV};
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Frame, Margin, Painter, Rect, Stroke, Theme, ThemePreference, Visuals};

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// The tab strip's gradient, left to right.
pub const GRADIENT: [Color32; 3] = [rgb(0x3a246e), rgb(0x5b3fa6), rgb(0x2d8fb3)];
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
fn luminance(c: Color32) -> f32 {
    let channel = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(c.r()) + 0.7152 * channel(c.g()) + 0.0722 * channel(c.b())
}

/// WCAG contrast ratio between two opaque colors (1 to 21).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
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
        assert!(contrast(TAB_TEXT, lightest) >= 3.0, "{}", contrast(TAB_TEXT, lightest));
    }
}
```

Note: `TAB_TEXT` is premultiplied white at 90 % over nothing; for the contrast test it reads as `rgb(230, 230, 230)`, which is the blended color over a dark background — a conservative stand-in. If `text_is_readable_everywhere` fails **only** on the last assertion, darken `GRADIENT[2]` step by step (e.g. `0x2a86a8`, `0x277d9d`) until it passes, and report the final value.

Add `pub mod theme;` to `src/ui/mod.rs`.

- [ ] **Step 2: Run the tests**

Run: `cargo test -q --bin browsingUnicorns theme`
Expected: 2 passed (or adjust `GRADIENT[2]` as described, then pass).

- [ ] **Step 3: Add `install` (fonts + visuals) and the drawing helpers to `theme.rs`**

```rust
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
            fonts
                .font_data
                .insert("system".into(), std::sync::Arc::new(FontData::from_owned(bytes)));
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
        Theme::Dark => (Visuals::dark(), dark::PANEL, dark::HOVER, dark::BORDER, dark::FIELD, dark::TEXT, CYAN),
        Theme::Light => (Visuals::light(), light::PANEL, light::HOVER, light::BORDER, light::FIELD, light::TEXT, light::ACCENT),
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
        mesh.vertices.push(Vertex { pos: egui::pos2(x, rect.top()), uv: WHITE_UV, color: *color });
        mesh.vertices.push(Vertex { pos: egui::pos2(x, rect.bottom()), uv: WHITE_UV, color: *color });
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
    Frame::NONE.fill(NIGHT).inner_margin(Margin::symmetric(8, 5))
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
```

If a field name does not exist in egui 0.31 (`window_corner_radius`, `menu_corner_radius`), look it up in `~/.cargo/registry/src/*/egui-0.31*/src/style.rs` (`pub struct Visuals`) and use the 0.31 name; do not drop the setting.

- [ ] **Step 4: Wire it in**

In `src/ui/mod.rs`, delete `pub fn install_fonts` (the whole function). In `src/bootstrap.rs:34` replace `crate::ui::install_fonts(&cc.egui_ctx);` with `crate::ui::theme::install(&cc.egui_ctx);`.

- [ ] **Step 5: Build, clippy, tests**

Run: `cargo fmt; cargo build; cargo clippy --all-targets; cargo test -q --bin browsingUnicorns`
Expected: builds; only the pre-existing clippy warning; tests pass. Unused-function warnings for `gradient`, `chrome_frame`, `chrome_style` are acceptable until Task 3/4 — mark them `#[allow(dead_code)]` **only** if clippy fails the build on them, and remove the allow in Task 3.

- [ ] **Step 6: Commit**

```bash
git add src/ui/theme.rs src/ui/mod.rs src/bootstrap.rs
git commit -m "feat(ui): unicorn palette, system font and themed visuals"
```

---

### Task 3: Tab strip on the gradient

**Files:**
- Modify: `src/ui/app.rs` (the `TopBottomPanel::top("tabs")` call, around line 469)
- Modify: `src/ui/tab_strip.rs` (`show`)

- [ ] **Step 1: Panel without its own fill, gradient painted behind**

In `src/ui/app.rs`, replace

```rust
        TopBottomPanel::top("tabs").show(ctx, |ui| {
            tab_strip::show(ui, self.browser.session(), &mut commands);
        });
```

with

```rust
        TopBottomPanel::top("tabs")
            .frame(egui::Frame::NONE.inner_margin(egui::Margin { left: 8, right: 8, top: 6, bottom: 0 }))
            .show_separator_line(false)
            .show(ctx, |ui| {
                super::theme::gradient(ui.painter(), ui.max_rect().expand2(egui::vec2(8.0, 6.0)));
                tab_strip::show(ui, self.browser.session(), &mut commands);
            });
```

(`max_rect` is the inner rect; `expand2` covers the margins. If the gradient leaves a gap at the panel's edges, paint `ui.ctx().screen_rect()`'s width instead: `egui::Rect::from_x_y_ranges(ctx.screen_rect().x_range(), ui.max_rect().expand2(egui::vec2(0.0, 6.0)).y_range())`.)

- [ ] **Step 2: Draw the tabs by hand in `tab_strip.rs`**

Replace the body of `show` (keep `truncate` and its test) with:

```rust
pub fn show(ui: &mut egui::Ui, session: &Session, commands: &mut Vec<Command>) {
    use super::theme;

    ui.spacing_mut().item_spacing.x = 3.0;
    ui.horizontal(|ui| {
        // As wide as the tabs: the + follows the last one (room kept for it
        // when they overflow).
        ScrollArea::horizontal()
            .auto_shrink([true, true])
            .max_width(ui.available_width() - 32.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for tab in session.tabs() {
                        let active = session.active_id() == Some(tab.id());
                        tab_button(ui, tab, active, commands);
                    }
                });
            });
        let plus = ui
            .add(Button::new(RichText::new(icon::PLUS).color(Color32::WHITE).size(16.0)).frame(false))
            .on_hover_text("New tab (Ctrl+T)");
        if plus.clicked() {
            commands.push(Command::NewTab);
        }
    });

    fn tab_button(ui: &mut egui::Ui, tab: &domain::Tab, active: bool, commands: &mut Vec<Command>) {
        let (fill, text) = if active { (theme::NIGHT, Color32::WHITE) } else { (Color32::TRANSPARENT, theme::TAB_TEXT) };
        let frame = egui::Frame::NONE
            .fill(fill)
            .corner_radius(egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 })
            .inner_margin(egui::Margin { left: 12, right: 8, top: 7, bottom: 7 });
        let response = frame
            .show(ui, |ui| {
                ui.set_min_width(110.0);
                ui.set_max_width(190.0);
                ui.horizontal(|ui| {
                    if tab.is_loading() {
                        ui.add(Spinner::new().size(12.0).color(text));
                    }
                    let title = truncate(tab.display_title(), TITLE_CHARS);
                    let label = ui
                        .add(egui::Label::new(RichText::new(title).color(text)).selectable(false).sense(Sense::click()))
                        .on_hover_text(tab.display_title());
                    if label.clicked() {
                        commands.push(Command::Activate(tab.id()));
                    }
                    if label.middle_clicked() {
                        commands.push(Command::CloseTab(tab.id()));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = ui
                            .add(Button::new(RichText::new(icon::X).color(text.gamma_multiply(0.6)).small()).frame(false))
                            .on_hover_text("Close tab (Ctrl+W)");
                        if close.clicked() {
                            commands.push(Command::CloseTab(tab.id()));
                        }
                    });
                });
            })
            .response;
        let response = response.interact(Sense::click());
        if !active && response.hovered() {
            ui.painter().rect_filled(
                response.rect,
                egui::CornerRadius { nw: 10, ne: 10, sw: 0, se: 0 },
                Color32::from_white_alpha(28),
            );
        }
        if response.clicked() {
            commands.push(Command::Activate(tab.id()));
        }
        if response.middle_clicked() {
            commands.push(Command::CloseTab(tab.id()));
        }
    }
}
```

Update the imports at the top of `tab_strip.rs` to:

```rust
use egui::{Button, Color32, RichText, ScrollArea, Sense, Spinner};
use egui_phosphor::regular as icon;

use domain::Session;

use super::command::Command;
```

Check that `domain::Tab` is the tab type returned by `session.tabs()` (it is used as `&Tab` in `toolbar.rs`). Remove `#[allow(dead_code)]` on `gradient` if Task 2 added it.

- [ ] **Step 3: Build, clippy, tests**

Run: `cargo fmt; cargo build; cargo clippy --all-targets; cargo test -q --bin browsingUnicorns`
Expected: clean (pre-existing warning only), tests pass (including `long_titles_are_cut`).

- [ ] **Step 4: Commit**

```bash
git add src/ui/app.rs src/ui/tab_strip.rs
git commit -m "feat(ui): tabs on the unicorn gradient, active tab attached to the toolbar"
```

---

### Task 4: Night toolbar, pill address bar, bookmarks bar

**Files:**
- Modify: `src/ui/app.rs` (panels `toolbar` and `bookmarks`, around lines 477 and 490)
- Modify: `src/ui/toolbar.rs`, `src/ui/omnibox.rs`, `src/ui/bookmarks_bar.rs`

- [ ] **Step 1: Chrome frames for the two panels**

In `src/ui/app.rs`, change `TopBottomPanel::top("toolbar")` to

```rust
        TopBottomPanel::top("toolbar")
            .frame(super::theme::chrome_frame())
            .show_separator_line(false)
            .show(ctx, |ui| {
                super::theme::chrome_style(ui);
                toolbar::show(
```

(keep the existing arguments and closing), and `TopBottomPanel::top("bookmarks")` to

```rust
            TopBottomPanel::top("bookmarks")
                .frame(super::theme::chrome_frame().inner_margin(egui::Margin::symmetric(10, 3)))
                .show_separator_line(false)
                .show(ctx, |ui| {
                    super::theme::chrome_style(ui);
                    bookmarks_bar::show(ui, self.browser.bookmarks(), &mut commands);
                    let r = ui.max_rect();
                    ui.painter().hline(
                        r.x_range(),
                        r.bottom() + 3.0,
                        egui::Stroke::new(1.0, super::theme::NIGHT_HOVER),
                    );
                });
```

- [ ] **Step 2: Cyan accents in the toolbar**

In `src/ui/toolbar.rs`:
- the lock glyph button: when `a.is_secure()`, use `RichText::new(icon::LOCK_SIMPLE).color(super::theme::CYAN)` as the button text;
- the bookmarked star: replace `.color(ui.visuals().warn_fg_color)` with `.color(super::theme::CYAN)`;
- make toolbar icons a bit larger: in `fn button`, wrap the glyph as `RichText` with `.size(16.0)` when it is a plain `&str` — simplest: change the `nav` loop and the right-side calls to pass `RichText::new(glyph).size(16.0)`; `button` already takes `impl Into<egui::WidgetText>`.

- [ ] **Step 3: Pill address bar**

In `src/ui/omnibox.rs`, replace the `TextEdit` construction with:

```rust
        let response = ui.add(
            TextEdit::singleline(&mut self.text)
                .id(id)
                .hint_text("Search or enter an address")
                .desired_width(f32::INFINITY)
                .margin(egui::Margin::symmetric(12, 6))
                .frame(true),
        );
```

and, before `ui.add`, give the field a pill shape for this widget only:

```rust
        ui.style_mut().visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(16);
        ui.style_mut().visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(16);
        ui.style_mut().visuals.widgets.active.corner_radius = egui::CornerRadius::same(16);
```

(The omnibox is the last widget of the toolbar's right-to-left layout, so changing the `ui` style here affects nothing after it.)

- [ ] **Step 4: Bookmarks bar text**

In `src/ui/bookmarks_bar.rs`, build each label as `RichText::new(label).color(super::theme::CHROME_TEXT_WEAK)` (import `egui::RichText`).

- [ ] **Step 5: Build, clippy, tests, commit**

Run: `cargo fmt; cargo build; cargo clippy --all-targets; cargo test -q --bin browsingUnicorns`

```bash
git add src/ui/app.rs src/ui/toolbar.rs src/ui/omnibox.rs src/ui/bookmarks_bar.rs
git commit -m "feat(ui): night toolbar and bookmarks bar, pill address bar, cyan accents"
```

---

### Task 5: Side panel section pills

**Files:**
- Modify: `src/ui/library.rs` (the section loop in `show`, around lines 90-115)
- Modify: `src/ui/theme.rs` (add `pill`)

- [ ] **Step 1: Add `pill` to `theme.rs`**

```rust
/// A section pill of the side panel; the selected one shows the gradient.
pub fn pill(ui: &mut egui::Ui, selected: bool, text: impl Into<String>) -> egui::Response {
    let text = text.into();
    let galley_color = if selected { Color32::WHITE } else { ui.visuals().text_color() };
    let button = egui::Button::new(egui::RichText::new(text).color(galley_color))
        .corner_radius(CornerRadius::same(255))
        .fill(if selected { Color32::TRANSPARENT } else { ui.visuals().faint_bg_color })
        .stroke(Stroke::NONE);
    let response = ui.add(button);
    if selected {
        // Under the label: the gradient, clipped to the pill by painting it
        // first on a layer below.
        let painter = ui.painter().clone().with_layer_id(egui::LayerId::new(
            egui::Order::Background,
            ui.id().with("pill"),
        ));
        let mut mesh_painter = painter.clone();
        mesh_painter.set_clip_rect(response.rect);
        gradient(&mesh_painter, response.rect);
    }
    response
}
```

If the background-layer gradient does not show under the button (egui draws the panel over the `Background` order in some setups), paint it with `ui.painter()` **before** adding the button instead: allocate the button's size with `ui.add_sized`-free approach — simplest fallback: for the selected pill use `.fill(GRADIENT[1])` (plain violet) and keep the gradient only on the tab strip. Report which variant you kept.

- [ ] **Step 2: Use it in `library.rs`**

Replace

```rust
                                if ui.selectable_label(view == v, label).clicked() && view != v {
```

with

```rust
                                if super::theme::pill(ui, view == v, label).clicked() && view != v {
```

- [ ] **Step 3: Build, clippy, tests, commit**

Run: `cargo fmt; cargo build; cargo clippy --all-targets; cargo test -q --bin browsingUnicorns`

```bash
git add src/ui/theme.rs src/ui/library.rs
git commit -m "feat(ui): side panel sections as pills"
```

---

### Task 6: Look at it

**Files:** none (verification); fixes go in the task they belong to, as new commits.

- [ ] **Step 1: Run the app on an isolated profile**

PowerShell: `$env:BU_PROFILE_DIR = "$env:TEMP\bu-theme-check"; cargo run`
(Never run it on the user's real profile; the installed browser may be open.)

- [ ] **Step 2: Check, in dark and light Windows modes**
- gradient spans the whole tab strip, no gap at the edges;
- active tab touches the toolbar (no line between them), inactive titles readable;
- `+` right after the last tab;
- toolbar, address bar, bookmarks bar night; lock and active star cyan;
- side panel (Ctrl+H): night in dark mode, lavender in light mode; selected section highlighted;
- notice bar (download one file) and update banner readable in both modes.

- [ ] **Step 3: Report**

Do not drive the GUI with simulated keys or clicks on the user's desktop. Report what could not be checked; the user validates the look.
