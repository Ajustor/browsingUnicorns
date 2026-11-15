use egui::{Key, KeyboardShortcut, Modifiers};

use super::command::Command;
use super::library::LibraryView;

/// Browser shortcuts. They only reach egui while the chrome has the keyboard
/// focus: a page that has it keeps its keys.
const SHORTCUTS: &[(Modifiers, Key, Command)] = &[
    (Modifiers::COMMAND, Key::T, Command::NewTab),
    (Modifiers::COMMAND, Key::W, Command::CloseActiveTab),
    (
        Modifiers::COMMAND.plus(Modifiers::SHIFT),
        Key::Tab,
        Command::CycleTabs(-1),
    ),
    (Modifiers::COMMAND, Key::Tab, Command::CycleTabs(1)),
    (Modifiers::COMMAND, Key::L, Command::FocusOmnibox),
    (Modifiers::NONE, Key::F6, Command::FocusOmnibox),
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

pub fn collect(ctx: &egui::Context, commands: &mut Vec<Command>) {
    ctx.input_mut(|input| {
        for (modifiers, key, command) in SHORTCUTS {
            if input.consume_shortcut(&KeyboardShortcut::new(*modifiers, *key)) {
                commands.push(command.clone());
            }
        }
    });
}
