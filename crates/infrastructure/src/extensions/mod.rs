//! Chrome extensions on disk: downloaded from the Chrome Web Store (or read
//! from a `.crx` file) and unpacked into `<profile>/extensions/<id>/`, where
//! the web view loads them from.

mod crx;
mod manifest;

use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;

use application::ports::{ExtensionError, ExtensionPackages, ExtensionSource};
use data_encoding::BASE64;
use domain::{Extension, ExtensionId, Extensions};

/// Packages bigger than this are refused.
const MAX_PACKAGE: u64 = 100 * 1024 * 1024;
/// The browser version announced to the Web Store, which serves the build of
/// the extension made for it.
const CHROME_VERSION: &str = "140.0.0.0";

pub struct ExtensionFolder {
    dir: PathBuf,
}

fn error(e: impl std::fmt::Display) -> ExtensionError {
    ExtensionError(e.to_string())
}

/// The folder of the unpacked extension `id`.
pub fn extension_dir(dir: &Path, id: &ExtensionId) -> PathBuf {
    dir.join(id.as_str())
}

impl ExtensionFolder {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn download(id: &ExtensionId) -> Result<Vec<u8>, ExtensionError> {
        let url = format!(
            "https://clients2.google.com/service/update2/crx?response=redirect\
             &prodversion={CHROME_VERSION}&acceptformat=crx2,crx3&x=id%3D{id}%26uc"
        );
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_global(Some(Duration::from_secs(120)))
            .build()
            .new_agent();
        let mut resp = agent
            .get(&url)
            .call()
            .map_err(|e| error(format!("Chrome Web Store: {e}")))?;
        let body = resp
            .body_mut()
            .with_config()
            .limit(MAX_PACKAGE)
            .read_to_vec()
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
            let info = manifest::read(&staging).map_err(error)?;
            let target = extension_dir(&self.dir, &crx.id);
            if target.exists() {
                fs::remove_dir_all(&target)
                    .map_err(|e| error(format!("could not replace the installed version: {e}")))?;
            }
            fs::rename(&staging, &target).map_err(error)?;
            Ok(Extension {
                id: crx.id.clone(),
                name: info.name,
                version: info.version,
                description: info.description,
                enabled: true,
                popup: info.popup,
                options: info.options,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }
}

impl ExtensionPackages for ExtensionFolder {
    fn install(&self, source: &ExtensionSource) -> Result<Extension, ExtensionError> {
        fs::create_dir_all(&self.dir).map_err(error)?;
        match source {
            ExtensionSource::Store(id) => self.unpack(&Self::download(id)?, Some(id)),
            ExtensionSource::File(path) => {
                let bytes =
                    fs::read(path).map_err(|e| error(format!("{}: {e}", path.display())))?;
                self.unpack(&bytes, None)
            }
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
    fn installs_updates_and_removes() {
        let root = tempfile::tempdir().unwrap();
        let folder = ExtensionFolder::new(root.path().join("extensions"));
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
        let folder = ExtensionFolder::new(root.path());
        let other = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        assert!(folder.unpack(&package("x"), Some(&other)).is_err());
        assert!(
            fs::read_dir(root.path()).unwrap().next().is_none(),
            "nothing left"
        );
    }

    /// Talks to the real Chrome Web Store.
    #[test]
    #[ignore = "downloads uBlock Origin Lite from the Chrome Web Store"]
    fn installs_from_the_store() {
        let root = tempfile::tempdir().unwrap();
        let folder = ExtensionFolder::new(root.path());
        let id = ExtensionId::parse("ddkjiahejlhfcafbddmgiahcphecmpfh").unwrap();
        let ext = folder.install(&ExtensionSource::Store(id.clone())).unwrap();
        assert_eq!(ext.id, id);
        println!("{ext:?}");
        assert!(
            extension_dir(folder.dir(), &id)
                .join("manifest.json")
                .exists()
        );
    }
}
