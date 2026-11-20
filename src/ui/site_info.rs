//! What the padlock shows: whether the connection is encrypted and the
//! certificate the site showed (its chain), in a bar under the toolbar
//! (the page's native view would hide anything drawn over it).

use domain::WebAddress;
use egui::{CollapsingHeader, RichText, TopBottomPanel};
use egui_phosphor::regular as icon;
use infrastructure::certificates::{CertificateInfo, Certificates};

/// The site whose information is shown.
pub struct SiteInfo {
    pub address: WebAddress,
    /// `None` while the engine looks it up.
    pub certificates: Option<Certificates>,
}

impl SiteInfo {
    /// Shows the bar; false once closed.
    pub fn show(&self, ctx: &egui::Context) -> bool {
        let mut open = true;
        TopBottomPanel::top("site_info").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let host = self.address.host().unwrap_or(self.address.as_str());
                if self.address.is_secure() {
                    ui.label(RichText::new(format!("{} {host}", icon::LOCK_SIMPLE)).strong());
                    ui.label(i18n::site_info::connection_secure());
                } else {
                    ui.label(RichText::new(format!("{} {host}", icon::LOCK_SIMPLE_OPEN)).strong());
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        i18n::site_info::connection_not_secure(),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(icon::X)
                        .on_hover_text(i18n::common::close())
                        .clicked()
                    {
                        open = false;
                    }
                });
            });
            if !self.address.is_secure() {
                return;
            }
            match &self.certificates {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(i18n::site_info::reading_certificate());
                    });
                }
                Some(Err(e)) => {
                    ui.label(i18n::site_info::certificate_unavailable(e));
                }
                Some(Ok(security)) => {
                    if !security.connection.is_empty() {
                        ui.label(i18n::site_info::connection_of(&security.connection));
                    }
                    for (i, cert) in security.chain.iter().enumerate() {
                        let title = if i == 0 {
                            i18n::site_info::certificate_of(&cert.subject)
                        } else {
                            i18n::site_info::issuer_certificate_of(&cert.subject)
                        };
                        CollapsingHeader::new(title)
                            .id_salt(("certificate", i))
                            .default_open(i == 0)
                            .show(ui, |ui| details(ui, cert));
                    }
                }
            }
        });
        open
    }
}

fn details(ui: &mut egui::Ui, cert: &CertificateInfo) {
    egui::Grid::new(("certificate", &cert.sha256))
        .num_columns(2)
        .spacing([12.0, 2.0])
        .show(ui, |ui| {
            let mut row = |name: &str, value: &str| {
                ui.label(RichText::new(name).weak());
                ui.add(egui::Label::new(value).wrap().selectable(true));
                ui.end_row();
            };
            row(i18n::site_info::issued_to(), &cert.subject);
            row(i18n::site_info::issued_by(), &cert.issuer);
            if !cert.names.is_empty() {
                row(i18n::site_info::covers(), &cert.names.join(", "));
            }
            row(i18n::site_info::valid_from(), &cert.valid_from);
            row(i18n::site_info::valid_until(), &cert.valid_until);
            row(i18n::site_info::serial_number(), &cert.serial);
            row("SHA-256", &cert.sha256);
        });
    if cert.expired {
        ui.colored_label(
            ui.visuals().error_fg_color,
            i18n::site_info::certificate_expired(),
        );
    }
}
