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
                    ui.label("Connection is secure (HTTPS).");
                } else {
                    ui.label(RichText::new(format!("{} {host}", icon::LOCK_SIMPLE_OPEN)).strong());
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Connection is not secure: what you send or see can be read on the way.",
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(icon::X).on_hover_text("Close").clicked() {
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
                        ui.label("Reading the certificate…");
                    });
                }
                Some(Err(e)) => {
                    ui.label(format!("Certificate not available: {e}"));
                }
                Some(Ok(security)) => {
                    if !security.connection.is_empty() {
                        ui.label(format!("Connection: {}", security.connection));
                    }
                    for (i, cert) in security.chain.iter().enumerate() {
                        let title = if i == 0 {
                            format!("Certificate: {}", cert.subject)
                        } else {
                            format!("Issuer certificate: {}", cert.subject)
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
            row("Issued to", &cert.subject);
            row("Issued by", &cert.issuer);
            if !cert.names.is_empty() {
                row("Covers", &cert.names.join(", "));
            }
            row("Valid from", &cert.valid_from);
            row("Valid until", &cert.valid_until);
            row("Serial number", &cert.serial);
            row("SHA-256", &cert.sha256);
        });
    if cert.expired {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "This certificate is not valid at this date.",
        );
    }
}
