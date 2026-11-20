//! User scripts on disk: `<profile>/userscripts/<id>.user.js`, and the
//! scripts they `@require` in `<id>.require.js`.

use std::fs;
use std::path::{Path, PathBuf};

use application::ports::{ExtensionError, UserScriptPackages};
use domain::{UserScript, UserScriptId, UserScriptMeta, UserScripts};
use sha2::{Digest, Sha256};

use crate::extensions::Download;

pub struct UserScriptFolder {
    dir: PathBuf,
    download: Download,
}

fn error(e: impl std::fmt::Display) -> ExtensionError {
    ExtensionError(e.to_string())
}

/// The `==UserScript==` block of `code` (what the profile keeps of it).
pub fn metadata_block(code: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in code.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("// ==UserScript==") {
            inside = true;
        }
        if inside {
            out.push_str(trimmed);
            out.push('\n');
        }
        if trimmed.starts_with("// ==/UserScript==") {
            break;
        }
    }
    out
}

/// `name-1a2b3c4d`: readable, and the same for the same name and namespace
/// (an update replaces the script).
pub fn script_id(meta: &UserScriptMeta) -> UserScriptId {
    let slug: String = meta
        .name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let slug: String = slug.chars().take(40).collect();
    let hash = Sha256::digest(format!("{}\n{}", meta.namespace, meta.name));
    let hex: String = hash[..4].iter().map(|b| format!("{b:02x}")).collect();
    let id = if slug.is_empty() {
        hex
    } else {
        format!("{slug}-{hex}")
    };
    UserScriptId::parse(&id).expect("only a-z, 0-9 and -")
}

impl UserScriptFolder {
    pub fn new(dir: impl Into<PathBuf>, download: Download) -> Self {
        Self {
            dir: dir.into(),
            download,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn fetch_text(&self, url: &str) -> Result<String, ExtensionError> {
        let bytes = (self.download)(url).map_err(|e| error(format!("{url}: {e}")))?;
        String::from_utf8(bytes)
            .map_err(|_| error(i18n::errors::infrastructure::not_utf8_text(url)))
    }

    /// Saves `code` (from `url`) and what it requires.
    pub fn save(&self, url: &str, code: &str) -> Result<UserScript, ExtensionError> {
        let meta = UserScriptMeta::parse(code)
            .ok_or_else(|| error(i18n::errors::infrastructure::not_a_user_script()))?;
        if meta.matches.is_empty() {
            return Err(error(i18n::errors::infrastructure::script_runs_on_no_page()));
        }
        let mut required = String::new();
        for lib in &meta.requires {
            required.push_str(&self.fetch_text(lib)?);
            required.push_str("\n;\n");
        }
        let id = script_id(&meta);
        fs::create_dir_all(&self.dir).map_err(error)?;
        crate::files::write(&code_path(&self.dir, &id), code).map_err(error)?;
        let require = require_path(&self.dir, &id);
        if required.is_empty() {
            let _ = fs::remove_file(&require);
        } else {
            crate::files::write(&require, &required).map_err(error)?;
        }
        Ok(UserScript {
            id,
            meta,
            source_url: url.to_owned(),
            enabled: true,
        })
    }
}

pub fn code_path(dir: &Path, id: &UserScriptId) -> PathBuf {
    dir.join(format!("{id}.user.js"))
}

pub fn require_path(dir: &Path, id: &UserScriptId) -> PathBuf {
    dir.join(format!("{id}.require.js"))
}

/// What runs for script `id`: its `@require`s, then its code.
pub fn read_code(dir: &Path, id: &UserScriptId) -> Option<(String, String)> {
    let code = fs::read_to_string(code_path(dir, id)).ok()?;
    let required = fs::read_to_string(require_path(dir, id)).unwrap_or_default();
    Some((required, code))
}

impl UserScriptPackages for UserScriptFolder {
    fn install(&self, url: &str) -> Result<UserScript, ExtensionError> {
        let url = url.trim();
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return Err(error(
                i18n::errors::infrastructure::paste_a_user_script_address(),
            ));
        }
        let code = self.fetch_text(url)?;
        self.save(url, &code)
    }

    fn remove(&self, id: &UserScriptId) -> Result<(), ExtensionError> {
        for path in [code_path(&self.dir, id), require_path(&self.dir, id)] {
            match fs::remove_file(path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(error(e)),
                _ => {}
            }
        }
        Ok(())
    }

    fn prune(&self, keep: &UserScripts) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name
                .strip_suffix(".user.js")
                .or_else(|| name.strip_suffix(".require.js"))
                .and_then(UserScriptId::parse);
            if id.is_none_or(|id| keep.get(&id).is_none())
                && entry.path().is_file()
                && let Err(e) = fs::remove_file(entry.path())
            {
                log::warn!("user scripts: removing {name}: {e}");
            }
        }
    }
}

/// The list of user scripts (`userscripts.json`): id, source and on/off.
/// Their metadata is read again from their code.
pub struct UserScriptRepository {
    file: PathBuf,
    dir: PathBuf,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct ListFile {
    scripts: Vec<EntryDto>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct EntryDto {
    id: String,
    source_url: String,
    enabled: bool,
}

impl UserScriptRepository {
    pub fn new(file: impl Into<PathBuf>, dir: impl Into<PathBuf>) -> Self {
        Self {
            file: file.into(),
            dir: dir.into(),
        }
    }
}

impl application::ports::Repository<UserScripts> for UserScriptRepository {
    fn load(&self) -> Result<UserScripts, application::ports::RepositoryError> {
        let list: ListFile = match crate::files::read(&self.file)? {
            Some(text) => {
                serde_json::from_str(&text).map_err(|e| crate::files::error(&self.file, e))?
            }
            None => ListFile::default(),
        };
        Ok(UserScripts::from_items(
            list.scripts
                .into_iter()
                .filter_map(|e| {
                    let id = UserScriptId::parse(&e.id)?;
                    let code = fs::read_to_string(code_path(&self.dir, &id)).ok()?;
                    Some(UserScript {
                        meta: UserScriptMeta::parse(&code)?,
                        id,
                        source_url: e.source_url,
                        enabled: e.enabled,
                    })
                })
                .collect(),
        ))
    }

    fn save(&self, scripts: &UserScripts) -> Result<(), application::ports::RepositoryError> {
        let list = ListFile {
            scripts: scripts
                .items()
                .iter()
                .map(|s| EntryDto {
                    id: s.id.to_string(),
                    source_url: s.source_url.clone(),
                    enabled: s.enabled,
                })
                .collect(),
        };
        let text = serde_json::to_string(&list).map_err(|e| crate::files::error(&self.file, e))?;
        crate::files::write(&self.file, &text)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use application::ports::Repository;

    use super::*;

    #[test]
    fn list_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let f = folder(dir.path());
        let mut s = f.install("https://greasyfork.test/wide.user.js").unwrap();
        s.enabled = false;
        let repo = UserScriptRepository::new(dir.path().join("userscripts.json"), dir.path());
        assert!(repo.load().unwrap().is_empty());
        let list = UserScripts::from_items(vec![s]);
        repo.save(&list).unwrap();
        assert_eq!(repo.load().unwrap(), list);
    }

    const SCRIPT: &str = "// ==UserScript==\n// @name  Wide Wikipedia!\n// @namespace ex\n\
        // @match https://*.wikipedia.org/*\n// @require https://cdn.test/lib.js\n\
        // ==/UserScript==\nconsole.log(1);";

    fn folder(dir: &Path) -> UserScriptFolder {
        UserScriptFolder::new(
            dir,
            Arc::new(|url: &str| match url {
                "https://greasyfork.test/wide.user.js" => Ok(SCRIPT.as_bytes().to_vec()),
                "https://cdn.test/lib.js" => Ok(b"var lib = 1;".to_vec()),
                "https://greasyfork.test/nometa.user.js" => Ok(b"alert(1)".to_vec()),
                _ => Err("404".into()),
            }),
        )
    }

    #[test]
    fn installs_with_requires_and_prunes() {
        let dir = tempfile::tempdir().unwrap();
        let f = folder(dir.path());
        let s = f.install("https://greasyfork.test/wide.user.js").unwrap();
        assert!(s.id.as_str().starts_with("wide-wikipedia-"), "{}", s.id);
        assert_eq!(script_id(&s.meta), s.id, "stable");
        let (required, code) = read_code(dir.path(), &s.id).unwrap();
        assert!(required.contains("var lib = 1;"));
        assert!(code.contains("console.log(1)"));
        assert!(f.install("https://greasyfork.test/nometa.user.js").is_err());
        assert!(f.install("ftp://x").is_err());

        fs::write(dir.path().join("old-1234.user.js"), "x").unwrap();
        f.prune(&UserScripts::from_items(vec![s.clone()]));
        assert!(!dir.path().join("old-1234.user.js").exists());
        assert!(code_path(dir.path(), &s.id).exists());
        f.remove(&s.id).unwrap();
        assert!(read_code(dir.path(), &s.id).is_none());
    }

    #[test]
    fn metadata_block_only() {
        let block = metadata_block(SCRIPT);
        assert!(block.ends_with("// ==/UserScript==\n"));
        assert!(!block.contains("console.log"));
        assert_eq!(UserScriptMeta::parse(&block), UserScriptMeta::parse(SCRIPT));
    }
}
