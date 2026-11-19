//! What the pages download: the engine saves it in the user's Downloads
//! folder; the downloads bar, at the bottom of the window (the page's
//! native view would hide a popup), shows each one with how far it is, and
//! opens it or its folder once done.

use std::path::{Path, PathBuf};
use std::process::Command as Process;
use std::time::Duration;

use application::ports::{DownloadProgress, EngineEvent};
use domain::WebAddress;
use egui::{CornerRadius, Frame, Margin, RichText, Sense, Stroke, TopBottomPanel};
use egui_phosphor::regular as icon;

use application::Target;

use super::command::Command;

const CARD_WIDTH: f32 = 290.0;

/// The downloads of this window, newest first.
#[derive(Default)]
pub struct Downloads {
    items: Vec<Item>,
    /// Whether the bar shows (a new download shows it).
    pub shown: bool,
    /// The cards' height at the last frame: the buttons beside them are
    /// laid out first, centered on it.
    row_height: f32,
}

struct Item {
    url: String,
    file: PathBuf,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Running { received: u64, total: Option<u64> },
    Done { size: Option<u64> },
    Failed,
    Cancelled,
}

/// What the bar asks the browser to do.
pub enum Action {
    Cancel(PathBuf),
}

impl Downloads {
    /// Takes a download event; false for any other event.
    pub fn handle(&mut self, event: &EngineEvent) -> bool {
        match event {
            EngineEvent::DownloadStarted { url, file } => {
                self.items.insert(
                    0,
                    Item {
                        url: url.clone(),
                        file: file.clone(),
                        state: State::Running {
                            received: 0,
                            total: None,
                        },
                    },
                );
                self.shown = true;
            }
            EngineEvent::DownloadFinished { url, file, ok } => {
                let running = |i: &&mut Item| matches!(i.state, State::Running { .. });
                let found = match file {
                    Some(f) => self.items.iter_mut().filter(running).find(|i| &i.file == f),
                    None => None,
                };
                let found = match found {
                    Some(item) => Some(item),
                    None => self
                        .items
                        .iter_mut()
                        .filter(running)
                        .find(|i| &i.url == url),
                };
                let item = match found {
                    Some(item) => item,
                    None => {
                        self.items.insert(
                            0,
                            Item {
                                url: url.clone(),
                                file: file.clone().unwrap_or_default(),
                                state: State::Failed,
                            },
                        );
                        self.shown = true;
                        &mut self.items[0]
                    }
                };
                if let Some(f) = file {
                    item.file = f.clone();
                }
                item.state = if *ok {
                    State::Done {
                        size: std::fs::metadata(&item.file).ok().map(|m| m.len()),
                    }
                } else if item.state == State::Cancelled {
                    State::Cancelled
                } else {
                    State::Failed
                };
            }
            _ => return false,
        }
        true
    }

    /// Updates the downloads under way with what the engine tells.
    pub fn progress(&mut self, progress: &[DownloadProgress]) {
        for item in &mut self.items {
            if let State::Running { .. } = item.state
                && let Some(p) = progress.iter().find(|p| p.file == item.file)
            {
                item.state = State::Running {
                    received: p.received,
                    total: p.total,
                };
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// How many are under way.
    pub fn running(&self) -> usize {
        self.items
            .iter()
            .filter(|i| matches!(i.state, State::Running { .. }))
            .count()
    }

    /// The bar, when shown.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        can_cancel: bool,
        commands: &mut Vec<Command>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        if !self.shown || self.items.is_empty() {
            return actions;
        }
        if self.running() > 0 {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        let mut close = false;
        let mut clear = false;
        let frame = Frame::side_top_panel(&ctx.style()).inner_margin(Margin::symmetric(10, 8));
        TopBottomPanel::bottom("downloads")
            .frame(frame)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.set_min_height(self.row_height);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = super::theme::hover_fill(ui, |ui| {
                            ui.add(egui::Button::new(icon::X).frame(false))
                        })
                        .on_hover_text("Close the downloads bar")
                        .clicked();
                        if self
                            .items
                            .iter()
                            .any(|i| !matches!(i.state, State::Running { .. }))
                        {
                            clear = super::theme::hover_fill(ui, |ui| {
                                ui.add(egui::Button::new("Clear").frame(false))
                            })
                            .on_hover_text("Remove the finished downloads from the list")
                            .clicked();
                        }
                        if let Some(folder) = self.items.first().and_then(|i| i.file.parent())
                            && super::theme::hover_fill(ui, |ui| {
                                ui.add(
                                    egui::Button::new(format!("{} Open folder", icon::FOLDER_OPEN))
                                        .frame(false),
                                )
                            })
                            .on_hover_text(folder.display().to_string())
                            .clicked()
                        {
                            open(folder);
                        }
                        ui.separator();
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                            let cards = egui::ScrollArea::horizontal()
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .show(ui, |ui| {
                                    ui.style_mut().always_scroll_the_only_direction = true;
                                    // Aligned on their top: centered in a row
                                    // that grows with them, they came out
                                    // staggered.
                                    ui.horizontal_top(|ui| {
                                        for item in &mut self.items {
                                            card(ui, item, can_cancel, &mut actions, commands);
                                        }
                                    });
                                });
                            self.row_height = cards.content_size.y;
                        });
                    });
                });
            });
        if clear {
            self.items
                .retain(|i| matches!(i.state, State::Running { .. }));
        }
        if close || self.items.is_empty() {
            self.shown = false;
        }
        actions
    }
}

/// One download: its file, how far it is, what can be done with it.
fn card(
    ui: &mut egui::Ui,
    item: &mut Item,
    can_cancel: bool,
    actions: &mut Vec<Action>,
    commands: &mut Vec<Command>,
) {
    let visuals = ui.visuals().clone();
    let (accent, status) = match item.state {
        State::Running { received, total } => (super::theme::CYAN, running_text(received, total)),
        State::Done { size } => (
            visuals.text_color(),
            match size {
                Some(s) => format!("Done · {}", bytes(s)),
                None => "Done".to_owned(),
            },
        ),
        State::Failed => (visuals.error_fg_color, "Failed".to_owned()),
        State::Cancelled => (visuals.weak_text_color(), "Cancelled".to_owned()),
    };
    let done = matches!(item.state, State::Done { .. });
    // Whether the pointer is on one of the card's buttons: the card itself
    // must not take their click.
    let mut on_button = false;
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = Frame::NONE
        .inner_margin(Margin::symmetric(10, 7))
        .show(ui, |ui| {
            ui.set_width(CARD_WIDTH);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(file_icon(&item.file))
                        .size(26.0)
                        .color(accent),
                );
                ui.vertical(|ui| {
                    ui.set_width(CARD_WIDTH - 80.0);
                    ui.add(
                        egui::Label::new(RichText::new(name(&item.file)).strong())
                            .truncate()
                            .selectable(false),
                    );
                    ui.label(RichText::new(status).size(12.5).color(match item.state {
                        State::Running { .. } | State::Done { .. } => visuals.weak_text_color(),
                        _ => accent,
                    }));
                    if let State::Running { received, total } = item.state {
                        let fraction = total
                            .filter(|&t| t > 0)
                            .map_or(0.0, |t| received as f32 / t as f32);
                        ui.add(
                            egui::ProgressBar::new(fraction)
                                .desired_height(4.0)
                                .fill(super::theme::CYAN)
                                .animate(total.is_none()),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut button = |ui: &mut egui::Ui, glyph: &str, tip: &str| {
                        let response = super::theme::hover_fill(ui, |ui| {
                            ui.add(egui::Button::new(RichText::new(glyph).size(16.0)).frame(false))
                        })
                        .on_hover_text(tip);
                        on_button |= response.hovered();
                        response.clicked()
                    };
                    match item.state {
                        State::Running { .. } if can_cancel => {
                            if button(ui, icon::X, "Cancel") {
                                item.state = State::Cancelled;
                                actions.push(Action::Cancel(item.file.clone()));
                            }
                        }
                        State::Running { .. } => {}
                        State::Done { .. } => {
                            if button(ui, icon::FOLDER_SIMPLE, "Show in folder") {
                                reveal(&item.file);
                            }
                        }
                        State::Failed | State::Cancelled => {
                            if let Ok(address) = WebAddress::parse(&item.url)
                                && button(ui, icon::ARROW_CLOCKWISE, "Download again")
                            {
                                commands.push(Command::Open(address, Target::NewTab));
                            }
                        }
                    }
                });
            });
        });
    let rect = inner.response.rect;
    // Done: the rest of the card opens the file. Sensed for hover only: a
    // click sense laid over the buttons, after them, would take their
    // clicks (egui gives a click to the last widget under the pointer).
    let card = ui.interact(rect, ui.id().with(&item.file), Sense::hover());
    let hovered = card.hovered() || ui.rect_contains_pointer(rect);
    let fill = if hovered && done {
        visuals.widgets.hovered.weak_bg_fill
    } else {
        visuals.faint_bg_color
    };
    ui.painter().set(
        background,
        egui::epaint::RectShape::new(
            rect,
            CornerRadius::same(8),
            fill,
            Stroke::new(
                super::theme::HAIRLINE,
                visuals.widgets.noninteractive.bg_stroke.color,
            ),
            egui::StrokeKind::Inside,
        ),
    );
    if done && !on_button {
        let card = card.on_hover_text(format!("Open {}", name(&item.file)));
        if card.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            if ui.input(|i| i.pointer.primary_clicked()) {
                open(&item.file);
            }
        }
    }
}

fn running_text(received: u64, total: Option<u64>) -> String {
    match total {
        Some(t) if t > 0 => format!(
            "{} of {} · {}%",
            bytes(received),
            bytes(t),
            (received * 100 / t).min(100)
        ),
        _ if received > 0 => format!("{} · downloading…", bytes(received)),
        _ => "Starting…".to_owned(),
    }
}

/// `bytes` for people: 512 B, 3.4 KB, 12.0 MB, 1.25 GB.
fn bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let decimals = if value < 10.0 { 2 } else { 1 };
    format!("{value:.decimals$} {}", UNITS[unit])
}

fn file_icon(file: &Path) -> &'static str {
    let ext = file
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "zip" | "7z" | "rar" | "gz" | "tgz" | "xz" | "bz2" | "tar" => icon::FILE_ZIP,
        "pdf" => icon::FILE_PDF,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "ico" | "avif" => {
            icon::FILE_IMAGE
        }
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" => icon::FILE_AUDIO,
        "mp4" | "mkv" | "webm" | "mov" | "avi" => icon::FILE_VIDEO,
        "exe" | "msi" | "dmg" | "appimage" | "deb" | "rpm" | "apk" => icon::PACKAGE,
        "txt" | "md" | "log" | "csv" => icon::FILE_TEXT,
        "doc" | "docx" | "odt" => icon::FILE_DOC,
        "xls" | "xlsx" | "ods" => icon::FILE_XLS,
        "ppt" | "pptx" | "odp" => icon::FILE_PPT,
        "js" | "ts" | "rs" | "py" | "json" | "html" | "css" | "xml" => icon::FILE_CODE,
        _ => icon::FILE,
    }
}

fn name(file: &Path) -> String {
    file.file_name().map_or_else(
        || file.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// The toolbar's button state: none without downloads.
pub fn toolbar_tip(downloads: &Downloads) -> Option<String> {
    if downloads.is_empty() {
        return None;
    }
    Some(match downloads.running() {
        0 => "Downloads".to_owned(),
        1 => "Downloads: 1 in progress".to_owned(),
        n => format!("Downloads: {n} in progress"),
    })
}

/// Opens `file` with its application.
pub fn open(file: &Path) {
    #[cfg(windows)]
    let result = Process::new("explorer").arg(file).spawn();
    #[cfg(target_os = "macos")]
    let result = Process::new("open").arg(file).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Process::new("xdg-open").arg(file).spawn();
    if let Err(e) = result {
        log::warn!("open {}: {e}", file.display());
    }
}

/// Shows `file` in its folder.
pub fn reveal(file: &Path) {
    #[cfg(windows)]
    let result = {
        use std::os::windows::process::CommandExt as _;
        // explorer wants `/select,"<path>"` as one raw argument.
        Process::new("explorer")
            .raw_arg(format!("/select,\"{}\"", file.display()))
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let result = Process::new("open").arg("-R").arg(file).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Process::new("xdg-open")
        .arg(
            file.parent()
                .map_or_else(|| std::path::PathBuf::from("."), Path::to_path_buf),
        )
        .spawn();
    if let Err(e) = result {
        log::warn!("reveal {}: {e}", file.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn started(url: &str, file: &str) -> EngineEvent {
        EngineEvent::DownloadStarted {
            url: url.into(),
            file: file.into(),
        }
    }

    #[test]
    fn a_download_runs_then_ends() {
        let mut d = Downloads::default();
        assert!(d.handle(&started("https://a.test/x.zip", "C:/D/x.zip")));
        assert!(d.shown);
        assert_eq!(d.running(), 1);
        d.progress(&[DownloadProgress {
            file: "C:/D/x.zip".into(),
            received: 50,
            total: Some(200),
        }]);
        assert_eq!(
            d.items[0].state,
            State::Running {
                received: 50,
                total: Some(200)
            }
        );
        d.handle(&EngineEvent::DownloadFinished {
            url: "https://a.test/x.zip".into(),
            file: Some("C:/D/x.zip".into()),
            ok: true,
        });
        assert!(matches!(d.items[0].state, State::Done { .. }));
        assert_eq!(d.running(), 0);
    }

    #[test]
    fn a_failure_without_file_is_found_by_its_address() {
        let mut d = Downloads::default();
        d.handle(&started("https://a.test/1", "C:/D/1"));
        d.handle(&started("https://a.test/2", "C:/D/2"));
        d.handle(&EngineEvent::DownloadFinished {
            url: "https://a.test/1".into(),
            file: None,
            ok: false,
        });
        assert_eq!(d.items.len(), 2);
        assert_eq!(d.items[1].state, State::Failed);
        assert_eq!(d.running(), 1);
    }

    /// Clicks at `pos` in a frame of the bar; what it asked for.
    fn click(ctx: &egui::Context, d: &mut Downloads, pos: egui::Pos2) -> Vec<Action> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut actions = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(pos)],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let input = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                actions.extend(d.show(ctx, true, &mut Vec::new()));
            });
        }
        actions
    }

    #[test]
    fn the_cancel_button_cancels() {
        let ctx = egui::Context::default();
        let mut d = Downloads::default();
        d.handle(&started("https://a.test/x.zip", "C:/D/x.zip"));
        // Somewhere on the card's row is its cancel button.
        let hit = (0..60)
            .flat_map(|y| (0..80).map(move |x| egui::pos2(x as f32 * 5.0, 600.0 - y as f32)))
            .find_map(|pos| {
                click(&ctx, &mut d, pos)
                    .into_iter()
                    .map(|Action::Cancel(f)| f)
                    .next()
            });
        assert_eq!(hit, Some(PathBuf::from("C:/D/x.zip")));
        assert_eq!(d.items[0].state, State::Cancelled);
    }

    #[test]
    fn sizes_for_people() {
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(3 * 1024 + 512), "3.50 KB");
        assert_eq!(bytes(12 * 1024 * 1024), "12.0 MB");
        assert_eq!(running_text(50, Some(200)), "50 B of 200 B · 25%");
        assert_eq!(running_text(0, None), "Starting…");
    }
}
