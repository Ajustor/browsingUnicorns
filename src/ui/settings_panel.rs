//! The Settings view of the side panel: home page, search engine,
//! downloads, passwords, zoom by site, and clearing what pages left.

use domain::{SearchEngine, Settings, WebAddress};
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
    /// The browsers found here to import bookmarks from (looked for once).
    sources: Option<Vec<infrastructure::bookmark_import::Source>>,
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
    /// The engine can save passwords and clear site data (WebView2).
    pub passwords: bool,
    pub clear_site_data: bool,
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

        section(ui, icon::HOUSE, "Home page");
        ui.weak("Opened by new tabs and the Home button.");
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
            ui.colored_label(ui.visuals().error_fg_color, "Not a web address.");
        }

        section(ui, icon::MAGNIFYING_GLASS, "Search engine");
        ui.weak("What the address bar searches with.");
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

        section(ui, icon::DOWNLOAD_SIMPLE, "Downloads");
        let folder = settings
            .device
            .download_dir
            .clone()
            .or(view.default_download_dir.clone());
        ui.horizontal_wrapped(|ui| {
            ui.label("Save to");
            ui.label(
                RichText::new(folder.as_ref().map_or_else(
                    || "the Downloads folder".to_owned(),
                    |f| f.display().to_string(),
                ))
                .strong(),
            );
        });
        ui.horizontal(|ui| {
            if ui
                .button(format!("{} Change…", icon::FOLDER_OPEN))
                .clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_title("Where downloads go")
                    .set_directory(folder.clone().unwrap_or_default())
                    .pick_folder()
            {
                change(&|s| s.device.download_dir = Some(dir.clone()));
            }
            if settings.device.download_dir.is_some()
                && ui
                    .button("Downloads folder")
                    .on_hover_text("Back to the system's Downloads folder")
                    .clicked()
            {
                change(&|s| s.device.download_dir = None);
            }
        });
        let mut ask = settings.device.ask_where_to_save;
        if ui
            .checkbox(&mut ask, "Ask where to save each file")
            .changed()
        {
            change(&|s| s.device.ask_where_to_save = ask);
        }

        if view.passwords {
            section(ui, icon::KEY, "Passwords");
            let mut save = settings.device.save_passwords;
            if ui
                .checkbox(&mut save, "Offer to save the passwords I type")
                .on_hover_text(
                    "The web view keeps them for this profile and fills them in next time. \
                     A password manager extension (Bitwarden…) works either way.",
                )
                .changed()
            {
                change(&|s| s.device.save_passwords = save);
            }
        }

        if !settings.device.site_zoom.is_empty() {
            section(ui, icon::MAGNIFYING_GLASS_PLUS, "Zoom by site");
            let mut reset = None;
            for (host, percent) in &settings.device.site_zoom {
                ui.horizontal(|ui| {
                    if super::theme::hover_fill(ui, |ui| {
                        ui.add(Button::new(icon::X).frame(false).small())
                    })
                    .on_hover_text("Back to 100%")
                    .clicked()
                    {
                        reset = Some(host.clone());
                    }
                    ui.label(format!("{host}: {percent}%"));
                });
            }
            if let Some(host) = reset {
                change(&|s| {
                    s.device.set_zoom(&host, 100);
                });
                commands.push(Command::ApplyZoom);
            }
        }

        section(ui, icon::STAR, "Import bookmarks");
        ui.weak("Into a folder named after the browser.");
        let sources = self
            .sources
            .get_or_insert_with(infrastructure::bookmark_import::chromium_sources);
        ui.horizontal_wrapped(|ui| {
            for source in sources.iter() {
                if ui.button(format!("From {}", source.name)).clicked() {
                    commands.push(Command::ImportBookmarks(ImportFrom::Chromium(
                        source.name.to_owned(),
                        source.file.clone(),
                    )));
                }
            }
            if ui
                .button(format!("{} From an HTML file…", icon::FILE_HTML))
                .on_hover_text(
                    "The file other browsers export (Firefox: Bookmarks > Manage \
                     bookmarks > Import and backup > Export bookmarks to HTML)",
                )
                .clicked()
                && let Some(file) = rfd::FileDialog::new()
                    .set_title("Bookmarks exported from a browser")
                    .add_filter("Bookmarks", &["html", "htm"])
                    .pick_file()
            {
                commands.push(Command::ImportBookmarks(ImportFrom::Html(file)));
            }
        });

        section(ui, icon::BROOM, "Clear browsing data");
        ui.checkbox(&mut self.clear.history, "History");
        if view.clear_site_data {
            ui.checkbox(
                &mut self.clear.cookies,
                "Cookies and site data (signs you out of sites)",
            );
            ui.checkbox(&mut self.clear.cache, "Cached images and files");
        }
        let choice = ClearData {
            history: self.clear.history,
            cookies: self.clear.cookies && view.clear_site_data,
            cache: self.clear.cache && view.clear_site_data,
        };
        if ui
            .add_enabled(
                choice.history || choice.cookies || choice.cache,
                Button::new(format!("{} Clear now", icon::TRASH)),
            )
            .clicked()
        {
            self.cleared = None;
            commands.push(Command::ClearBrowsingData(choice));
        }
        if let Some(text) = &self.cleared {
            ui.label(text);
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

/// The home page typed: a full address, or a host name.
fn parse_home(text: &str) -> Option<WebAddress> {
    let text = text.trim();
    WebAddress::parse(text)
        .ok()
        .or_else(|| WebAddress::parse(&format!("https://{text}")).ok())
        .filter(|a| {
            a.host()
                .is_some_and(|h| h.contains('.') || h == "localhost")
        })
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
