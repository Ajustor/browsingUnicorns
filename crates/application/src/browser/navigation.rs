//! The active page: what is typed in the address bar, back and forward,
//! reload, find and print.

use domain::resolve_omnibox;

use super::{Browser, Target};
use crate::BrowserError;
use crate::ports::{Viewport, WebEngine};

impl<E: WebEngine> Browser<E> {
    /// Prints the active tab's page.
    pub fn print(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.engine.print(id);
        }
    }

    /// Finds `query` in the active tab's page and selects it (the next one
    /// after the selection, or before it when `backwards`); `restart`: from
    /// the top. Empty: the selection goes.
    pub fn find_in_page(&mut self, query: &str, backwards: bool, restart: bool) {
        let Some(id) = self.session.active_id() else {
            return;
        };
        let clear = "window.getSelection && window.getSelection().removeAllRanges();";
        let script = if query.is_empty() {
            clear.to_owned()
        } else {
            let query = js_string(query);
            let restart = if restart { clear } else { "" };
            format!("{restart}window.find({query}, false, {backwards}, true);")
        };
        self.engine.run_in_tab(id, &script);
    }

    /// Loads what was typed in the address bar in the active tab (in a new
    /// one when there is none). Returns `false` when there was nothing to load.
    pub fn submit_omnibox(&mut self, input: &str) -> Result<bool, BrowserError> {
        self.submit(input, Target::ActiveTab)
    }

    /// Loads typed input (address, host name or search) in `target`.
    /// Returns `false` when there was nothing to load.
    pub fn submit(&mut self, input: &str, target: Target) -> Result<bool, BrowserError> {
        let Some(address) = resolve_omnibox(input, &self.settings.search_engine) else {
            return Ok(false);
        };
        self.load(address, target)?;
        Ok(true)
    }

    pub fn go_home(&mut self) -> Result<(), BrowserError> {
        self.load(self.settings.home_page.clone(), Target::ActiveTab)
    }

    pub fn go_back(&mut self) {
        if let Some(tab) = self.session.active()
            && tab.history().can_go_back()
        {
            self.engine.go_back(tab.id());
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(tab) = self.session.active()
            && tab.history().can_go_forward()
        {
            self.engine.go_forward(tab.id());
        }
    }

    pub fn reload(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.reload_tab(id);
        }
    }

    /// Reloads the active tab without the cache (Ctrl+Shift+R).
    pub fn reload_bypassing_cache(&mut self) {
        if let Some(id) = self.session.active_id() {
            self.engine.reload_bypassing_cache(id);
        }
    }

    pub fn set_viewport(&mut self, viewport: Viewport) {
        self.engine.set_viewport(viewport);
    }

    /// See [`WebEngine::set_page_holes`].
    pub fn set_page_holes(&mut self, holes: &[Viewport]) {
        self.engine.set_page_holes(holes);
    }
}

/// `text` as a JavaScript string literal.
fn js_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    #[test]
    fn omnibox_navigates_the_active_tab() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        take_calls(&mut b);
        assert!(b.submit_omnibox("rust-lang.org").unwrap());
        assert!(!b.submit_omnibox("  ").unwrap());
        assert_eq!(
            take_calls(&mut b),
            vec![Call::Navigate(id.get(), "https://rust-lang.org/".into())]
        );
    }

    #[test]
    fn omnibox_without_tabs_opens_one() {
        let mut b = browser();
        b.submit_omnibox("hello").unwrap();
        assert_eq!(b.session().len(), 1);
    }

    #[test]
    fn back_and_forward_follow_the_mirrored_history() {
        let mut b = browser();
        let id = b.open_tab(None).unwrap();
        b.go_back();
        b.go_forward();
        take_calls(&mut b);
        for url in ["https://a.com/", "https://b.com/"] {
            b.handle(EngineEvent::LoadFinished {
                tab: id,
                address: addr(url),
            })
            .unwrap();
        }
        b.go_back();
        b.go_forward();
        assert_eq!(take_calls(&mut b), vec![Call::Back(id.get())]);
    }

    #[test]
    fn cycling_and_reload_target_the_active_tab() {
        let mut b = browser();
        let first = b.open_tab(None).unwrap();
        let second = b.open_tab(None).unwrap();
        b.cycle_tabs(1);
        assert_eq!(b.session().active_id(), Some(first));
        take_calls(&mut b);
        b.reload();
        b.go_home().unwrap();
        b.set_viewport(Viewport::default());
        assert_eq!(
            take_calls(&mut b),
            vec![
                Call::Reload(first.get()),
                Call::Navigate(first.get(), "https://duckduckgo.com/".into()),
                Call::Viewport(Viewport::default()),
            ]
        );
        assert_ne!(first, second);
    }

    #[test]
    fn searched_text_is_quoted_for_the_page() {
        assert_eq!(js_string(r#"a"b\c"#), r#""a\"b\\c""#);
        // A line break and U+2028 (a line break to JavaScript): escaped.
        let line_breaks = format!("x{}y{}", '\n', '\u{2028}');
        let backslash = '\\';
        let expected = format!("\"x{backslash}ny{backslash}u2028\"");
        assert_eq!(js_string(&line_breaks), expected);
        assert_eq!(js_string("\u{1}"), r#""\u0001""#);
    }

    #[test]
    fn find_and_print_reach_the_shown_page() {
        let mut b = browser();
        let t = b.open_tab(None).unwrap().get();
        take_calls(&mut b);
        b.find_in_page("rust", false, true);
        b.find_in_page("rust", true, false);
        b.find_in_page("", false, false);
        b.print();
        let calls = take_calls(&mut b);
        let scripts: Vec<&String> = calls
            .iter()
            .filter_map(|c| match c {
                Call::Script(id, s) if *id == t => Some(s),
                _ => None,
            })
            .collect();
        assert!(scripts[0].ends_with(r#"window.find("rust", false, false, true);"#));
        assert!(scripts[0].starts_with("window.getSelection"));
        assert_eq!(scripts[1], r#"window.find("rust", false, true, true);"#);
        assert!(!scripts[2].contains("find("));
        assert_eq!(calls.last(), Some(&Call::Print(t)));
    }
}
