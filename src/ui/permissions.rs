//! What pages ask to use (camera, microphone, location...), where the web
//! view doesn't ask the user itself (WebKitGTK): a bar under the toolbar,
//! for the tab shown.

use application::ports::PermissionKind;
use domain::TabId;
use egui::TopBottomPanel;
use egui_phosphor::regular as icon;

use super::command::Command;

/// A page's request, waiting for the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ask {
    pub id: u64,
    pub tab: TabId,
    pub host: String,
    pub kind: PermissionKind,
}

fn what(kind: PermissionKind) -> (&'static str, &'static str) {
    match kind {
        PermissionKind::Camera => (icon::VIDEO_CAMERA, i18n::permissions::camera()),
        PermissionKind::Microphone => (icon::MICROPHONE, i18n::permissions::microphone()),
        PermissionKind::CameraAndMicrophone => (
            icon::VIDEO_CAMERA,
            i18n::permissions::camera_and_microphone(),
        ),
        PermissionKind::Screen => (icon::MONITOR, i18n::permissions::screen()),
        PermissionKind::Location => (icon::MAP_PIN, i18n::permissions::location()),
        PermissionKind::Notifications => (icon::BELL, i18n::permissions::notifications()),
    }
}

/// The bar for `ask` (the shown tab's first request), if any.
pub fn bar(ctx: &egui::Context, ask: Option<&Ask>, commands: &mut Vec<Command>) {
    let Some(ask) = ask else {
        return;
    };
    let (glyph, what) = what(ask.kind);
    TopBottomPanel::top("permission_ask").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "{glyph} {}",
                i18n::permissions::ask(&ask.host, &what)
            ))
            .on_hover_text(i18n::permissions::until_closed());
            if ui.button(i18n::permissions::allow()).clicked() {
                commands.push(Command::AnswerPermission(ask.id, true));
            }
            if ui.button(i18n::permissions::deny()).clicked() {
                commands.push(Command::AnswerPermission(ask.id, false));
            }
        });
    });
}
