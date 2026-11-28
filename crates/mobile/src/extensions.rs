//! Extensions on Android: installed like on the desktop (Chrome Web Store),
//! their network rules applied to the WebViews' requests through
//! [`MobileBrowser::intercept`], with the built-in ad blocker's.

use std::path::{Component, Path, PathBuf};

use application::ports::ExtensionSource;
use domain::{ExtensionId, Request, ResourceType, UserScriptId, Verdict};
use infrastructure::extensions::extension_dir;

use infrastructure::extension_hub::BrowserHub;
use infrastructure::extension_server::{Origins, Requester};

use crate::{MobileBrowser, MobileError, PageTarget};

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExtensionInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub has_popup: bool,
    pub has_options: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ExtensionResource {
    pub mime_type: String,
    pub data: Vec<u8>,
    /// Answer with `Access-Control-Allow-Origin: *` (a web accessible
    /// resource another origin asked for).
    pub cors: bool,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct UserScriptInfo {
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

fn script_id(id: &str) -> Result<UserScriptId, MobileError> {
    UserScriptId::parse(id).ok_or_else(|| MobileError::new(i18n::mobile::not_a_user_script_id()))
}

fn id(id: &str) -> Result<ExtensionId, MobileError> {
    ExtensionId::parse(id).ok_or_else(|| MobileError::new(i18n::mobile::not_an_extension_id()))
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
                has_popup: e.popup.is_some(),
                has_options: e.options.is_some(),
            })
            .collect()
    }

    /// Downloads and installs an extension from a Chrome Web Store link or
    /// id. Blocking: call it off the main thread. Returns its name.
    pub fn install_extension(&self, input: String) -> Result<String, MobileError> {
        let id = ExtensionId::from_store_input(&input)
            .ok_or_else(|| MobileError::new(i18n::mobile::paste_store_link_or_id()))?;
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

    /// Installs the newer versions of the extensions from their stores, at
    /// most once a day. Blocking (downloads): call it off the main thread.
    /// Returns how many were updated.
    pub fn update_extensions(&self) -> u32 {
        let clock = infrastructure::extensions::UpdateClock::new(
            self.extensions_dir.join(".last-update-check"),
        );
        let (packages, installed) = {
            let manager = self.extensions_lock();
            (manager.packages(), manager.versions())
        };
        if installed.is_empty() || !clock.is_due() {
            return 0;
        }
        clock.mark();
        let updated = application::extensions::fetch_updates(packages.as_ref(), &installed);
        let count = updated.len() as u32;
        let mut manager = self.extensions_lock();
        for extension in updated {
            if let Err(e) = manager.installed(extension, self.lock().browser.engine_mut()) {
                eprintln!("extensions: recording an update: {e}");
            }
        }
        count
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
            .map_err(MobileError::new)?;
        self.hub_lock().forget(&format!("ext:{id}"));
        Ok(())
    }

    pub fn user_scripts(&self) -> Vec<UserScriptInfo> {
        self.user_scripts_lock()
            .scripts()
            .items()
            .iter()
            .map(|s| UserScriptInfo {
                id: s.id.to_string(),
                name: s.meta.name.clone(),
                version: s.meta.version.clone(),
                description: s.meta.description.clone(),
                enabled: s.enabled,
            })
            .collect()
    }

    /// Downloads and installs a user script (`.user.js` address). Blocking:
    /// call it off the main thread. Returns its name.
    pub fn install_user_script(&self, url: String) -> Result<String, MobileError> {
        let packages = self.user_scripts_lock().packages();
        let script = packages.install(&url).map_err(MobileError::new)?;
        let name = format!("{} {}", script.meta.name, script.meta.version);
        let mut manager = self.user_scripts_lock();
        manager
            .installed(script, self.lock().browser.engine_mut())
            .map_err(MobileError::new)?;
        Ok(name)
    }

    pub fn set_user_script_enabled(&self, id: String, enabled: bool) -> Result<(), MobileError> {
        let id = script_id(&id)?;
        let mut manager = self.user_scripts_lock();
        manager
            .set_enabled(&id, enabled, self.lock().browser.engine_mut())
            .map_err(MobileError::new)
    }

    pub fn remove_user_script(&self, id: String) -> Result<(), MobileError> {
        let id = script_id(&id)?;
        let mut manager = self.user_scripts_lock();
        manager
            .remove(&id, self.lock().browser.engine_mut())
            .map_err(MobileError::new)?;
        self.hub_lock().forget(&format!("us:{id}"));
        Ok(())
    }

    /// A message of the page scripts of `from` (`BUBridge.call`). The
    /// answer comes back through `WebViewHost::post_to_page`.
    pub fn page_message(&self, from: PageTarget, message: String) -> String {
        if self.credential_message(&from, &message) {
            return String::new();
        }
        let Some(source) = from.source() else {
            return String::new();
        };
        let extensions = self.extensions_lock().extensions().clone();
        let mut hub = self.hub_lock();
        let mut inner = self.lock();
        hub.handle(
            &source,
            &message,
            &mut BrowserHub {
                browser: &mut inner.browser,
                extensions: &extensions,
            },
        );
        String::new()
    }

    /// A file of an extension (`https://<id>.bu-ext.invalid/…`), for the
    /// WebViews' `shouldInterceptRequest`; `None` for any other address,
    /// or a file `origin`, `referer`, `fetch_site` (the request's `Origin`,
    /// `Referer`, `Sec-Fetch-Site` headers, when the WebView gives them)
    /// tell is for another origin the manifest does not let have it.
    pub fn extension_resource(
        &self,
        url: String,
        origin: Option<String>,
        referer: Option<String>,
        fetch_site: Option<String>,
    ) -> Option<ExtensionResource> {
        let requester = Requester {
            origin,
            referer,
            fetch_site,
        };
        self.server
            .serve_to(&url, &requester)
            .map(|r| ExtensionResource {
                mime_type: r.mime.split(';').next().unwrap_or(r.mime).to_owned(),
                data: r.body,
                cors: r.cors,
            })
    }

    /// The address of an extension's popup, to show in a panel.
    pub fn extension_popup_url(&self, id: String) -> Option<String> {
        let id = ExtensionId::parse(&id)?;
        let manager = self.extensions_lock();
        let e = manager.extensions().get(&id)?;
        Some(Origins::Android.url(&id, e.popup.as_deref()?))
    }

    /// The address of an extension's options page, to open in a tab.
    pub fn extension_options_url(&self, id: String) -> Option<String> {
        let id = ExtensionId::parse(&id)?;
        let manager = self.extensions_lock();
        let e = manager.extensions().get(&id)?;
        Some(Origins::Android.url(&id, e.options.as_deref()?))
    }

    /// The popup closed.
    pub fn extension_popup_closed(&self) {
        self.hub_lock().panel_closed();
    }

    /// What to do with a request of `tab`'s WebView
    /// (`shouldInterceptRequest`): called on the WebView's IO thread, for
    /// every request. `document_url` is the page that makes it; `accept` and
    /// `fetch_dest` its `Accept` and `Sec-Fetch-Dest` headers, when the
    /// WebView gives them. The extensions' rules decide first, then the ad
    /// blocker.
    #[allow(clippy::too_many_arguments)]
    pub fn intercept(
        &self,
        tab: u64,
        url: String,
        document_url: Option<String>,
        main_frame: bool,
        method: String,
        accept: Option<String>,
        fetch_dest: Option<String>,
    ) -> Interception {
        // A new page: the ad blocker counts again.
        if main_frame {
            self.blocked_lock().insert(tab, 0);
        }
        let rules = self.rules.read().unwrap_or_else(|p| p.into_inner());
        if rules.is_empty() && self.blocker.get().is_none() {
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
        let verdict = if rules.is_empty() {
            Verdict::Allow
        } else {
            let initiator = document
                .as_deref()
                .and_then(|d| Request::new(d, None, None, "GET"))
                .map(|r| r.host);
            match Request::new(&url, initiator, resource_type, &method) {
                Some(request) => rules.decide(&request, document.as_deref()),
                None => Verdict::Allow,
            }
        };
        drop(rules);
        match verdict {
            Verdict::Allow => match document {
                Some(page) if self.blocker.should_block(&url, &page, resource_type) => {
                    let mut blocked = self.blocked_lock();
                    let count = blocked.entry(tab).or_insert(0);
                    *count = count.saturating_add(1);
                    Interception::Block
                }
                _ => Interception::Allow,
            },
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

    /// The ad blocker's work in a page about to load at `url`: the style
    /// sheet hiding its site's ad slots, then its scriptlets, run on its host
    /// only; empty when there is none (blocker off, site allowlisted, no list
    /// yet). Registered by the app at document start, once per site: the
    /// lists' cosmetic filters go by site, not by page. Here rather than in
    /// a page script, which would take the whole runtime into every frame.
    pub fn scriptlets(&self, url: String) -> String {
        let Some(host) = infrastructure::site_scripts::web_host(&url) else {
            return String::new();
        };
        let css = self.blocker.cosmetic_css(&url);
        let mut script = if css.is_empty() {
            String::new()
        } else {
            cosmetic_style(&css)
        };
        script.push_str(&self.blocker.scriptlets(&url));
        if script.is_empty() {
            return String::new();
        }
        infrastructure::site_scripts::guarded(&host, &script)
    }
}

/// A script adding `css` to the document, as soon as it has a root.
fn cosmetic_style(css: &str) -> String {
    let css = serde_json::to_string(css).expect("a string");
    format!(
        "(() => {{
const style = document.createElement(\"style\");
style.textContent = {css};
const add = () => (document.head || document.documentElement).appendChild(style);
if (document.documentElement) add();
else new MutationObserver((_, o) => {{ if (document.documentElement) {{ o.disconnect(); add(); }} }})
  .observe(document, {{ childList: true }});
}})();
"
    )
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
