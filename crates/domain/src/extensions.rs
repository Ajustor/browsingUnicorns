//! Browser extensions installed by the user (Chrome extensions, which the
//! Windows web view can run).

use std::fmt;

use crate::WebAddress;

/// A Chrome extension id: 32 letters from `a` to `p`, derived from the
/// extension's public key. The same id in the Chrome Web Store and once
/// installed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionId(String);

impl ExtensionId {
    pub fn parse(id: &str) -> Option<Self> {
        let id = id.trim().to_ascii_lowercase();
        (id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b))).then_some(Self(id))
    }

    /// An id, or a Chrome Web Store or Edge Add-ons link
    /// (`https://chromewebstore.google.com/detail/<name>/<id>`,
    /// `https://microsoftedge.microsoft.com/addons/detail/<name>/<id>`).
    pub fn from_store_input(input: &str) -> Option<Self> {
        let input = input.trim();
        Self::parse(input).or_else(|| {
            let path = input.split(['?', '#']).next()?;
            path.split('/').rev().find_map(Self::parse)
        })
    }

    /// The extension a download is the store package of (what a store's
    /// "Add to Chrome" button fetches): a `.crx` file whose name or address
    /// names its id.
    pub fn of_store_package(url: &str, file_name: &str) -> Option<Self> {
        if !file_name.to_ascii_lowercase().ends_with(".crx") {
            return None;
        }
        // Ids are lowercase: `id%3D<id>%26` splits around them.
        let named = |text: &str| {
            text.split(|c: char| !c.is_ascii_lowercase())
                .find_map(Self::parse)
        };
        named(file_name).or_else(|| named(url))
    }

    /// The id of the extension signed with `public_key` (DER): the first
    /// 16 bytes of its SHA-256, each nibble written as a letter `a`..`p`.
    pub fn from_key_hash(sha256: &[u8]) -> Self {
        Self(
            sha256
                .iter()
                .take(16)
                .flat_map(|b| [b >> 4, b & 0x0f])
                .map(|n| char::from(b'a' + n))
                .collect(),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ExtensionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extension {
    pub id: ExtensionId,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    /// The toolbar popup's page, relative to the extension's root.
    pub popup: Option<String>,
    /// The options page, relative to the extension's root.
    pub options: Option<String>,
}

/// Whether extension version `candidate` comes after `installed` (Chrome's
/// versions: up to four dot-separated numbers, `1.2` = `1.2.0.0`).
pub fn is_newer_version(candidate: &str, installed: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        let mut p: Vec<u64> = v
            .trim()
            .split('.')
            .map(|n| n.trim().parse().unwrap_or(0))
            .collect();
        p.resize(4, 0);
        p
    };
    parts(candidate) > parts(installed)
}

impl Extension {
    /// `chrome-extension://<id>/<path>`.
    fn page(&self, path: &str) -> Option<WebAddress> {
        let path = path.trim_start_matches('/');
        WebAddress::parse(&format!("chrome-extension://{}/{path}", self.id)).ok()
    }

    pub fn popup_page(&self) -> Option<WebAddress> {
        self.popup.as_deref().and_then(|p| self.page(p))
    }

    pub fn options_page(&self) -> Option<WebAddress> {
        self.options.as_deref().and_then(|p| self.page(p))
    }
}

/// The installed extensions, in installation order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extensions {
    items: Vec<Extension>,
}

impl Extensions {
    pub fn from_items(items: Vec<Extension>) -> Self {
        let mut extensions = Self::default();
        for e in items {
            extensions.install(e);
        }
        extensions
    }

    pub fn items(&self) -> &[Extension] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn get(&self, id: &ExtensionId) -> Option<&Extension> {
        self.items.iter().find(|e| &e.id == id)
    }

    /// Adds `extension`, or updates it in place when already installed
    /// (an update keeps it enabled or disabled as it was).
    pub fn install(&mut self, mut extension: Extension) {
        match self.items.iter_mut().find(|e| e.id == extension.id) {
            Some(existing) => {
                extension.enabled = existing.enabled;
                *existing = extension;
            }
            None => self.items.push(extension),
        }
    }

    /// Whether it was installed.
    pub fn remove(&mut self, id: &ExtensionId) -> bool {
        let before = self.items.len();
        self.items.retain(|e| &e.id != id);
        self.items.len() != before
    }

    /// Whether it changed.
    pub fn set_enabled(&mut self, id: &ExtensionId, enabled: bool) -> bool {
        match self.items.iter_mut().find(|e| &e.id == id) {
            Some(e) if e.enabled != enabled => {
                e.enabled = enabled;
                true
            }
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_number_by_number() {
        assert!(is_newer_version("1.17.651", "1.15.2"));
        assert!(is_newer_version("1.10", "1.9.9"));
        assert!(is_newer_version("2", "1.99.99.99"));
        assert!(!is_newer_version("1.2", "1.2.0.0"));
        assert!(!is_newer_version("1.1", "1.2"));
    }

    const UBO_LITE: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

    fn ext(id: &str, name: &str) -> Extension {
        Extension {
            id: ExtensionId::parse(id).unwrap(),
            name: name.into(),
            version: "1.0".into(),
            description: String::new(),
            enabled: true,
            popup: Some("popup.html".into()),
            options: None,
        }
    }

    #[test]
    fn ids_are_32_letters_a_to_p() {
        assert!(ExtensionId::parse(UBO_LITE).is_some());
        assert_eq!(
            ExtensionId::parse(&UBO_LITE.to_uppercase())
                .unwrap()
                .as_str(),
            UBO_LITE
        );
        assert!(ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfz").is_none());
        assert!(ExtensionId::parse("abc").is_none());
    }

    #[test]
    fn ids_from_store_links() {
        for input in [
            format!("https://chromewebstore.google.com/detail/ublock-origin-lite/{UBO_LITE}"),
            format!("https://chromewebstore.google.com/detail/ublock-origin-lite/{UBO_LITE}?hl=fr"),
            format!("https://chrome.google.com/webstore/detail/x/{UBO_LITE}/"),
            format!("https://microsoftedge.microsoft.com/addons/detail/x/{UBO_LITE}"),
            format!(
                "https://chromewebstore.google.com/detail/heylogin-%E2%80%93-password-manag/{UBO_LITE}"
            ),
            format!("  {UBO_LITE} "),
        ] {
            assert_eq!(
                ExtensionId::from_store_input(&input).unwrap().as_str(),
                UBO_LITE,
                "{input}"
            );
        }
        assert!(ExtensionId::from_store_input("https://example.com/").is_none());
    }

    #[test]
    fn store_packages_name_their_extension() {
        let store = "https://clients2.google.com/service/update2/crx?response=redirect\
                     &x=id%3Dddkjiahejlhfcafbddmgiahcphecmpfh%26installsource%3Dondemand%26uc";
        let blob = "https://clients2.googleusercontent.com/crx/blobs/AV8Xwo6/x.crx";
        let id = |url: &str, file: &str| ExtensionId::of_store_package(url, file).map(|i| i.0);
        // Named by the file, or by the address.
        assert_eq!(
            id(blob, &format!("{UBO_LITE}_2025_1_0.crx")).as_deref(),
            Some(UBO_LITE)
        );
        assert_eq!(id(store, "extension.crx").as_deref(), Some(UBO_LITE));
        // Not a package, or no id: an ordinary download.
        assert_eq!(id(store, "notes.txt"), None);
        assert_eq!(id(blob, "extension.crx"), None);
    }

    #[test]
    fn id_from_key_hash() {
        let hash = [0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef].repeat(4);
        assert_eq!(
            ExtensionId::from_key_hash(&hash).as_str(),
            "abcdefghijklmnopabcdefghijklmnop"
        );
    }

    #[test]
    fn pages_are_extension_addresses() {
        let e = ext(UBO_LITE, "uBO Lite");
        assert_eq!(
            e.popup_page().unwrap().as_str(),
            format!("chrome-extension://{UBO_LITE}/popup.html")
        );
        assert!(e.options_page().is_none());
    }

    #[test]
    fn updates_keep_the_enabled_state() {
        let mut list = Extensions::default();
        list.install(ext(UBO_LITE, "v1"));
        let id = list.items()[0].id.clone();
        assert!(list.set_enabled(&id, false));
        assert!(!list.set_enabled(&id, false));
        list.install(ext(UBO_LITE, "v2"));
        assert_eq!(list.items().len(), 1);
        assert_eq!(list.items()[0].name, "v2");
        assert!(!list.items()[0].enabled);
        assert!(list.remove(&id));
        assert!(!list.remove(&id));
        assert!(list.is_empty());
    }
}
