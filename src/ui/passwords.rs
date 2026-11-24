//! The password manager: the bar that offers to save a sign-in typed in a
//! page, and the saved passwords in the Settings view.

use application::{CredentialAnswer, PendingCredential};
use domain::Credentials;
use egui::{Button, RichText, TopBottomPanel};
use egui_phosphor::regular as icon;

use super::command::Command;

/// `https://a.com:8443` → `a.com:8443`.
pub fn site(origin: &str) -> &str {
    origin.split_once("://").map_or(origin, |(_, rest)| rest)
}

/// The bar under the toolbar while a sign-in waits for an answer.
pub fn banner(
    ctx: &egui::Context,
    pending: Option<&PendingCredential>,
    commands: &mut Vec<Command>,
) {
    let Some(p) = pending else {
        return;
    };
    let site = site(&p.origin);
    let question = if p.update {
        i18n::passwords::update_offer(&p.username, site)
    } else if p.username.is_empty() {
        i18n::passwords::save_offer(site)
    } else {
        i18n::passwords::save_offer_for(&p.username, site)
    };
    TopBottomPanel::top("password_offer").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{} {question}", icon::KEY))
                .on_hover_text(i18n::passwords::synced_tip());
            let mut answer = |label: &str, answer| {
                if ui.button(label).clicked() {
                    commands.push(Command::AnswerCredential(answer));
                }
            };
            let save = if p.update {
                i18n::passwords::update()
            } else {
                i18n::passwords::save()
            };
            answer(save, CredentialAnswer::Save);
            answer(i18n::passwords::not_now(), CredentialAnswer::NotNow);
            if !p.update {
                answer(i18n::passwords::never(), CredentialAnswer::Never);
            }
        });
    });
}

/// The saved passwords (and the sites never saved), each with a button to
/// copy its password and one to delete it.
pub fn list(ui: &mut egui::Ui, credentials: &Credentials, commands: &mut Vec<Command>) {
    ui.label(RichText::new(i18n::passwords::saved()).strong());
    let mut items: Vec<_> = credentials.items().iter().collect();
    items.sort_by(|a, b| (site(&a.origin), &a.username).cmp(&(site(&b.origin), &b.username)));
    if items.is_empty() {
        ui.weak(i18n::passwords::none_saved());
        return;
    }
    for c in items {
        ui.horizontal(|ui| {
            if super::theme::hover_fill(ui, |ui| ui.add(Button::new(icon::X).frame(false).small()))
                .on_hover_text(i18n::passwords::remove_tip())
                .clicked()
            {
                commands.push(Command::RemoveCredential(
                    c.origin.clone(),
                    c.username.clone(),
                ));
            }
            if !c.never
                && super::theme::hover_fill(ui, |ui| {
                    ui.add(Button::new(icon::COPY).frame(false).small())
                })
                .on_hover_text(i18n::passwords::copy_tip())
                .clicked()
            {
                ui.ctx().copy_text(c.password.clone());
            }
            let who = if c.never {
                RichText::new(i18n::passwords::never_saved()).weak()
            } else if c.username.is_empty() {
                RichText::new(i18n::passwords::no_username()).weak()
            } else {
                RichText::new(&c.username)
            };
            ui.label(site(&c.origin));
            ui.label(who);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_drop_the_scheme() {
        assert_eq!(site("https://a.com"), "a.com");
        assert_eq!(site("http://localhost:3000"), "localhost:3000");
        assert_eq!(site("weird"), "weird");
    }
}
