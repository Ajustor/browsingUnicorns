//! Chrome extensions on disk: downloaded from the Chrome Web Store (or read
//! from a `.crx` file, or copied from an unpacked folder) into
//! `<profile>/extensions/<id>/`, where the web view loads them from.
//!
//! Where the web view has no extension engine (Android, WebKit), the
//! browser applies the extensions' network rules itself: [`rule_set`].

mod crx;
pub mod dnr;
pub mod manifest;
pub mod safari;
pub mod state;

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use application::ports::{ExtensionError, ExtensionPackages, ExtensionSource};
use data_encoding::BASE64;
use domain::{Extension, ExtensionId, Extensions, RuleSet};

/// Packages bigger than this are refused.
pub const MAX_PACKAGE: u64 = 100 * 1024 * 1024;
/// The browser version announced to the Web Store, which serves the build of
/// the extension made for it.
const CHROME_VERSION: &str = "140.0.0.0";

/// Fetches a URL (following redirects) and returns the body, or an error
/// message. Each app brings its HTTP client.
pub type Download = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

pub struct ExtensionFolder {
    dir: PathBuf,
    download: Download,
}

/// Where the Chrome Web Store serves the package of `id`.
pub fn store_package_url(id: &ExtensionId) -> String {
    format!(
        "https://clients2.google.com/service/update2/crx?response=redirect\
         &prodversion={CHROME_VERSION}&acceptformat=crx2,crx3&x=id%3D{id}%26uc"
    )
}

/// The network rules of the enabled `extensions` unpacked in `dir`, with
/// what they changed at run time (kept in `state_dir`).
pub fn rule_set(dir: &Path, state_dir: Option<&Path>, extensions: &Extensions) -> RuleSet {
    let per_extension = extensions
        .items()
        .iter()
        .filter(|e| e.enabled)
        .filter_map(|e| {
            let state = state_dir
                .map(|s| state::ExtensionState::load(s, &e.id))
                .unwrap_or_default();
            let read = dnr::read_rules(
                &extension_dir(dir, &e.id),
                state.enabled_rulesets.as_deref(),
            )
            .map(|(mut rules, mut skipped)| {
                for list in [&state.dynamic_rules, &state.session_rules] {
                    let (more, left) = dnr::convert_rules(list);
                    rules.extend(more);
                    skipped += left;
                }
                (rules, skipped)
            });
            match read {
                Ok((rules, skipped)) => {
                    log::info!(
                        "extensions: {} network rules from {} ({skipped} left out)",
                        rules.len(),
                        e.name
                    );
                    Some((e.id.clone(), rules))
                }
                Err(err) => {
                    log::warn!("extensions: rules of {}: {err}", e.name);
                    None
                }
            }
        })
        .collect();
    RuleSet::new(per_extension)
}

/// [`Download`] with ureq (desktop).
#[cfg(feature = "desktop")]
pub fn ureq_download() -> Download {
    use std::time::Duration;

    let tls = ureq::tls::TlsConfig::builder()
        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
        .build();
    let agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .new_agent();
    Arc::new(move |url: &str| {
        let mut resp = agent.get(url).call().map_err(|e| e.to_string())?;
        resp.body_mut()
            .with_config()
            .limit(MAX_PACKAGE)
            .read_to_vec()
            .map_err(|e| e.to_string())
    })
}

fn error(e: impl std::fmt::Display) -> ExtensionError {
    ExtensionError(e.to_string())
}

/// The folder of the unpacked extension `id`.
pub fn extension_dir(dir: &Path, id: &ExtensionId) -> PathBuf {
    dir.join(id.as_str())
}

impl ExtensionFolder {
    pub fn new(dir: impl Into<PathBuf>, download: Download) -> Self {
        Self {
            dir: dir.into(),
            download,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn download(&self, id: &ExtensionId) -> Result<Vec<u8>, ExtensionError> {
        let body = (self.download)(&store_package_url(id))
            .map_err(|e| error(format!("Chrome Web Store: {e}")))?;
        if body.is_empty() {
            return Err(error("the Chrome Web Store has no extension with this id"));
        }
        Ok(body)
    }

    /// Unpacks `zip` into `to` (emptied first).
    fn unzip(zip: &[u8], to: &Path) -> Result<(), ExtensionError> {
        if to.exists() {
            fs::remove_dir_all(to).map_err(error)?;
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(zip)).map_err(error)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).map_err(error)?;
            // Entries that would land outside `to` (`../`) are skipped.
            let Some(relative) = entry.enclosed_name() else {
                continue;
            };
            let path = to.join(relative);
            if entry.is_dir() {
                fs::create_dir_all(&path).map_err(error)?;
                continue;
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(error)?;
            }
            let mut data = Vec::new();
            entry.read_to_end(&mut data).map_err(error)?;
            fs::write(&path, data).map_err(error)?;
        }
        Ok(())
    }

    fn unpack(
        &self,
        bytes: &[u8],
        expected: Option<&ExtensionId>,
    ) -> Result<Extension, ExtensionError> {
        let crx = crx::parse(bytes).map_err(error)?;
        if let Some(expected) = expected
            && &crx.id != expected
        {
            return Err(error(format!(
                "the package is extension {}, not {expected}",
                crx.id
            )));
        }
        let staging = self.dir.join(format!(".install-{}", crx.id));
        let result = (|| {
            Self::unzip(crx.zip, &staging)?;
            manifest::set_key(&staging, &BASE64.encode(&crx.public_key)).map_err(error)?;
            self.put_in_place(&staging, &crx.id)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// Copies the unpacked extension in `folder`. Its id comes from the
    /// manifest's `key` when it has one, else from the folder's path (as in
    /// Chrome): installing the same folder again updates it.
    fn copy_unpacked(&self, folder: &Path) -> Result<Extension, ExtensionError> {
        use sha2::{Digest, Sha256};

        let folder =
            fs::canonicalize(folder).map_err(|e| error(format!("{}: {e}", folder.display())))?;
        if !folder.join("manifest.json").is_file() {
            return Err(error(format!(
                "{} has no manifest.json: choose the extension's folder",
                folder.display()
            )));
        }
        let id = match manifest::key(&folder).and_then(|k| BASE64.decode(k.as_bytes()).ok()) {
            Some(der) => ExtensionId::from_key_hash(&Sha256::digest(der)),
            None => {
                ExtensionId::from_key_hash(&Sha256::digest(folder.to_string_lossy().as_bytes()))
            }
        };
        let staging = self.dir.join(format!(".install-{id}"));
        let result = (|| {
            let _ = fs::remove_dir_all(&staging);
            copy_dir(&folder, &staging)?;
            self.put_in_place(&staging, &id)
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }

    /// Replaces extension `id`'s files with `staging`'s.
    fn put_in_place(&self, staging: &Path, id: &ExtensionId) -> Result<Extension, ExtensionError> {
        let info = manifest::read(staging).map_err(error)?;
        let target = extension_dir(&self.dir, id);
        if target.exists() {
            fs::remove_dir_all(&target)
                .map_err(|e| error(format!("could not replace the installed version: {e}")))?;
        }
        fs::rename(staging, &target).map_err(error)?;
        Ok(Extension {
            id: id.clone(),
            name: info.name,
            version: info.version,
            description: info.description,
            enabled: true,
            popup: info.popup,
            options: info.options,
        })
    }
}

/// Copies `from` into `to`, version control folders left out.
fn copy_dir(from: &Path, to: &Path) -> Result<(), ExtensionError> {
    fs::create_dir_all(to).map_err(error)?;
    for entry in fs::read_dir(from).map_err(error)? {
        let entry = entry.map_err(error)?;
        let name = entry.file_name();
        if name == ".git" || name == ".svn" || name == ".hg" {
            continue;
        }
        let kind = entry.file_type().map_err(error)?;
        if kind.is_dir() {
            copy_dir(&entry.path(), &to.join(&name))?;
        } else if kind.is_file() {
            fs::copy(entry.path(), to.join(&name)).map_err(error)?;
        }
    }
    Ok(())
}

impl ExtensionPackages for ExtensionFolder {
    fn install(&self, source: &ExtensionSource) -> Result<Extension, ExtensionError> {
        fs::create_dir_all(&self.dir).map_err(error)?;
        match source {
            ExtensionSource::Store(id) => self.unpack(&self.download(id)?, Some(id)),
            ExtensionSource::File(path) => {
                let bytes =
                    fs::read(path).map_err(|e| error(format!("{}: {e}", path.display())))?;
                self.unpack(&bytes, None)
            }
            ExtensionSource::Folder(folder) => self.copy_unpacked(folder),
        }
    }

    fn remove(&self, id: &ExtensionId) -> Result<(), ExtensionError> {
        match fs::remove_dir_all(extension_dir(&self.dir, id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(error(e)),
            _ => Ok(()),
        }
    }

    fn prune(&self, keep: &Extensions) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let kept = ExtensionId::parse(&name.to_string_lossy())
                .is_some_and(|id| keep.get(&id).is_some());
            if !kept
                && entry.path().is_dir()
                && let Err(e) = fs::remove_dir_all(entry.path())
            {
                log::warn!("extensions: removing {}: {e}", entry.path().display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn no_download() -> Download {
        Arc::new(|_: &str| Err("offline".to_owned()))
    }

    fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in files {
            out.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(content.as_bytes()).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    fn package(name: &str) -> Vec<u8> {
        crx::tests::crx3(
            b"publisher key",
            &zip_with(&[
                (
                    "manifest.json",
                    &format!(
                        r#"{{"name":"{name}","version":"1","action":{{"default_popup":"p.html"}}}}"#
                    ),
                ),
                ("p.html", "<p>hi</p>"),
                ("../evil.txt", "nope"),
            ]),
        )
    }

    #[test]
    fn installs_unpacked_folders() {
        let root = tempfile::tempdir().unwrap();
        let folder = ExtensionFolder::new(root.path().join("extensions"), no_download());
        let src = root.path().join("my-ext");
        fs::create_dir_all(src.join("js")).unwrap();
        fs::create_dir_all(src.join(".git")).unwrap();
        fs::write(
            src.join("manifest.json"),
            r#"{"name":"Mine","version":"1"}"#,
        )
        .unwrap();
        fs::write(src.join("js/bg.js"), "1").unwrap();
        fs::write(src.join(".git/HEAD"), "x").unwrap();

        let first = folder
            .install(&ExtensionSource::Folder(src.clone()))
            .unwrap();
        assert_eq!(first.name, "Mine");
        let dir = extension_dir(folder.dir(), &first.id);
        assert_eq!(fs::read_to_string(dir.join("js/bg.js")).unwrap(), "1");
        assert!(!dir.join(".git").exists());

        // Again from the same folder: the same extension, updated.
        fs::write(
            src.join("manifest.json"),
            r#"{"name":"Mine","version":"2"}"#,
        )
        .unwrap();
        let again = folder
            .install(&ExtensionSource::Folder(src.clone()))
            .unwrap();
        assert_eq!((again.id, again.version.as_str()), (first.id, "2"));

        // A key in the manifest gives the store's id.
        let key = BASE64.encode(b"publisher key");
        fs::write(
            src.join("manifest.json"),
            format!(r#"{{"name":"Mine","version":"3","key":"{key}"}}"#),
        )
        .unwrap();
        let keyed = folder
            .install(&ExtensionSource::Folder(src.clone()))
            .unwrap();
        let from_crx = folder.install(&ExtensionSource::File({
            let crx = root.path().join("p.crx");
            fs::write(&crx, package("Packed")).unwrap();
            crx
        }));
        assert_eq!(keyed.id, from_crx.unwrap().id);

        let err = folder.install(&ExtensionSource::Folder(root.path().join("extensions")));
        assert!(err.unwrap_err().to_string().contains("manifest.json"));
    }

    #[test]
    fn installs_updates_and_removes() {
        let root = tempfile::tempdir().unwrap();
        let folder = ExtensionFolder::new(root.path().join("extensions"), no_download());
        let file = root.path().join("ext.crx");
        fs::write(&file, package("First")).unwrap();

        let ext = folder
            .install(&ExtensionSource::File(file.clone()))
            .unwrap();
        assert_eq!(ext.id, crx::id_of_key(b"publisher key"));
        assert_eq!(ext.name, "First");
        assert_eq!(ext.popup.as_deref(), Some("p.html"));
        let dir = extension_dir(folder.dir(), &ext.id);
        assert!(dir.join("p.html").exists());
        assert!(!root.path().join("evil.txt").exists());
        assert!(
            fs::read_to_string(dir.join("manifest.json"))
                .unwrap()
                .contains(&BASE64.encode(b"publisher key"))
        );

        fs::write(&file, package("Second")).unwrap();
        assert_eq!(
            folder.install(&ExtensionSource::File(file)).unwrap().name,
            "Second"
        );

        fs::create_dir_all(folder.dir().join(".install-leftover")).unwrap();
        folder.prune(&Extensions::from_items(vec![ext.clone()]));
        assert!(dir.exists());
        assert!(!folder.dir().join(".install-leftover").exists());

        folder.remove(&ext.id).unwrap();
        assert!(!dir.exists());
        folder.remove(&ext.id).unwrap();
    }

    #[test]
    fn a_store_package_must_be_the_one_asked_for() {
        let root = tempfile::tempdir().unwrap();
        let folder = ExtensionFolder::new(root.path(), no_download());
        let other = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        assert!(folder.unpack(&package("x"), Some(&other)).is_err());
        assert!(
            fs::read_dir(root.path()).unwrap().next().is_none(),
            "nothing left"
        );
    }

    /// Talks to the real Chrome Web Store.
    #[cfg(feature = "desktop")]
    #[test]
    #[ignore = "downloads uBlock Origin Lite from the Chrome Web Store"]
    fn installs_from_the_store() {
        let root = tempfile::tempdir().unwrap();
        let folder = ExtensionFolder::new(root.path(), ureq_download());
        let id = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        let ext = folder.install(&ExtensionSource::Store(id.clone())).unwrap();
        assert_eq!(ext.id, id);
        println!("{ext:?}");
        assert!(
            extension_dir(folder.dir(), &id)
                .join("manifest.json")
                .exists()
        );

        // Its network rules, and how long a decision takes.
        let set = rule_set(folder.dir(), None, &Extensions::from_items(vec![ext]));
        println!("{} rules", set.len());
        assert!(set.len() > 1000);
        let (webkit, count) = safari::content_rule_list(&set);
        println!("{count} WebKit rules, {} KB", webkit.len() / 1024);
        let page = Some("https://news.example/");
        let urls = [
            "https://www.google-analytics.com/collect?v=1",
            "https://googleads.g.doubleclick.net/pagead/viewthroughconversion/1/",
            "https://news.example/style.css",
            "https://cdn.example.org/lib/jquery.min.js",
        ];
        let start = std::time::Instant::now();
        let verdicts: Vec<_> = urls
            .iter()
            .map(|url| {
                let ty = domain::ResourceType::guess(url, "*/*");
                let request = domain::Request::new(url, Some("news.example"), ty, "GET").unwrap();
                set.decide(&request, page)
            })
            .collect();
        println!("{} decisions in {:?}", urls.len(), start.elapsed());
        for (url, verdict) in urls.iter().zip(&verdicts) {
            println!("{url}: {verdict:?}");
        }
        assert_eq!(verdicts[0], domain::Verdict::Block);
        assert_eq!(verdicts[1], domain::Verdict::Block);
        assert_eq!(verdicts[2], domain::Verdict::Allow);
        assert_eq!(verdicts[3], domain::Verdict::Allow);
    }
}
