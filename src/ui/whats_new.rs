//! The "What's new" view of the side panel: the release notes of each
//! version, read from the CHANGELOG built into the browser.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, FontId, RichText};
use egui_phosphor::regular as icon;

use super::panel_kit::{self, card_frame, chip};

/// The CHANGELOG, as published with each version.
const CHANGELOG: &str = include_str!("../../CHANGELOG.md");

/// How many versions show at first, and how many more each time.
const PAGE: usize = 8;

/// One version's notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub date: String,
    pub sections: Vec<Section>,
}

/// "Added", "Fixed"… and what goes in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// A list item, `depth` 0 at the margin.
    Item {
        depth: usize,
        text: String,
    },
    Paragraph(String),
}

/// The releases of a Keep a Changelog file, newest first as written.
pub fn parse(markdown: &str) -> Vec<Release> {
    let mut releases = Vec::new();
    let mut lines = Vec::new();
    let mut head: Option<(String, String)> = None;
    for line in markdown.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            if let Some((version, date)) = head.take() {
                releases.push(Release {
                    version,
                    date,
                    sections: sections(&lines),
                });
            }
            lines.clear();
            let (version, date) = title.split_once(" - ").unwrap_or((title, ""));
            head = Some((
                version.trim().trim_matches(['[', ']']).to_owned(),
                date.trim().to_owned(),
            ));
        } else if head.is_some() {
            lines.push(line);
        }
    }
    if let Some((version, date)) = head {
        releases.push(Release {
            version,
            date,
            sections: sections(&lines),
        });
    }
    releases
}

/// The sections of one version's notes (`### Added`, then its list).
pub fn sections(lines: &[&str]) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    // Whether the last line belongs to a block still open.
    let mut open = false;
    for line in lines {
        if let Some(title) = line.strip_prefix("### ") {
            sections.push(Section {
                title: title.trim().to_owned(),
                blocks: Vec::new(),
            });
            open = false;
            continue;
        }
        if line.trim().is_empty() {
            open = false;
            continue;
        }
        if sections.is_empty() {
            sections.push(Section {
                title: String::new(),
                blocks: Vec::new(),
            });
        }
        let blocks = &mut sections.last_mut().expect("one at least").blocks;
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();
        if let Some(text) = trimmed.strip_prefix("- ").or(trimmed.strip_prefix("* ")) {
            blocks.push(Block::Item {
                depth: indent / 2,
                text: text.to_owned(),
            });
            open = true;
        } else if open
            && let Some(Block::Item { text, .. } | Block::Paragraph(text)) = blocks.last_mut()
        {
            text.push(' ');
            text.push_str(trimmed);
        } else {
            blocks.push(Block::Paragraph(trimmed.to_owned()));
            open = true;
        }
    }
    sections
}

/// How a piece of text is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    Strong,
    Emphasis,
    Code,
}

/// The text cut where its Markdown style changes: `code`, **strong**,
/// *emphasis*; links keep their text.
pub fn spans(text: &str) -> Vec<(Style, String)> {
    let mut out: Vec<(Style, String)> = Vec::new();
    let mut push = |style: Style, s: &str| {
        if s.is_empty() {
            return;
        }
        match out.last_mut() {
            Some((last, text)) if *last == style => text.push_str(s),
            _ => out.push((style, s.to_owned())),
        }
    };
    let mut rest = text;
    while !rest.is_empty() {
        let next = rest.find(['`', '*', '[']).unwrap_or(rest.len());
        push(Style::Plain, &rest[..next]);
        rest = &rest[next..];
        if rest.is_empty() {
            break;
        }
        let (marker, style) = if rest.starts_with("**") {
            ("**", Style::Strong)
        } else if rest.starts_with('*') {
            ("*", Style::Emphasis)
        } else if rest.starts_with('`') {
            ("`", Style::Code)
        } else {
            // A link: its text, without the address.
            if let Some(close) = rest.find("](")
                && let Some(end) = rest[close..].find(')')
            {
                push(Style::Plain, &rest[1..close]);
                rest = &rest[close + end + 1..];
            } else {
                push(Style::Plain, "[");
                rest = &rest[1..];
            }
            continue;
        };
        let inner = &rest[marker.len()..];
        match inner.find(marker) {
            Some(end) if end > 0 => {
                push(style, &inner[..end]);
                rest = &inner[end + marker.len()..];
            }
            _ => {
                push(Style::Plain, marker);
                rest = inner;
            }
        }
    }
    out
}

/// The color of a section's chip, by its title.
fn section_color(title: &str) -> Color32 {
    let title = title.to_lowercase();
    let pick = if title.starts_with("ajout") || title.starts_with("add") {
        domain::GroupColor::Green
    } else if title.starts_with("corrig") || title.starts_with("fix") {
        domain::GroupColor::Orange
    } else if title.starts_with("retir")
        || title.starts_with("supprim")
        || title.starts_with("remov")
    {
        domain::GroupColor::Red
    } else if title.starts_with("sécu") || title.starts_with("secu") {
        domain::GroupColor::Yellow
    } else {
        domain::GroupColor::Blue
    };
    super::theme::group_color(pick)
}

/// What the view keeps between frames.
pub struct WhatsNewPanel {
    releases: Vec<Release>,
    shown: usize,
}

impl Default for WhatsNewPanel {
    fn default() -> Self {
        Self {
            releases: Vec::new(),
            shown: PAGE,
        }
    }
}

impl WhatsNewPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, current: &str) {
        if self.releases.is_empty() {
            self.releases = parse(CHANGELOG);
        }
        if i18n::current() != i18n::Lang::Fr {
            ui.add(
                egui::Label::new(
                    RichText::new(i18n::whats_new::in_french())
                        .weak()
                        .size(12.5),
                )
                .wrap(),
            );
            ui.add_space(6.0);
        }
        for release in self.releases.iter().take(self.shown) {
            card_frame(ui, |ui| {
                release_card(ui, release, release.version == current)
            });
        }
        if self.shown < self.releases.len() {
            ui.vertical_centered(|ui| {
                if ui
                    .button(format!("{} {}", icon::CARET_DOWN, i18n::whats_new::older()))
                    .clicked()
                {
                    self.shown += PAGE;
                }
            });
        }
    }
}

fn release_card(ui: &mut egui::Ui, release: &Release, installed: bool) {
    ui.horizontal(|ui| {
        panel_kit::badge(ui, icon::SPARKLE, 28.0, panel_kit::ACCENT);
        ui.add_space(4.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(i18n::whats_new::version(&release.version))
                        .strong()
                        .size(15.0),
                );
                if installed {
                    chip(ui, i18n::whats_new::installed(), panel_kit::cyan_text(ui));
                }
            });
            if !release.date.is_empty() {
                ui.label(RichText::new(&release.date).weak().size(12.5));
            }
        });
    });
    ui.add_space(4.0);
    notes(ui, &release.sections);
}

/// A version's notes: each section's chip, then its list.
pub fn notes(ui: &mut egui::Ui, sections: &[Section]) {
    for section in sections {
        if !section.title.is_empty() {
            ui.add_space(4.0);
            chip(ui, &section.title, section_color(&section.title));
        }
        for block in &section.blocks {
            let (depth, text, bullet) = match block {
                Block::Item { depth, text } => (*depth, text, true),
                Block::Paragraph(text) => (0, text, false),
            };
            ui.horizontal_top(|ui| {
                ui.add_space(4.0 + 14.0 * depth as f32);
                if bullet {
                    let dot = if depth == 0 { "•" } else { "◦" };
                    ui.label(RichText::new(dot).color(panel_kit::accent_text(ui)));
                }
                ui.add(egui::Label::new(rich(ui, text)).wrap());
            });
        }
    }
}

/// Markdown text, styled.
fn rich(ui: &egui::Ui, text: &str) -> LayoutJob {
    let color = ui.visuals().text_color();
    let strong = ui.visuals().strong_text_color();
    let body = FontId::proportional(13.5);
    let mut job = LayoutJob::default();
    for (style, piece) in spans(text) {
        let format = match style {
            Style::Plain => TextFormat::simple(body.clone(), color),
            Style::Strong => TextFormat::simple(body.clone(), strong),
            Style::Emphasis => TextFormat {
                italics: true,
                ..TextFormat::simple(body.clone(), color)
            },
            Style::Code => TextFormat {
                background: ui.visuals().code_bg_color,
                ..TextFormat::simple(FontId::monospace(12.5), color)
            },
        };
        job.append(&piece, 0.0, format);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Changelog\n\nIntro [link](https://x/).\n\n\
## [1.1.0] - 2026-01-02\n\n### Ajouté\n\n- One thing,\n  on two lines.\n- Two:\n  - nested\n\n\
### Corrigé\n\nA paragraph\nthat goes on.\n\n## [1.0.0] - 2026-01-01\n\nPremière version.\n";

    #[test]
    fn releases_sections_and_items() {
        let releases = parse(SAMPLE);
        assert_eq!(releases.len(), 2);
        assert_eq!(releases[0].version, "1.1.0");
        assert_eq!(releases[0].date, "2026-01-02");
        let added = &releases[0].sections[0];
        assert_eq!(added.title, "Ajouté");
        assert_eq!(
            added.blocks,
            vec![
                Block::Item {
                    depth: 0,
                    text: "One thing, on two lines.".into()
                },
                Block::Item {
                    depth: 0,
                    text: "Two:".into()
                },
                Block::Item {
                    depth: 1,
                    text: "nested".into()
                },
            ]
        );
        assert_eq!(
            releases[0].sections[1].blocks,
            vec![Block::Paragraph("A paragraph that goes on.".into())]
        );
        assert_eq!(releases[1].sections[0].title, "");
        assert_eq!(
            releases[1].sections[0].blocks,
            vec![Block::Paragraph("Première version.".into())]
        );
    }

    #[test]
    fn the_built_in_changelog_reads() {
        let releases = parse(CHANGELOG);
        assert!(releases.len() > 20);
        assert_eq!(releases[0].version, env!("CARGO_PKG_VERSION"));
        assert!(releases.iter().all(|r| !r.sections.is_empty()));
    }

    #[test]
    fn notes_without_a_version_head() {
        let lines: Vec<&str> = "### Fixed\n\n- A bug.".lines().collect();
        let s = sections(&lines);
        assert_eq!(s[0].title, "Fixed");
        assert_eq!(s[0].blocks.len(), 1);
    }

    #[test]
    fn inline_styles() {
        assert_eq!(
            spans("Use `chrome` with *care* and **force**, see [the doc](https://d/)."),
            vec![
                (Style::Plain, "Use ".into()),
                (Style::Code, "chrome".into()),
                (Style::Plain, " with ".into()),
                (Style::Emphasis, "care".into()),
                (Style::Plain, " and ".into()),
                (Style::Strong, "force".into()),
                (Style::Plain, ", see the doc.".into()),
            ]
        );
        assert_eq!(
            spans("2 * 3 = 6 [x"),
            vec![(Style::Plain, "2 * 3 = 6 [x".into())]
        );
    }
}
