//! The installed extensions' manifests, parsed once and shared by everything
//! that reads them: the extension server, the hub, the page scripts and the
//! extension pages' `fetch`.
//!
//! An extension is parsed the first time something asks for it (its
//! default-locale messages the first time they are needed), then kept until
//! the installed extensions change ([`ExtensionCatalog::installed_changed`]):
//! nothing is read from disk again in between.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use domain::ExtensionId;
use serde_json::{Map, Value};

use super::manifest::{self, Background, ContentScript, ManifestInfo};
use super::{dnr, extension_dir};

/// One installed extension, as its manifest describes it.
#[derive(Debug)]
pub struct ParsedExtension {
    dir: PathBuf,
    manifest: Map<String, Value>,
    messages: OnceLock<Map<String, Value>>,
}

impl ParsedExtension {
    fn load(dir: PathBuf) -> Option<Self> {
        let manifest = manifest::load(&dir)
            .inspect_err(|e| log::debug!("extensions: {}: {e}", dir.display()))
            .ok()?;
        Some(Self {
            dir,
            manifest,
            messages: OnceLock::new(),
        })
    }

    /// The whole `manifest.json`.
    pub fn manifest(&self) -> &Map<String, Value> {
        &self.manifest
    }

    /// The default locale's messages, keys in lower case
    /// (`chrome.i18n.getMessage`): read the first time they are asked for.
    pub fn messages(&self) -> &Map<String, Value> {
        self.messages
            .get_or_init(|| manifest::messages(&self.dir, &self.manifest))
    }

    /// Name (localized), version...: `Err` when the manifest has no name.
    pub fn info(&self) -> Result<ManifestInfo, String> {
        manifest::info(&self.manifest, self.messages())
    }

    pub fn background(&self) -> Option<Background> {
        manifest::background(&self.manifest)
    }

    /// The content scripts the manifest declares.
    pub fn content_scripts(&self) -> Vec<ContentScript> {
        manifest::content_scripts(&self.manifest)
    }

    pub fn has_permission(&self, permission: &str) -> bool {
        manifest::has_permission(&self.manifest, permission)
    }

    /// The ids of the `declarativeNetRequest` rulesets enabled by default.
    pub fn default_rulesets(&self) -> Vec<String> {
        dnr::default_rulesets(&self.manifest)
    }

    /// The ids of all the `declarativeNetRequest` rulesets.
    pub fn all_rulesets(&self) -> Vec<String> {
        dnr::all_rulesets(&self.manifest)
    }
}

/// The parsed extensions unpacked in one folder.
#[derive(Debug)]
pub struct ExtensionCatalog {
    dir: PathBuf,
    /// `None`: no readable manifest (asked again once the installed
    /// extensions change).
    parsed: Mutex<HashMap<ExtensionId, Option<Arc<ParsedExtension>>>>,
}

/// The catalogues alive, by folder: [`ExtensionCatalog::for_dir`].
static CATALOGS: Mutex<Vec<(PathBuf, Weak<ExtensionCatalog>)>> = Mutex::new(Vec::new());

impl ExtensionCatalog {
    /// The catalogue of the extensions unpacked in `dir`: the same one for
    /// everyone asking with the same folder while one of them holds it, so
    /// the server, the hub and the page scripts parse each manifest once.
    pub fn for_dir(dir: &Path) -> Arc<Self> {
        let mut all = CATALOGS.lock().unwrap_or_else(|p| p.into_inner());
        all.retain(|(_, c)| c.strong_count() > 0);
        if let Some(catalog) = all
            .iter()
            .find(|(d, _)| d == dir)
            .and_then(|(_, c)| c.upgrade())
        {
            return catalog;
        }
        let catalog = Arc::new(Self {
            dir: dir.to_owned(),
            parsed: Mutex::default(),
        });
        all.push((dir.to_owned(), Arc::downgrade(&catalog)));
        catalog
    }

    /// Extension `id`, parsed (`None`: not installed, or its manifest is
    /// not readable).
    pub fn get(&self, id: &ExtensionId) -> Option<Arc<ParsedExtension>> {
        let mut parsed = self.parsed.lock().unwrap_or_else(|p| p.into_inner());
        parsed
            .entry(id.clone())
            .or_insert_with(|| ParsedExtension::load(extension_dir(&self.dir, id)).map(Arc::new))
            .clone()
    }

    /// The installed extensions changed (installed, updated, removed,
    /// enabled, disabled, or their rules and scripts changed): what was
    /// parsed is read again when next asked for.
    pub fn installed_changed(&self) {
        self.parsed
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    const ID: &str = "ddkjiahejlhfcafbddmgiahcphecmpfh";

    #[test]
    fn parses_once_until_the_extensions_change() {
        let root = tempfile::tempdir().unwrap();
        let id = ExtensionId::parse(ID).unwrap();
        let dir = extension_dir(root.path(), &id);
        fs::create_dir_all(dir.join("_locales/en")).unwrap();
        let write = |name: &str| {
            fs::write(
                dir.join("manifest.json"),
                format!(
                    r#"{{"name":"{name}","version":"1","permissions":["nativeMessaging"],
                    "background":{{"service_worker":"bg.js"}}}}"#
                ),
            )
            .unwrap();
        };
        write("__MSG_title__");
        fs::write(
            dir.join("_locales/en/messages.json"),
            r#"{"Title":{"message":"First"}}"#,
        )
        .unwrap();

        let catalog = ExtensionCatalog::for_dir(root.path());
        assert!(Arc::ptr_eq(
            &catalog,
            &ExtensionCatalog::for_dir(root.path())
        ));
        let parsed = catalog.get(&id).unwrap();
        assert_eq!(parsed.info().unwrap().name, "First");
        assert_eq!(parsed.messages()["title"]["message"], "First");
        assert!(parsed.has_permission("nativeMessaging"));
        assert_eq!(parsed.background().unwrap().scripts, vec!["bg.js"]);

        // Changed on disk: the catalogue keeps what it parsed...
        write("Second");
        assert_eq!(catalog.get(&id).unwrap().info().unwrap().name, "First");
        // ...until the installed extensions change.
        catalog.installed_changed();
        assert_eq!(catalog.get(&id).unwrap().info().unwrap().name, "Second");

        let other = ExtensionId::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        assert!(catalog.get(&other).is_none());
    }
}
