//! The password manager's page scripts (`credentials.js`): one injected in
//! every page, that reports the sign-in forms sent, and one run in a page
//! to fill its sign-in form.

use serde_json::json;

use super::{Bridge, MARKER};

const SCRIPT: &str = include_str!("credentials.js");

/// What a page reports when a sign-in form is sent.
pub const SUBMIT_OP: &str = "credentials.submit";

/// The script that reports the sign-ins sent from the top frame.
pub fn capture_script(bridge: Bridge) -> String {
    format!(
        "{MARKER}(function () {{\nconst BRIDGE = {};\n{SCRIPT}\ncaptureSignIns();\n}})();\n",
        json!(bridge.name())
    )
}

/// The script that fills the page's sign-in form, if it is at `origin`.
pub fn fill_script(origin: &str, username: &str, password: &str) -> String {
    format!(
        "(function () {{\nconst BRIDGE = null;\n{SCRIPT}\nfillSignIn({}, {}, {});\n}})();\n",
        json!(origin),
        json!(username),
        json!(password)
    )
}

/// A sign-in a page reported: (user name, password); `None` for any
/// other message.
pub fn submitted(body: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    if value.get("op")?.as_str()? != SUBMIT_OP {
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
        assert!(capture_script(Bridge::Android).contains("const BRIDGE = \"android\";"));
        assert!(capture_script(Bridge::Ipc).contains("captureSignIns();"));
    }

    #[test]
    fn submitted_sign_ins_are_read_from_their_message() {
        assert_eq!(
            submitted(r#"{"op":"credentials.submit","username":"me","password":"pw"}"#),
            Some(("me".into(), "pw".into()))
        );
        assert_eq!(
            submitted(r#"{"op":"credentials.submit","password":"pw"}"#),
            Some((String::new(), "pw".into()))
        );
        assert_eq!(submitted(r#"{"op":"storage.get"}"#), None);
        assert_eq!(submitted("nope"), None);
    }
}
