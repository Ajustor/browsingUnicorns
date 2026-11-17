use egui::{Key, KeyboardShortcut, Modifiers};
use infrastructure::KeyPress;

use super::command::Command;
use super::library::LibraryView;

/// Browser shortcuts. They reach egui while the chrome has the keyboard
/// focus; on Windows the engine also takes them from the page that has it
/// ([`page_keys`], [`from_page`]).
const SHORTCUTS: &[(Modifiers, Key, Command)] = &[
    (Modifiers::COMMAND, Key::T, Command::NewTab),
    (
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::N,
        Command::OpenPrivateWindow,
    ),
    (Modifiers::COMMAND, Key::W, Command::CloseActiveTab),
    (
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::Tab,
        Command::CycleTabs(-1),
    ),
    (Modifiers::COMMAND, Key::Tab, Command::CycleTabs(1)),
    (Modifiers::COMMAND, Key::L, Command::FocusOmnibox),
    (Modifiers::NONE, Key::F6, Command::FocusOmnibox),
    // Before Ctrl+R and F5, which match with Shift or Ctrl held too.
    (
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::R,
        Command::HardReload,
    ),
    (Modifiers::COMMAND, Key::F5, Command::HardReload),
    (Modifiers::SHIFT, Key::F5, Command::HardReload),
    (Modifiers::COMMAND, Key::R, Command::Reload),
    (Modifiers::NONE, Key::F5, Command::Reload),
    (Modifiers::ALT, Key::ArrowLeft, Command::Back),
    (Modifiers::ALT, Key::ArrowRight, Command::Forward),
    (Modifiers::ALT, Key::Home, Command::Home),
    (Modifiers::COMMAND, Key::D, Command::ToggleBookmark),
    (Modifiers::NONE, Key::F12, Command::OpenDevTools),
    (
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::I,
        Command::OpenDevTools,
    ),
    (
        Modifiers::COMMAND.plus(Modifiers::ALT),
        Key::I,
        Command::OpenDevTools,
    ),
    (
        Modifiers::COMMAND,
        Key::H,
        Command::ToggleLibrary(LibraryView::History),
    ),
    (
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::O,
        Command::ToggleLibrary(LibraryView::Bookmarks),
    ),
];

/// A shortcut as the engine sees a key press (Ctrl is Command off macOS).
fn key_press(modifiers: Modifiers, key: Key) -> KeyPress {
    KeyPress {
        key: key.name().to_owned(),
        ctrl: modifiers.command || modifiers.ctrl,
        shift: modifiers.shift,
        alt: modifiers.alt,
    }
}

/// The keys the engine takes from the pages.
pub fn page_keys() -> Vec<KeyPress> {
    SHORTCUTS
        .iter()
        .map(|(m, k, _)| key_press(*m, *k))
        .collect()
}

/// The command of a shortcut pressed in a page.
pub fn from_page(press: &KeyPress) -> Option<Command> {
    SHORTCUTS
        .iter()
        .find(|(m, k, _)| key_press(*m, *k) == *press)
        .map(|(.., command)| command.clone())
}

pub fn collect(ctx: &egui::Context, commands: &mut Vec<Command>) {
    ctx.input_mut(|input| {
        for (modifiers, key, command) in SHORTCUTS {
            if input.consume_shortcut(&KeyboardShortcut::new(*modifiers, *key)) {
                commands.push(command.clone());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str, ctrl: bool, shift: bool, alt: bool) -> KeyPress {
        KeyPress {
            key: key.into(),
            ctrl,
            shift,
            alt,
        }
    }

    #[test]
    fn page_keys_map_to_their_commands() {
        assert!(matches!(
            from_page(&press("T", true, false, false)),
            Some(Command::NewTab)
        ));
        assert!(matches!(
            from_page(&press("Tab", true, true, false)),
            Some(Command::CycleTabs(-1))
        ));
        assert!(matches!(
            from_page(&press("Left", false, false, true)),
            Some(Command::Back)
        ));
        assert!(matches!(
            from_page(&press("F5", false, false, false)),
            Some(Command::Reload)
        ));
        assert!(matches!(
            from_page(&press("R", true, true, false)),
            Some(Command::HardReload)
        ));
        assert!(matches!(
            from_page(&press("F5", true, false, false)),
            Some(Command::HardReload)
        ));
        assert!(matches!(
            from_page(&press("N", true, true, false)),
            Some(Command::OpenPrivateWindow)
        ));
        // A page's own keys stay with it.
        assert!(from_page(&press("C", true, false, false)).is_none());
        assert!(from_page(&press("T", false, false, false)).is_none());
    }
}
