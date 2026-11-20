//! The interface's messages in English and French, and the current language.
//!
//! # Using a message
//!
//! Each message is a function in the module of its domain (`toolbar`,
//! `settings`…). It speaks the current language, set once at startup and
//! when the user changes it:
//!
//! ```
//! i18n::set(i18n::Lang::from_locale("fr-FR"));
//! assert_eq!(i18n::toolbar::bookmark_this_page(), "Ajouter aux favoris (Ctrl+D)");
//! # i18n::set(i18n::Lang::En);
//! ```
//!
//! Every message also exists in `<module>::in_lang`, taking the language
//! first:
//!
//! ```
//! use i18n::{Lang, shield::in_lang::items_blocked};
//! assert_eq!(items_blocked(Lang::En, 1), "1 item blocked on this page.");
//! assert_eq!(items_blocked(Lang::Fr, 3), "3 éléments bloqués sur cette page.");
//! ```
//!
//! A language change applies to the current process only: the other
//! windows already open (other processes) follow it when they restart.
//!
//! # Tests
//!
//! Tests run in parallel and the language is global, so no test outside
//! this crate calls [`set`] (nor the apps' `apply_language` or
//! `set_language`, which call it). A test either calls `in_lang::` messages,
//! or runs code that builds messages inside [`with_lang`], which sets the
//! language for the current thread only:
//!
//! ```
//! use i18n::{Lang, with_lang};
//! let tip = with_lang(Lang::Fr, i18n::toolbar::bookmark_this_page);
//! assert_eq!(tip, "Ajouter aux favoris (Ctrl+D)");
//! ```
//!
//! # Adding a message
//!
//! In the module of its domain, inside its `messages!` block. Both
//! languages are required, English first: a missing one does not compile.
//! Three forms:
//!
//! ```ignore
//! messages! {
//!     /// No parameter: a `&'static str`.
//!     fn bookmark_this_page() { en: "Bookmark this page (Ctrl+D)", fr: "Ajouter aux favoris (Ctrl+D)" }
//!
//!     /// Typed parameters: a `String`, each text a `format!` string that
//!     /// names the parameters (`{n}`, `{host:?}`…).
//!     fn zoom_of(host: &str, percent: u32) { en: "{host}: {percent}%", fr: "{host} : {percent} %" }
//!
//!     /// Anything else (plurals…): give the return type, and each language
//!     /// is an expression of that type, written in a block.
//!     fn items_blocked(n: u32) -> String {
//!         en: { if n == 1 { "1 item blocked.".into() } else { format!("{n} items blocked.") } },
//!         fr: { if n <= 1 { format!("{n} élément bloqué.") } else { format!("{n} éléments bloqués.") } },
//!     }
//! }
//! ```
//!
//! Each text is a single token: a string literal, or a `{ block }` with
//! `-> Type`. Names are the message's role (`settings_pill`, not
//! `settings`), never `in_lang` nor a Rust keyword. Every module is already
//! declared below, `messages! {}` waiting in it; a new one would be a new
//! file declared at the end of this file (after the macros, which it then
//! sees). French uses a non-breaking space (`\u{a0}`) before `:` `;` `!` `?`
//! and `%`, and the typographic apostrophe `’` (`l’onglet`), never `'`
//! (which stays in code and in English).
//!
//! # Errors
//!
//! - An error enum translates in its `Display` impl: its text is made when
//!   it is shown, in the language of that moment.
//! - An error already flattened to a `String` translates where the string
//!   is built (in the current language).
//! - A message wrapping another error takes it as
//!   `e: &dyn std::fmt::Display` and prints it with `{e}`.
//!
//! # Logs
//!
//! Log literals stay in English (`log::warn!("could not read {path}: {e}")`):
//! only what the user reads goes through here. The `{e}` in a log may come
//! out translated; that is accepted.

use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

/// A language of the interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Lang {
    #[default]
    En,
    Fr,
}

impl Lang {
    /// Every language, in the order they are offered.
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Fr];

    /// The language of a system locale (`fr`, `fr-FR`, `fr_CA`…): French
    /// when it is French, English otherwise (empty included).
    pub fn from_locale(locale: &str) -> Lang {
        let primary = locale
            .trim()
            .split(['-', '_', '.', '@'])
            .next()
            .unwrap_or("");
        if primary.eq_ignore_ascii_case("fr") {
            Lang::Fr
        } else {
            Lang::En
        }
    }

    /// Its ISO 639-1 code.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Fr => "fr",
        }
    }

    /// Its name in itself, as language menus show it.
    pub fn native_name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Fr => "Français",
        }
    }

    fn from_u8(n: u8) -> Lang {
        match n {
            1 => Lang::Fr,
            _ => Lang::En,
        }
    }
}

static CURRENT: AtomicU8 = AtomicU8::new(Lang::En as u8);

thread_local! {
    /// [`with_lang`]'s language, over the global one on this thread.
    static OVERRIDE: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// Makes `lang` the language of every message from now on. Apps only: see
/// the crate's documentation for tests.
pub fn set(lang: Lang) {
    CURRENT.store(lang as u8, Ordering::Relaxed);
}

/// The language messages are in: [`with_lang`]'s on this thread, else the
/// one [`set`] (English until then).
pub fn current() -> Lang {
    OVERRIDE
        .get()
        .unwrap_or_else(|| Lang::from_u8(CURRENT.load(Ordering::Relaxed)))
}

/// Runs `f` with the messages in `lang`, on this thread only (other threads
/// keep the global language). The previous language comes back after, even
/// if `f` panics.
pub fn with_lang<R>(lang: Lang, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<Lang>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OVERRIDE.set(self.0);
        }
    }
    let _restore = Restore(OVERRIDE.replace(Some(lang)));
    f()
}

/// Declares messages: see the crate's documentation. For each one, a
/// function in the current language and the same in `in_lang`, taking the
/// language first.
macro_rules! messages {
    ($(
        $(#[$attr:meta])*
        fn $name:ident ( $($arg:ident : $ty:ty),* $(,)? ) $(-> $ret:ty)? {
            en: $en:tt,
            fr: $fr:tt $(,)?
        }
    )*) => {
        $(
            $(#[$attr])*
            pub fn $name($($arg: $ty),*) -> message!(@ret ($($arg)*) $($ret)?) {
                in_lang::$name($crate::current() $(, $arg)*)
            }
        )*

        /// The same messages, in the given language.
        pub mod in_lang {
            #[allow(unused_imports)]
            use super::*;
            $(
                $(#[$attr])*
                pub fn $name(lang: $crate::Lang $(, $arg: $ty)*) -> message!(@ret ($($arg)*) $($ret)?) {
                    match lang {
                        $crate::Lang::En => message!(@text ($($arg)*) $($ret)? ; $en),
                        $crate::Lang::Fr => message!(@text ($($arg)*) $($ret)? ; $fr),
                    }
                }
            )*
        }
    };
}

/// One message's return type and texts, by form (see [`messages!`]).
macro_rules! message {
    (@ret ($($arg:ident)*) $ret:ty) => { $ret };
    (@ret ()) => { &'static str };
    (@ret ($($arg:ident)+)) => { String };
    (@text ($($arg:ident)*) $ret:ty ; $text:tt) => { $text };
    (@text () ; $text:tt) => { $text };
    (@text ($($arg:ident)+) ; $text:tt) => { format!($text) };
}

// Desktop interface (src/ui), one module per area.
pub mod about;
pub mod bookmarks;
pub mod common;
pub mod default_browser;
pub mod downloads;
pub mod extensions;
pub mod history;
pub mod library;
pub mod notices;
pub mod omnibox;
pub mod profiles;
pub mod settings;
pub mod shield;
pub mod site_info;
pub mod sync;
pub mod tabs;
pub mod toolbar;
pub mod updates;
pub mod user_scripts;
pub mod window;

// The core's messages the user reads, by crate.
pub mod errors;
// Built in Rust for the Android app.
pub mod mobile;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn french_locales_are_french() {
        for locale in ["fr", "fr-FR", "fr_CA", "FR-be", "fr_FR.UTF-8", " fr "] {
            assert_eq!(Lang::from_locale(locale), Lang::Fr, "{locale}");
        }
    }

    #[test]
    fn other_locales_are_english() {
        for locale in ["", "en-US", "de", "de-DE", "fry", "f", "es_FR"] {
            assert_eq!(Lang::from_locale(locale), Lang::En, "{locale:?}");
        }
    }

    // The only test touching the global language: the others hold their
    // own with `with_lang` or `in_lang`.
    #[test]
    fn the_current_language_is_the_one_set() {
        set(Lang::Fr);
        assert_eq!(current(), Lang::Fr);
        assert_eq!(
            toolbar::bookmark_this_page(),
            "Ajouter aux favoris (Ctrl+D)"
        );
        set(Lang::En);
        assert_eq!(current(), Lang::En);
        assert_eq!(toolbar::bookmark_this_page(), "Bookmark this page (Ctrl+D)");
    }

    #[test]
    fn with_lang_holds_on_this_thread_only() {
        with_lang(Lang::Fr, || {
            assert_eq!(current(), Lang::Fr);
            assert_eq!(library::settings_pill(), "Réglages");
            with_lang(Lang::En, || assert_eq!(current(), Lang::En));
            assert_eq!(current(), Lang::Fr);
            let other = std::thread::spawn(|| OVERRIDE.get()).join().unwrap();
            assert_eq!(other, None);
        });
        assert_eq!(OVERRIDE.get(), None);
    }

    #[test]
    fn with_lang_restores_after_a_panic() {
        let r = std::panic::catch_unwind(|| with_lang(Lang::Fr, || panic!("boom")));
        assert!(r.is_err());
        assert_eq!(OVERRIDE.get(), None);
        with_lang(Lang::En, || {
            let r = std::panic::catch_unwind(|| with_lang(Lang::Fr, || panic!("boom")));
            assert!(r.is_err());
            assert_eq!(current(), Lang::En);
        });
    }

    #[test]
    fn messages_take_parameters() {
        use settings::in_lang::zoom_of;
        assert_eq!(zoom_of(Lang::En, "example.com", 125), "example.com: 125%");
        assert_eq!(
            zoom_of(Lang::Fr, "example.com", 125),
            "example.com\u{a0}: 125\u{a0}%"
        );
    }

    #[test]
    fn plurals_are_written_by_hand() {
        use shield::in_lang::items_blocked;
        assert_eq!(items_blocked(Lang::En, 1), "1 item blocked on this page.");
        assert_eq!(items_blocked(Lang::En, 2), "2 items blocked on this page.");
        assert_eq!(
            items_blocked(Lang::Fr, 1),
            "1 élément bloqué sur cette page."
        );
        assert_eq!(
            items_blocked(Lang::Fr, 2),
            "2 éléments bloqués sur cette page."
        );
    }

    #[test]
    fn languages_are_named_in_themselves() {
        assert_eq!(Lang::ALL.map(Lang::native_name), ["English", "Français"]);
        assert_eq!(Lang::Fr.code(), "fr");
    }
}
