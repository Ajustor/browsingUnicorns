//! Extensions on Android: installed like on the desktop (Chrome Web Store),
//! their network rules applied to the WebViews' requests through
//! [`MobileBrowser::intercept`].

use std::path::{Component, Path, PathBuf};

use application::ports::ExtensionSource;
use domain::{ExtensionId, Request, ResourceType, Verdict};
use infrastructure::extensions::extension_dir;

use crate::{MobileBrowser, MobileError};

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
}

/// What a WebView does with a request.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum Interception {
    Allow,
    Block,
    /// Answer with this file (an extension's harmless stand-in).
    Serve {
        path: String,
        mime_type: String,
    },
}

fn mime_type(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("js" | "mjs") => "application/javascript",
        Some("css") => "text/css",
        Some("html" | "htm") => "text/html",
        Some("json") => "application/json",
        Some("xml") => "text/xml",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("svg") => "image/svg+xml",
        Some("mp4") => "video/mp4",
        Some("mp3") => "audio/mpeg",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}

/// `path` inside the extension's folder, refusing `..`.
fn served_file(dir: &Path, id: &ExtensionId, path: &str) -> Option<PathBuf> {
    let relative = Path::new(path.trim_start_matches('/'));
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return None;
    }
    let file = extension_dir(dir, id).join(relative);
    file.is_file().then_some(file)
}

fn id(id: &str) -> Result<ExtensionId, MobileError> {
    ExtensionId::parse(id).ok_or_else(|| MobileError::new("not an extension id"))
}

#[uniffi::export]
impl MobileBrowser {
    pub fn extensions(&self) -> Vec<ExtensionInfo> {
        self.extensions_lock()
            .extensions()
            .items()
            .iter()
            .map(|e| ExtensionInfo {
                id: e.id.to_string(),
                name: e.name.clone(),
                version: e.version.clone(),
                description: e.description.clone(),
                enabled: e.enabled,
            })
            .collect()
    }

    /// Downloads and installs an extension from a Chrome Web Store link or
    /// id. Blocking: call it off the main thread. Returns its name.
    pub fn install_extension(&self, input: String) -> Result<String, MobileError> {
        let id = ExtensionId::from_store_input(&input)
            .ok_or_else(|| MobileError::new("Paste a Chrome Web Store link or an extension id."))?;
        let packages = self.extensions_lock().packages();
        let extension = packages
            .install(&ExtensionSource::Store(id))
            .map_err(MobileError::new)?;
        let name = format!("{} {}", extension.name, extension.version);
        let mut manager = self.extensions_lock();
        manager
            .installed(extension, self.lock().browser.engine_mut())
            .map_err(MobileError::new)?;
        Ok(name)
    }

    pub fn set_extension_enabled(&self, id: String, enabled: bool) -> Result<(), MobileError> {
        let id = self::id(&id)?;
        let mut manager = self.extensions_lock();
        manager
            .set_enabled(&id, enabled, self.lock().browser.engine_mut())
            .map_err(MobileError::new)
    }

    pub fn remove_extension(&self, id: String) -> Result<(), MobileError> {
        let id = self::id(&id)?;
        let mut manager = self.extensions_lock();
        manager
            .remove(&id, self.lock().browser.engine_mut())
            .map_err(MobileError::new)
    }

    /// What to do with a request of a WebView (`shouldInterceptRequest`):
    /// called on the WebView's IO thread, for every request. `document_url`
    /// is the page that makes it; `accept` and `fetch_dest` its `Accept` and
    /// `Sec-Fetch-Dest` headers, when the WebView gives them.
    pub fn intercept(
        &self,
        url: String,
        document_url: Option<String>,
        main_frame: bool,
        method: String,
        accept: Option<String>,
        fetch_dest: Option<String>,
    ) -> Interception {
        let rules = self.rules.read().unwrap_or_else(|p| p.into_inner());
        if rules.is_empty() {
            return Interception::Allow;
        }
        let resource_type = if main_frame {
            Some(ResourceType::MainFrame)
        } else {
            fetch_dest
                .as_deref()
                .and_then(ResourceType::from_fetch_dest)
                .filter(|t| *t != ResourceType::MainFrame)
                .or_else(|| ResourceType::guess(&url, accept.as_deref().unwrap_or("")))
        };
        let document = if main_frame { None } else { document_url };
        let initiator = document
            .as_deref()
            .and_then(|d| Request::new(d, None, None, "GET"))
            .map(|r| r.host);
        let Some(request) = Request::new(&url, initiator, resource_type, &method) else {
            return Interception::Allow;
        };
        match rules.decide(&request, document.as_deref()) {
            Verdict::Allow => Interception::Allow,
            Verdict::Block => Interception::Block,
            Verdict::Redirect { extension, path } => {
                match served_file(&self.extensions_dir, &extension, &path) {
                    Some(file) => Interception::Serve {
                        path: file.to_string_lossy().into_owned(),
                        mime_type: mime_type(&path).to_owned(),
                    },
                    None => Interception::Block,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn served_files_stay_in_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let id = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        let war = extension_dir(dir.path(), &id).join("war");
        std::fs::create_dir_all(&war).unwrap();
        std::fs::write(war.join("noop.js"), "").unwrap();
        assert!(served_file(dir.path(), &id, "/war/noop.js").is_some());
        assert!(served_file(dir.path(), &id, "/war/missing.js").is_none());
        assert!(served_file(dir.path(), &id, "/../../etc/passwd").is_none());
        assert_eq!(mime_type("/war/noop.JS"), "application/javascript");
    }
}
