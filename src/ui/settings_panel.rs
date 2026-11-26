//! The Settings view of the side panel: language, home page, search engine,
//! downloads, passwords, zoom by site, clearing what pages left, and
//! resetting the protected videos' data.

use domain::{LanguageChoice, SearchEngine, Settings, TabPosition, WebAddress};
use egui::{Button, RichText, TextEdit};
use egui_phosphor::regular as icon;

use super::command::{ClearData, Command, ImportFrom};
use super::panel_kit::{card, segmented, switch_row};

/// What the view keeps between frames: the fields being typed in.
#[derive(Default)]
pub struct SettingsPanel {
    /// The home page as typed, `None` while it shows the saved one.
    home_page: Option<String>,
    home_error: bool,
    clear: ClearChoice,
    /// What the last clearing did, to tell.
    pub cleared: Option<String>,
    /// The browsers found here to import from (looked for once).
    sources: Option<Vec<infrastructure::browser_import::Source>>,
}

/// What "Clear browsing data" removes.
struct ClearChoice {
    history: bool,
    cookies: bool,
    cache: bool,
}

impl Default for ClearChoice {
    fn default() -> Self {
        Self {
            history: false,
            cookies: true,
            cache: true,
        }
    }
}

/// What the view needs to know besides the settings.
pub struct SettingsView<'a> {
    pub settings: &'a Settings,
    /// The language chosen on this computer.
    pub language: LanguageChoice,
    /// The engine can save passwords and clear site data (WebView2).
    pub passwords: bool,
    /// The saved sign-ins.
    pub credentials: &'a domain::Credentials,
    pub clear_site_data: bool,
    /// The protected videos' data can be reset (Windows, the profile's
    /// window).
    pub reset_drm: bool,
    /// Where downloads go when no folder is chosen.
    pub default_download_dir: Option<std::path::PathBuf>,
}

impl SettingsPanel {
    pub fn show(&mut self, ui: &mut egui::Ui, view: SettingsView<'_>, commands: &mut Vec<Command>) {
        let settings = view.settings;
        let mut changed: Option<Settings> = None;
        let mut change = |f: &dyn Fn(&mut Settings)| {
            let mut s = changed.take().unwrap_or_else(|| settings.clone());
            f(&mut s);
            changed = Some(s);
        };

        card(
            ui,
            icon::TRANSLATE,
            i18n::settings::language(),
            Some(i18n::settings::language_hint()),
            |ui| {
                egui::ComboBox::from_id_salt("language")
                    .selected_text(language_name(view.language))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for choice in [LanguageChoice::Auto, LanguageChoice::En, LanguageChoice::Fr]
                        {
                            let selected = choice == view.language;
                            if ui
                                .selectable_label(selected, language_name(choice))
                                .clicked()
                                && !selected
                            {
                                commands.push(Command::SetLanguage(choice));
                            }
                        }
                    });
            },
        );

        card(
            ui,
            icon::HOUSE,
            i18n::settings::home_page(),
            Some(i18n::settings::home_page_hint()),
            |ui| {
                let text = self
                    .home_page
                    .get_or_insert_with(|| settings.home_page.to_string());
                let field = ui.add(
                    TextEdit::singleline(text)
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 6))
                        .hint_text("https://…"),
                );
                let typed = text.clone();
                if field.changed() {
                    self.home_error = false;
                }
                if field.lost_focus() {
                    match parse_home(&typed) {
                        Some(address) if address != settings.home_page => {
                            change(&|s| s.home_page = address.clone());
                            self.home_page = None;
                        }
                        Some(_) => self.home_page = None,
                        None => self.home_error = true,
                    }
                }
                if self.home_error {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{} {}", icon::WARNING, i18n::settings::not_an_address()),
                    );
                }
            },
        );

        card(
            ui,
            icon::MAGNIFYING_GLASS,
            i18n::settings::search_engine(),
            Some(i18n::settings::search_engine_hint()),
            |ui| {
                egui::ComboBox::from_id_salt("search engine")
                    .selected_text(settings.search_engine.name.as_str())
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        for engine in SearchEngine::well_known() {
                            let selected = engine == settings.search_engine;
                            if ui.selectable_label(selected, &engine.name).clicked() && !selected {
                                change(&|s| s.search_engine = engine.clone());
                            }
                        }
                    });
            },
        );

        card(
            ui,
            icon::TABS,
            i18n::settings::tabs(),
            Some(i18n::settings::tabs_hint()),
            |ui| {
                ui.weak(i18n::settings::tabs_position());
                let left = settings.device.tab_position == TabPosition::Left;
                let positions = [
                    (icon::TABS, i18n::settings::tabs_top()),
                    (icon::SIDEBAR_SIMPLE, i18n::settings::tabs_left()),
                ];
                if let Some(picked) = segmented(ui, &positions, usize::from(left)) {
                    let choice = if picked == 1 {
                        TabPosition::Left
                    } else {
                        TabPosition::Top
                    };
                    change(&|s| s.device.tab_position = choice);
                }
                // The column scrolls: only the strip has a choice.
                if !left {
                    ui.add_space(6.0);
                    ui.weak(i18n::settings::tabs_overflow());
                    let shrink = settings.device.shrink_tabs;
                    let options = [
                        (icon::ARROWS_LEFT_RIGHT, i18n::settings::tabs_scroll()),
                        (
                            icon::ARROWS_IN_LINE_HORIZONTAL,
                            i18n::settings::tabs_shrink(),
                        ),
                    ];
                    if let Some(picked) = segmented(ui, &options, usize::from(shrink)) {
                        let choice = picked == 1;
                        change(&|s| s.device.shrink_tabs = choice);
                    }
                }
            },
        );

        card(
            ui,
            icon::DOWNLOAD_SIMPLE,
            i18n::settings::downloads(),
            None,
            |ui| {
                let folder = settings
                    .device
                    .download_dir
                    .clone()
                    .or(view.default_download_dir.clone());
                ui.weak(i18n::settings::save_to());
                let shown = folder.as_ref().map_or_else(
                    || i18n::settings::system_downloads_folder().to_owned(),
                    |f| f.display().to_string(),
                );
                ui.horizontal(|ui| {
                    let change_text =
                        format!("{} {}", icon::FOLDER_OPEN, i18n::settings::change_folder());
                    let button_width = ui
                        .painter()
                        .layout_no_wrap(
                            change_text.clone(),
                            egui::TextStyle::Button.resolve(ui.style()),
                            ui.visuals().text_color(),
                        )
                        .size()
                        .x
                        + 2.0 * ui.spacing().button_padding.x;
                    let path_width =
                        ui.available_width() - button_width - ui.spacing().item_spacing.x - 2.0;
                    egui::Frame::NONE
                        .fill(ui.visuals().extreme_bg_color)
                        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.set_width(path_width - 16.0);
                            ui.add(
                                egui::Label::new(format!("{} {shown}", icon::FOLDER)).truncate(),
                            )
                            .on_hover_text(&shown);
                        });
                    if ui.button(change_text).clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title(i18n::settings::folder_dialog_title())
                            .set_directory(folder.clone().unwrap_or_default())
                            .pick_folder()
                    {
                        change(&|s| s.device.download_dir = Some(dir.clone()));
                    }
                });
                if settings.device.download_dir.is_some()
                    && ui
                        .button(format!(
                            "{} {}",
                            icon::ARROW_COUNTER_CLOCKWISE,
                            i18n::settings::downloads_folder()
                        ))
                        .on_hover_text(i18n::settings::downloads_folder_tip())
                        .clicked()
                {
                    change(&|s| s.device.download_dir = None);
                }
                let mut ask = settings.device.ask_where_to_save;
                if switch_row(ui, i18n::settings::ask_where_to_save(), None, &mut ask) {
                    change(&|s| s.device.ask_where_to_save = ask);
                }
            },
        );

        if view.passwords {
            card(ui, icon::KEY, i18n::settings::passwords(), None, |ui| {
                let mut save = settings.device.save_passwords;
                if switch_row(
                    ui,
                    i18n::settings::offer_to_save_passwords(),
                    Some(i18n::settings::offer_to_save_passwords_tip()),
                    &mut save,
                ) {
                    change(&|s| s.device.save_passwords = save);
                }
                let mut cookies = settings.device.sync_cookies;
                if switch_row(
                    ui,
                    i18n::passwords::sync_cookies(),
                    Some(i18n::passwords::sync_cookies_tip()),
                    &mut cookies,
                ) {
                    change(&|s| s.device.sync_cookies = cookies);
                }
                ui.separator();
                super::passwords::list(ui, view.credentials, commands);
            });
        }

        if !settings.device.site_zoom.is_empty() {
            card(
                ui,
                icon::MAGNIFYING_GLASS_PLUS,
                i18n::settings::zoom_by_site(),
                None,
                |ui| {
                    let mut reset = None;
                    for (host, percent) in &settings.device.site_zoom {
                        ui.horizontal(|ui| {
                            ui.label(host);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if super::theme::hover_fill(ui, |ui| {
                                        ui.add(Button::new(icon::X).frame(false).small())
                                    })
                                    .on_hover_text(i18n::settings::zoom_reset_tip())
                                    .clicked()
                                    {
                                        reset = Some(host.clone());
                                    }
                                    ui.label(RichText::new(format!("{percent} %")).strong());
                                },
                            );
                        });
                    }
                    if let Some(host) = reset {
                        change(&|s| {
                            s.device.set_zoom(&host, 100);
                        });
                        commands.push(Command::ApplyZoom);
                    }
                },
            );
        }

        card(
            ui,
            icon::DOWNLOAD,
            i18n::settings::import_bookmarks(),
            Some(i18n::settings::import_bookmarks_hint()),
            |ui| {
                let sources = self
                    .sources
                    .get_or_insert_with(infrastructure::browser_import::sources);
                ui.horizontal_wrapped(|ui| {
                    for source in sources.iter() {
                        if ui
                            .button(i18n::settings::import_from(source.name))
                            .clicked()
                        {
                            commands.push(Command::Import(ImportFrom::Browser(source.clone())));
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(format!(
                            "{} {}",
                            icon::FILE_HTML,
                            i18n::settings::import_html()
                        ))
                        .on_hover_text(i18n::settings::import_html_tip())
                        .clicked()
                        && let Some(file) = rfd::FileDialog::new()
                            .set_title(i18n::settings::import_dialog_title())
                            .add_filter(i18n::settings::import_filter(), &["html", "htm"])
                            .pick_file()
                    {
                        commands.push(Command::Import(ImportFrom::Html(file)));
                    }
                    if ui
                        .button(format!(
                            "{} {}",
                            icon::KEY,
                            i18n::settings::import_passwords()
                        ))
                        .on_hover_text(i18n::settings::import_passwords_tip())
                        .clicked()
                        && let Some(file) = rfd::FileDialog::new()
                            .set_title(i18n::settings::import_passwords_title())
                            .add_filter(i18n::settings::import_passwords_filter(), &["csv"])
                            .pick_file()
                    {
                        commands.push(Command::Import(ImportFrom::Passwords(file)));
                    }
                });
            },
        );

        card(
            ui,
            icon::BROOM,
            i18n::settings::clear_browsing_data(),
            None,
            |ui| {
                switch_row(
                    ui,
                    i18n::settings::clear_history(),
                    None,
                    &mut self.clear.history,
                );
                if view.clear_site_data {
                    switch_row(
                        ui,
                        i18n::settings::clear_cookies(),
                        None,
                        &mut self.clear.cookies,
                    );
                    switch_row(
                        ui,
                        i18n::settings::clear_cache(),
                        None,
                        &mut self.clear.cache,
                    );
                }
                let choice = ClearData {
                    history: self.clear.history,
                    cookies: self.clear.cookies && view.clear_site_data,
                    cache: self.clear.cache && view.clear_site_data,
                };
                let danger = ui.visuals().error_fg_color;
                if ui
                    .add_enabled(
                        choice.history || choice.cookies || choice.cache,
                        Button::new(
                            RichText::new(format!(
                                "{} {}",
                                icon::TRASH,
                                i18n::settings::clear_now()
                            ))
                            .color(danger),
                        )
                        .stroke(egui::Stroke::new(1.0_f32, danger.gamma_multiply(0.6)))
                        .min_size(egui::vec2(ui.available_width(), 30.0)),
                    )
                    .clicked()
                {
                    self.cleared = None;
                    commands.push(Command::ClearBrowsingData(choice));
                }
                if let Some(text) = &self.cleared {
                    ui.label(format!("{} {text}", icon::CHECK));
                }
            },
        );

        if view.reset_drm {
            card(
                ui,
                icon::FILM_STRIP,
                i18n::settings::protected_videos(),
                Some(i18n::settings::protected_videos_hint()),
                |ui| {
                    if ui
                        .button(format!(
                            "{} {}",
                            icon::ARROW_CLOCKWISE,
                            i18n::settings::reset_drm()
                        ))
                        .on_hover_text(i18n::settings::reset_drm_tip())
                        .clicked()
                    {
                        commands.push(Command::ResetDrm);
                    }
                },
            );
        }
        ui.add_space(8.0);

        if let Some(s) = changed {
            commands.push(Command::UpdateSettings(Box::new(s)));
        }
    }
}

/// A language as the menu offers it: each in itself, Automatic in the
/// current one.
fn language_name(choice: LanguageChoice) -> &'static str {
    match choice {
        LanguageChoice::Auto => i18n::settings::language_auto(),
        LanguageChoice::En => i18n::Lang::En.native_name(),
        LanguageChoice::Fr => i18n::Lang::Fr.native_name(),
    }
}

fn parse_home(text: &str) -> Option<WebAddress> {
    WebAddress::parse_site(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_pages_are_addresses_or_hosts() {
        assert_eq!(
            parse_home(" example.com ").unwrap().as_str(),
            "https://example.com/"
        );
        assert_eq!(
            parse_home("http://localhost:3000").unwrap().as_str(),
            "http://localhost:3000/"
        );
        assert!(parse_home("not an address").is_none());
        assert!(parse_home("").is_none());
    }
}
