//! Tells the browser when a page gets the keyboard (the user went into it),
//! where the web view itself doesn't (WebKit; WebView2 has `GotFocus`): the
//! other page of a split view becomes the active one, the browser's open
//! menus close. One message per focus, not per click. Also hands it the
//! keys WebKit does nothing with: Ctrl/Cmd+F (find) and Ctrl/Cmd+P (print).

use super::MARKER;

/// What a page reports when it gets the keyboard.
pub const FOCUS_OP: &str = "page.focused";
/// What a page reports for a browser shortcut typed in it.
pub const SHORTCUT_OP: &str = "page.shortcut";

/// The script, in every frame (a click in a frame focuses that frame).
pub fn script() -> String {
    format!(
        "{MARKER}(function () {{\n\
         const ipc = window.ipc;\n\
         if (!ipc || !ipc.postMessage) return;\n\
         const post = ipc.postMessage.bind(ipc);\n\
         window.addEventListener(\"focus\", () => {{\n\
         try {{ post({}); }} catch (e) {{}}\n\
         }}, true);\n\
         window.addEventListener(\"keydown\", (e) => {{\n\
         const key = (e.key || \"\").toLowerCase();\n\
         if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || (key !== \"f\" && key !== \"p\")) return;\n\
         e.preventDefault();\n\
         try {{ post(JSON.stringify({{ op: {}, key: key }})); }} catch (err) {{}}\n\
         }}, true);\n\
         }})();\n",
        serde_json::json!(serde_json::json!({ "op": FOCUS_OP }).to_string()),
        serde_json::json!(SHORTCUT_OP)
    )
}

/// The shortcut a page reported (`f` or `p`), if `body` is one.
pub fn shortcut(body: &str) -> Option<char> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if value.get("op")?.as_str()? != SHORTCUT_OP {
        return None;
    }
    match value.get("key")?.as_str()? {
        "f" => Some('f'),
        "p" => Some('p'),
        _ => None,
    }
}

/// Whether `body` is a page saying it got the keyboard.
pub fn focused(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("op")?.as_str().map(|op| op == FOCUS_OP))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_script_reports_what_the_browser_reads() {
        let script = script();
        assert!(script.starts_with(MARKER));
        assert!(
            script.contains(r#"post("{\"op\":\"page.focused\"}")"#),
            "{script}"
        );
        assert!(focused(r#"{"op":"page.focused"}"#));
        assert!(!focused(r#"{"op":"credentials.submit"}"#));
        assert!(!focused("not json"));
        assert!(script.contains(r#"op: "page.shortcut""#), "{script}");
        assert_eq!(shortcut(r#"{"op":"page.shortcut","key":"f"}"#), Some('f'));
        assert_eq!(shortcut(r#"{"op":"page.shortcut","key":"p"}"#), Some('p'));
        assert_eq!(shortcut(r#"{"op":"page.shortcut","key":"x"}"#), None);
        assert_eq!(shortcut(r#"{"op":"page.focused"}"#), None);
    }
}
