//! The Settings view of the side panel: language, home page, search engine,
//! downloads, passwords, zoom by site, clearing what pages left, and
//! resetting the protected videos' data.

use domain::{LanguageChoice, SearchEngine, Settings, WebAddress};
use egui::{Button, RichText, TextEdit};
use egui_phosphor::regular as icon;

use super::command::{ClearData, Command, ImportFrom};

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

        section(ui, icon::TRANSLATE, i18n::settings::language());
        ui.weak(i18n::settings::language_hint());
        egui::ComboBox::from_id_salt("language")
            .selected_text(language_name(view.language))
            .width(ui.available_width().min(240.0))
            .show_ui(ui, |ui| {
                for choice in [LanguageChoice::Auto, LanguageChoice::En, LanguageChoice::Fr] {
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

        section(ui, icon::HOUSE, i18n::settings::home_page());
        ui.weak(i18n::settings::home_page_hint());
        let text = self
            .home_page
            .get_or_insert_with(|| settings.home_page.to_string());
        let field = ui.add(
            TextEdit::singleline(text)
                .desired_width(f32::INFINITY)
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
                i18n::settings::not_an_address(),
            );
        }

        section(ui, icon::MAGNIFYING_GLASS, i18n::settings::search_engine());
        ui.weak(i18n::settings::search_engine_hint());
        egui::ComboBox::from_id_salt("search engine")
            .selected_text(settings.search_engine.name.as_str())
            .width(ui.available_width().min(240.0))
            .show_ui(ui, |ui| {
                for engine in SearchEngine::well_known() {
                    let selected = engine == settings.search_engine;
                    if ui.selectable_label(selected, &engine.name).clicked() && !selected {
                        change(&|s| s.search_engine = engine.clone());
                    }
                }
            });

        section(ui, icon::TABS, i18n::settings::tabs());
        ui.weak(i18n::settings::tabs_hint());
        let shrink = settings.device.shrink_tabs;
        for (choice, text) in [
            (false, i18n::settings::tabs_scroll()),
            (true, i18n::settings::tabs_shrink()),
        ] {
            if ui.radio(shrink == choice, text).clicked() && shrink != choice {
                change(&|s| s.device.shrink_tabs = choice);
            }
        }

        section(ui, icon::DOWNLOAD_SIMPLE, i18n::settings::downloads());
        let folder = settings
            .device
            .download_dir
            .clone()
            .or(view.default_download_dir.clone());
        ui.horizontal_wrapped(|ui| {
            ui.label(i18n::settings::save_to());
            ui.label(
                RichText::new(folder.as_ref().map_or_else(
                    || i18n::settings::system_downloads_folder().to_owned(),
                    |f| f.display().to_string(),
                ))
                .strong(),
            );
        });
        ui.horizontal(|ui| {
            if ui
                .button(format!(
                    "{} {}",
                    icon::FOLDER_OPEN,
                    i18n::settings::change_folder()
                ))
                .clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_title(i18n::settings::folder_dialog_title())
                    .set_directory(folder.clone().unwrap_or_default())
                    .pick_folder()
            {
                change(&|s| s.device.download_dir = Some(dir.clone()));
            }
            if settings.device.download_dir.is_some()
                && ui
                    .button(i18n::settings::downloads_folder())
                    .on_hover_text(i18n::settings::downloads_folder_tip())
                    .clicked()
            {
                change(&|s| s.device.download_dir = None);
            }
        });
        let mut ask = settings.device.ask_where_to_save;
        if ui
            .checkbox(&mut ask, i18n::settings::ask_where_to_save())
            .changed()
        {
            change(&|s| s.device.ask_where_to_save = ask);
        }

        if view.passwords {
            section(ui, icon::KEY, i18n::settings::passwords());
            let mut save = settings.device.save_passwords;
            if ui
                .checkbox(&mut save, i18n::settings::offer_to_save_passwords())
                .on_hover_text(i18n::settings::offer_to_save_passwords_tip())
                .changed()
            {
                change(&|s| s.device.save_passwords = save);
            }
            let mut cookies = settings.device.sync_cookies;
            if ui
                .checkbox(&mut cookies, i18n::passwords::sync_cookies())
                .on_hover_text(i18n::passwords::sync_cookies_tip())
                .changed()
            {
                change(&|s| s.device.sync_cookies = cookies);
            }
            ui.add_space(4.0);
            super::passwords::list(ui, view.credentials, commands);
        }

        if !settings.device.site_zoom.is_empty() {
            section(
                ui,
                icon::MAGNIFYING_GLASS_PLUS,
                i18n::settings::zoom_by_site(),
            );
            let mut reset = None;
            for (host, percent) in &settings.device.site_zoom {
                ui.horizontal(|ui| {
                    if super::theme::hover_fill(ui, |ui| {
                        ui.add(Button::new(icon::X).frame(false).small())
                    })
                    .on_hover_text(i18n::settings::zoom_reset_tip())
                    .clicked()
                    {
                        reset = Some(host.clone());
                    }
                    ui.label(i18n::settings::zoom_of(host, *percent));
                });
            }
            if let Some(host) = reset {
                change(&|s| {
                    s.device.set_zoom(&host, 100);
                });
                commands.push(Command::ApplyZoom);
            }
        }

        section(ui, icon::STAR, i18n::settings::import_bookmarks());
        ui.weak(i18n::settings::import_bookmarks_hint());
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

        section(ui, icon::BROOM, i18n::settings::clear_browsing_data());
        ui.checkbox(&mut self.clear.history, i18n::settings::clear_history());
        if view.clear_site_data {
            ui.checkbox(&mut self.clear.cookies, i18n::settings::clear_cookies());
            ui.checkbox(&mut self.clear.cache, i18n::settings::clear_cache());
        }
        let choice = ClearData {
            history: self.clear.history,
            cookies: self.clear.cookies && view.clear_site_data,
            cache: self.clear.cache && view.clear_site_data,
        };
        if ui
            .add_enabled(
                choice.history || choice.cookies || choice.cache,
                Button::new(format!("{} {}", icon::TRASH, i18n::settings::clear_now())),
            )
            .clicked()
        {
            self.cleared = None;
            commands.push(Command::ClearBrowsingData(choice));
        }
        if let Some(text) = &self.cleared {
            ui.label(text);
        }

        if view.reset_drm {
            section(ui, icon::FILM_STRIP, i18n::settings::protected_videos());
            ui.weak(i18n::settings::protected_videos_hint());
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
        }

        if let Some(s) = changed {
            commands.push(Command::UpdateSettings(Box::new(s)));
        }
    }
}

fn section(ui: &mut egui::Ui, glyph: &str, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(format!("{glyph} {title}")).strong());
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
