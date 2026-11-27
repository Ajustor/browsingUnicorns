//! The password manager's page scripts (`credentials.js`): one injected in
//! every page, that reports the sign-in forms sent, and one run in a page
//! to fill its sign-in form.
//!
//! A report carries a token of the run that only the reporting script has,
//! and that script only uses it in the top frame: a frame of the page (an
//! ad's) cannot offer to save a password for the site it sits in (the
//! browser takes the tab's address for it), on any platform. The page
//! itself could take it, but only to offer a password for its own site,
//! which a form sent does anyway.

use std::sync::OnceLock;

use serde_json::json;

use super::{Bridge, MARKER};
use crate::script_storage::new_token;

const SCRIPT: &str = include_str!("credentials.js");

/// What a page reports when a sign-in form is sent.
pub const SUBMIT_OP: &str = "credentials.submit";

/// The token of the reporting script, for this run.
fn token() -> &'static str {
    static TOKEN: OnceLock<String> = OnceLock::new();
    TOKEN.get_or_init(new_token)
}

/// The script that reports the sign-ins sent from the top frame.
pub fn capture_script(bridge: Bridge) -> String {
    format!(
        "{MARKER}(function () {{\nconst BRIDGE = {};\nconst TOKEN = {};\n{SCRIPT}\ncaptureSignIns();\n}})();\n",
        json!(bridge.name()),
        json!(token()),
    )
}

/// The script that fills the page's sign-in form, if it is at `origin`.
pub fn fill_script(origin: &str, username: &str, password: &str) -> String {
    format!(
        "(function () {{\nconst BRIDGE = null;\nconst TOKEN = null;\n{SCRIPT}\nfillSignIn({}, {}, {});\n}})();\n",
        json!(origin),
        json!(username),
        json!(password)
    )
}

/// Whether `body` is a sign-in report, with or without the right token:
/// either way, no other handler should take it.
pub fn is_report(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("op")?.as_str().map(|op| op == SUBMIT_OP))
        .unwrap_or(false)
}

/// A sign-in the reporting script reported: (user name, password); `None`
/// for any other message, or one without its token.
pub fn submitted(body: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if value.get("op")?.as_str()? != SUBMIT_OP || value.get("t")?.as_str()? != token() {
        return None;
    }
    let field = |name| value.get(name).and_then(|v| v.as_str()).unwrap_or("");
    Some((field("username").to_owned(), field("password").to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_embed_their_values_as_json() {
        let fill = fill_script("https://a.com", "me\"", "p</script>w");
        assert!(fill.contains(r#"fillSignIn("https://a.com", "me\"", "p</script>w");"#));
        assert!(!fill.contains(token()), "only the reporting script has it");
        assert!(capture_script(Bridge::Android).contains("const BRIDGE = \"android\";"));
        assert!(capture_script(Bridge::Ipc).contains("captureSignIns();"));
        assert!(capture_script(Bridge::Ipc).contains(&format!("const TOKEN = \"{}\";", token())));
    }

    #[test]
    fn submitted_sign_ins_are_read_from_their_message() {
        let t = token();
        assert_eq!(
            submitted(&format!(
                r#"{{"t":"{t}","op":"credentials.submit","username":"me","password":"pw"}}"#
            )),
            Some(("me".into(), "pw".into()))
        );
        assert_eq!(
            submitted(&format!(
                r#"{{"t":"{t}","op":"credentials.submit","password":"pw"}}"#
            )),
            Some((String::new(), "pw".into()))
        );
        assert_eq!(submitted(r#"{"op":"storage.get"}"#), None);
        assert_eq!(submitted("nope"), None);
    }

    #[test]
    fn a_report_without_the_token_is_refused() {
        // A frame's (or any page's) forged report.
        for forged in [
            r#"{"op":"credentials.submit","username":"me","password":"pw"}"#,
            r#"{"t":"guess","op":"credentials.submit","username":"me","password":"pw"}"#,
            r#"{"t":null,"op":"credentials.submit","password":"pw"}"#,
        ] {
            assert_eq!(submitted(forged), None, "{forged}");
            assert!(is_report(forged));
        }
        assert!(!is_report(r#"{"op":"get"}"#));
    }
}
