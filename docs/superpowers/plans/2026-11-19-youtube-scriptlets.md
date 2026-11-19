# Scriptlets du bloqueur (pubs YouTube) — plan d'implémentation

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** bloquer les pubs de YouTube (et de tout site couvert par les listes uBlock) en injectant les scriptlets d'adblock-rust au départ de chaque navigation, sur Windows, macOS, Linux et Android.

**Architecture:** le moteur (`crates/infrastructure/src/blocker.rs`) charge en plus les listes uBlock (avec la permission des scriptlets « trusted ») et les ressources de scriptlets de Brave, et donne `scriptlets(url)`. Un petit module pur (`site_scripts.rs`) transforme ce script en script gardé par nom d'hôte et retient les hôtes déjà enregistrés par vue. Chaque plateforme l'enregistre en début de document au départ de la navigation principale : `NavigationStarting` (WebView2), le gestionnaire de navigation wry (WebKit), `addDocumentStartJavaScript` (Android).

**Tech Stack:** Rust (adblock 0.13.5, wry 0.57, webview2-com 0.39, webkit2gtk, objc2-web-kit), uniffi, Kotlin (androidx.webkit).

Spec : `docs/superpowers/specs/2026-11-19-youtube-scriptlets-design.md`.

Conventions du dépôt : commits conventionnels **sans** ligne `Co-Authored-By` ; commentaires en anglais dans le code, à la manière du code existant ; préfixer les commandes par `rtk`.

---

## Fichiers

| Fichier | Rôle |
|---|---|
| `crates/infrastructure/src/blocker.rs` (modifié) | listes uBlock, permissions, `resources.json`, `scriptlets(url)`, `due` si un fichier manque |
| `crates/infrastructure/src/site_scripts.rs` (créé) | pur : script gardé par hôte, hôtes déjà enregistrés d'une vue |
| `crates/infrastructure/src/webview2_scriptlets.rs` (créé) | Windows : `NavigationStarting` → `AddScriptToExecuteOnDocumentCreated` |
| `crates/infrastructure/src/webkit_scriptlets.rs` (créé) | macOS / Linux : ajout et retrait des scripts de début de document |
| `crates/infrastructure/src/wry_engine.rs` (modifié) | branchement par onglet, vidage quand le bloqueur change |
| `crates/infrastructure/src/lib.rs` (modifié) | déclaration des modules |
| `crates/mobile/src/extensions.rs` (modifié) | `MobileBrowser::scriptlets(url)` exporté |
| `crates/mobile/src/tests.rs` (modifié) | test Android côté Rust |
| `android/.../WebViews.kt`, `BrowserController.kt` (modifiés) | enregistrement par origine, vidage |
| `src/ui/shield.rs`, `android/.../ui/Shield.kt` (modifiés) | nom des listes |
| `CHANGELOG.md` (modifié) | entrée « Non publié » |

---

### Task 1 : listes uBlock et ressources de scriptlets dans le cache

**Files:**
- Modify: `crates/infrastructure/src/blocker.rs`

- [ ] **Step 1: Réécrire le faux téléchargement des tests, indexé par adresse**

Les tests actuels donnent « une réponse par liste, dans l'ordre » ; avec 4 listes et les ressources, on indexe par adresse (adresse absente = hors ligne). Dans `mod tests`, remplacer `fn fake` par :

```rust
    /// Answers `answers` by address; any other address is offline.
    fn fake(answers: &[(&str, &'static str)]) -> Download {
        let answers: std::collections::HashMap<String, &'static str> = answers
            .iter()
            .map(|(url, text)| (url.to_string(), *text))
            .collect();
        Arc::new(move |url: &str| {
            answers
                .get(url)
                .map(|t| t.as_bytes().to_vec())
                .ok_or_else(|| "offline".to_string())
        })
    }

    const EASYLIST: &str = "https://easylist.to/easylist/easylist.txt";
```

et adapter les appels existants :
- `fake(vec![Ok(LIST), Err("offline")])` → `fake(&[(EASYLIST, LIST)])` ;
- `fake(vec![Ok("! no rule\n"), Err("offline")])` → `fake(&[(EASYLIST, "! no rule\n")])` ;
- `fake(vec![Ok(LIST), Ok(LIST)])` → `fake(&[(EASYLIST, LIST)])` ;
- `fake(vec![])` → `fake(&[])`.

- [ ] **Step 2: Lancer les tests existants (ils doivent passer)**

Run: `rtk cargo test -p infrastructure blocker`
Expected: PASS (même comportement, faux téléchargement réécrit).

- [ ] **Step 3: Écrire les tests qui échouent**

Ajouter dans `mod tests` :

```rust
    /// A scriptlet: sets `window.said` to its first argument.
    const RESOURCES_JSON: &str = r#"[{"name":"say.js","aliases":[],"kind":{"mime":"application/javascript"},"content":"d2luZG93LnNhaWQgPSAie3sxfX0iOw=="}]"#;

    #[test]
    fn the_ublock_lists_and_the_resources_are_cached() {
        let dir = tempfile::tempdir().unwrap();
        let ublock = "https://ublockorigin.github.io/uAssets/filters/filters.txt";
        let lists = BlockerLists::new(
            dir.path(),
            fake(&[
                (EASYLIST, LIST),
                (ublock, "site.test##+js(say, hello)\n"),
                (RESOURCES.1, RESOURCES_JSON),
            ]),
        );
        lists.refresh().unwrap();
        assert!(dir.path().join("ublock-filters.txt").exists());
        assert!(dir.path().join(RESOURCES.0).exists());
    }

    #[test]
    fn unreadable_resources_are_not_cached() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(
            dir.path(),
            fake(&[(EASYLIST, LIST), (RESOURCES.1, "not json")]),
        );
        lists.refresh().unwrap();
        assert!(!dir.path().join(RESOURCES.0).exists());
    }

    #[test]
    fn a_list_missing_from_the_cache_makes_the_lists_due() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST)]));
        let now = SystemTime::now();
        lists.save_state(&State {
            last_check: Some(unix(now)),
            updated: None,
        });
        // Checked a minute ago, but only EasyList is there (an update added
        // lists): due again.
        fs::write(dir.path().join("easylist.txt"), LIST).unwrap();
        assert!(lists.due_at(now + Duration::from_secs(60)));
        for (name, _) in LISTS {
            fs::write(dir.path().join(name), LIST).unwrap();
        }
        fs::write(dir.path().join(RESOURCES.0), RESOURCES_JSON).unwrap();
        assert!(!lists.due_at(now + Duration::from_secs(60)));
    }
```

Et remplacer `the_lists_are_due_after_a_day` par une version où tous les fichiers sont là (sinon `due` serait vrai pour une autre raison) :

```rust
    #[test]
    fn the_lists_are_due_after_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[]));
        for (name, _) in LISTS {
            fs::write(dir.path().join(name), LIST).unwrap();
        }
        fs::write(dir.path().join(RESOURCES.0), RESOURCES_JSON).unwrap();
        let now = SystemTime::now();
        lists.save_state(&State {
            last_check: Some(unix(now)),
            updated: None,
        });
        assert!(!lists.due_at(now + Duration::from_secs(60)));
        assert!(lists.due_at(now + CHECK_EVERY));
    }
```

- [ ] **Step 4: Les lancer pour les voir échouer**

Run: `rtk cargo test -p infrastructure blocker`
Expected: FAIL à la compilation (`RESOURCES` inconnu).

- [ ] **Step 5: Implémenter**

En tête de `blocker.rs`, remplacer la doc du module et `LISTS` :

```rust
//! The built-in ad blocker: EasyList, EasyPrivacy and uBlock Origin's own
//! lists through adblock-rust (Brave's engine), with Brave's scriptlet
//! resources for the lists' `+js(...)` rules. The lists are downloaded and
//! cached under the app's data folder with the compiled engine; the request
//! hooks (WebView2, Android), the cosmetic page script and the scriptlet
//! injection consult it through a [`BlockerHandle`].
```

```rust
use adblock::resources::{PermissionMask, Resource};
```

```rust
/// The lists: file name in the cache, address.
pub const LISTS: [(&str, &str); 4] = [
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    (
        "easyprivacy.txt",
        "https://easylist.to/easylist/easyprivacy.txt",
    ),
    (
        "ublock-filters.txt",
        "https://ublockorigin.github.io/uAssets/filters/filters.txt",
    ),
    (
        "ublock-quick-fixes.txt",
        "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt",
    ),
];

/// uBlock Origin's own lists: their scriptlets may use the trusted ones
/// (`trusted-*`), as uBlock Origin lets them.
const TRUSTED: [&str; 2] = ["ublock-filters.txt", "ublock-quick-fixes.txt"];

/// The code of the scriptlets the lists name (Brave's, in adblock-rust's
/// format): file name in the cache, address.
pub const RESOURCES: (&str, &str) = (
    "resources.json",
    "https://raw.githubusercontent.com/brave/adblock-resources/master/dist/resources.json",
);
```

Dans `impl Blocker`, remplacer `from_lists` par une version qui sait quelles listes sont de confiance, en gardant `from_lists` pour les tests et l'existant :

```rust
    /// Compiles `lists`; `None` when none has a rule.
    pub fn from_lists(lists: &[&str]) -> Option<Self> {
        let lists: Vec<(bool, &str)> = lists.iter().map(|l| (false, *l)).collect();
        Self::compile(&lists)
    }

    /// Compiles `lists`, each with whether its scriptlets are trusted;
    /// `None` when none has a rule.
    fn compile(lists: &[(bool, &str)]) -> Option<Self> {
        if !lists.iter().any(|(_, l)| has_rules(l)) {
            return None;
        }
        let mut set = FilterSet::new(false);
        for (trusted, list) in lists {
            let options = if *trusted {
                ParseOptions {
                    permissions: PermissionMask::from_bits(u8::MAX),
                    ..ParseOptions::default()
                }
            } else {
                ParseOptions::default()
            };
            set.add_filter_list(list.to_string(), options);
        }
        Some(Self::new(
            Engine::new_with_filter_set(set),
            SystemTime::now(),
        ))
    }

    /// Gives the engine the scriptlets' code (`resources.json`); false when
    /// unreadable.
    pub fn use_resources(&mut self, json: &str) -> bool {
        match serde_json::from_str::<Vec<Resource>>(json) {
            Ok(resources) if !resources.is_empty() => {
                self.engine.use_resources(resources);
                true
            }
            Ok(_) => false,
            Err(e) => {
                log::warn!("blocker: resources: {e}");
                false
            }
        }
    }
```

Remplacer `cached_lists` (les noms servent à savoir lesquelles sont de confiance) :

```rust
/// The lists cached in `dir`, with whether their scriptlets are trusted.
fn cached_lists(dir: &Path) -> Vec<(bool, String)> {
    LISTS
        .iter()
        .filter_map(|(name, _)| {
            let text = fs::read_to_string(dir.join(name)).ok()?;
            Some((TRUSTED.contains(name), text))
        })
        .collect()
}
```

et adapter `webkit_rules_in`, qui ne prend que le texte :

```rust
    let lists = cached_lists(dir);
    if lists.is_empty() {
        return None;
    }
    let json = webkit_rules(
        &lists.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
        &policy.allowlist,
    );
```

Dans `impl BlockerLists` :

```rust
    /// The engine from the cached lists, with the cached resources.
    fn build(&self) -> Option<Blocker> {
        let lists = cached_lists(&self.dir);
        let mut blocker =
            Blocker::compile(&lists.iter().map(|(t, l)| (*t, l.as_str())).collect::<Vec<_>>())?;
        self.load_resources(&mut blocker);
        Some(blocker)
    }

    /// Hands `blocker` the cached scriptlets' code, when there is some
    /// (`engine.dat` does not keep it).
    fn load_resources(&self, blocker: &mut Blocker) {
        if let Ok(json) = fs::read_to_string(self.dir.join(RESOURCES.0)) {
            blocker.use_resources(&json);
        }
    }
```

Dans `cached`, après avoir obtenu `blocker` depuis `Blocker::deserialize` (branche `Some(blocker) => blocker`), charger les ressources : remplacer `Some(blocker) => blocker,` par

```rust
            Some(mut blocker) => {
                self.load_resources(&mut blocker);
                blocker
            }
```

Dans `refresh`, après la boucle des listes et avant `if fetched == 0`, télécharger les ressources (leur échec ne compte pas comme un échec des listes) :

```rust
        let (name, url) = RESOURCES;
        match (self.download)(url)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
        {
            Ok(json) if serde_json::from_str::<Vec<Resource>>(&json).is_ok_and(|r| !r.is_empty()) => {
                files::write(&self.dir.join(name), &json).map_err(|e| e.to_string())?;
            }
            Ok(_) => log::warn!("blocker: {url} is unreadable, the cached copy is kept"),
            Err(e) => log::warn!("blocker: {url}: {e}"),
        }
```

Et `due_at` devient vrai aussi quand un fichier attendu manque :

```rust
    fn due_at(&self, now: SystemTime) -> bool {
        // A list added by an update, or never downloaded: at once.
        let missing = LISTS
            .iter()
            .map(|(name, _)| *name)
            .chain([RESOURCES.0])
            .any(|name| !self.dir.join(name).exists());
        missing
            || self
                .state()
                .last_check
                .is_none_or(|t| unix(now).saturating_sub(t) >= CHECK_EVERY.as_secs())
    }
```

Note : un hors-ligne permanent avec des fichiers manquants fait retenter au démarrage et au retour sur l'app à chaque fois — même rythme que le tout premier lancement, acceptable.

- [ ] **Step 6: Lancer les tests**

Run: `rtk cargo test -p infrastructure blocker`
Expected: PASS.

Run: `rtk cargo test -p mobile`
Expected: PASS (`the_ad_blocker_blocks_and_counts` : les adresses non servies par `FakeHttp` sont hors ligne, EasyList suffit).

- [ ] **Step 7: Commit**

```bash
rtk git add crates/infrastructure/src/blocker.rs
rtk git commit -m "feat(blocker): uBlock Origin's lists and Brave's scriptlet resources"
```

---

### Task 2 : `scriptlets(url)` dans le moteur et la poignée partagée

**Files:**
- Modify: `crates/infrastructure/src/blocker.rs`

- [ ] **Step 1: Écrire les tests qui échouent**

Dans `mod tests` :

```rust
    const SCRIPTLET_LIST: &str = "\
site.test##+js(say, hello)
sub.site.test#@#+js(say, hello)
";

    fn with_scriptlets() -> Blocker {
        let mut b = Blocker::from_lists(&[SCRIPTLET_LIST]).unwrap();
        assert!(b.use_resources(RESOURCES_JSON));
        b
    }

    #[test]
    fn scriptlets_are_the_sites_own() {
        let b = with_scriptlets();
        let script = b.scriptlets("https://www.site.test/watch");
        assert!(script.contains("window.said"), "{script}");
        assert!(script.contains("hello"), "{script}");
        assert_eq!(b.scriptlets("https://other.test/"), "");
        // `#@#+js` cancels it on that subdomain.
        assert_eq!(b.scriptlets("https://sub.site.test/"), "");
    }

    #[test]
    fn without_resources_there_is_no_scriptlet() {
        let b = Blocker::from_lists(&[SCRIPTLET_LIST]).unwrap();
        assert_eq!(b.scriptlets("https://site.test/"), "");
        assert!(!Blocker::from_lists(&[SCRIPTLET_LIST]).unwrap().use_resources("[]"));
        assert!(!Blocker::from_lists(&[SCRIPTLET_LIST]).unwrap().use_resources("nope"));
    }

    #[test]
    fn a_cached_engine_gets_its_resources_back() {
        let dir = tempfile::tempdir().unwrap();
        let ublock = "https://ublockorigin.github.io/uAssets/filters/filters.txt";
        let lists = BlockerLists::new(
            dir.path(),
            fake(&[(ublock, SCRIPTLET_LIST), (RESOURCES.1, RESOURCES_JSON)]),
        );
        lists.refresh().unwrap();
        // From engine.dat, then from the lists.
        let b = lists.load_cached().unwrap();
        assert!(b.scriptlets("https://site.test/").contains("window.said"));
        fs::write(dir.path().join(ENGINE_FILE), b"garbage").unwrap();
        let b = lists.read_cached().unwrap();
        assert!(b.scriptlets("https://site.test/").contains("window.said"));
    }

    #[test]
    fn the_handle_gives_scriptlets_under_the_policy() {
        let handle = BlockerHandle::default();
        assert_eq!(handle.scriptlets("https://site.test/"), "");
        handle.set(with_scriptlets());
        assert!(!handle.scriptlets("https://site.test/").is_empty());
        handle.set_policy(BlockerPolicy {
            enabled: true,
            allowlist: vec!["site.test".into()],
        });
        assert_eq!(handle.scriptlets("https://www.site.test/"), "");
        handle.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        assert_eq!(handle.scriptlets("https://site.test/"), "");
    }
```

- [ ] **Step 2: Les voir échouer**

Run: `rtk cargo test -p infrastructure blocker`
Expected: FAIL à la compilation (`scriptlets` inconnu).

- [ ] **Step 3: Implémenter**

Dans `impl Blocker`, après `cosmetic_css` :

```rust
    /// The script the lists' `+js(...)` rules inject into `url`'s pages
    /// (their exceptions included), empty when none.
    pub fn scriptlets(&self, url: &str) -> String {
        self.engine.url_cosmetic_resources(url).injected_script
    }
```

Dans `impl BlockerHandle`, après `cosmetic_css` :

```rust
    /// The scriptlets for `url`'s pages; empty when the policy does not
    /// apply or no list is loaded.
    pub fn scriptlets(&self, url: &str) -> String {
        match self.get() {
            Some(blocker) if self.policy().applies_to(url) => blocker.scriptlets(url),
            _ => String::new(),
        }
    }
```

- [ ] **Step 4: Lancer les tests**

Run: `rtk cargo test -p infrastructure blocker`
Expected: PASS. Si `scriptlets_are_the_sites_own` échoue sur `sub.site.test` (forme d'exception non reconnue), afficher `b.scriptlets(...)` et ajuster la règle d'exception du test à la syntaxe qu'adblock-rust 0.13 accepte (`sub.site.test#@#+js(say, hello)` est celle d'uBlock) — ne pas affaiblir l'assertion.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/infrastructure/src/blocker.rs
rtk git commit -m "feat(blocker): the scriptlets of a page's site"
```

---

### Task 3 : `site_scripts.rs`, le script gardé et les hôtes d'une vue

**Files:**
- Create: `crates/infrastructure/src/site_scripts.rs`
- Modify: `crates/infrastructure/src/lib.rs` (après `pub mod script_storage;`)

- [ ] **Step 1: Écrire le module avec ses tests (tests d'abord, implémentation vide)**

`crates/infrastructure/src/site_scripts.rs` :

```rust
//! The ad blocker's scriptlets, injected at document start for one site:
//! each view keeps the hosts it has a script for, the script runs on that
//! host only and cannot break the page.

use std::collections::HashSet;

use domain::WebAddress;

/// Starts every scriptlet script: tells them from the other page scripts.
pub const MARKER: &str = "/* browsingUnicorns scriptlets */";

/// The host of `url` when it is a web page (http, https).
pub fn web_host(url: &str) -> Option<String> {
    todo!()
}

/// `script`, run on `host`'s documents only, its errors caught.
pub fn guarded(host: &str, script: &str) -> String {
    todo!()
}

/// The sites a view has a scriptlet script for, and what the web view
/// handed back for each (`H`: an id, a script object, nothing).
pub struct SiteScripts<H> {
    hosts: HashSet<String>,
    handles: Vec<H>,
}

impl<H> Default for SiteScripts<H> {
    fn default() -> Self {
        Self {
            hosts: HashSet::new(),
            handles: Vec::new(),
        }
    }
}

impl<H> SiteScripts<H> {
    /// The script to add for a navigation to `url`, when its host has
    /// scriptlets (`scriptlets(url)` not empty) and none was added yet.
    pub fn script_for(&mut self, url: &str, scriptlets: impl FnOnce(&str) -> String) -> Option<String> {
        todo!()
    }

    /// Keeps what the web view handed back for an added script.
    pub fn keep(&mut self, handle: H) {
        todo!()
    }

    /// Forgets every site; the handles, to remove their scripts.
    pub fn clear(&mut self) -> Vec<H> {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_pages_have_a_host() {
        assert_eq!(web_host("https://www.youtube.com/watch?v=1").as_deref(), Some("www.youtube.com"));
        assert_eq!(web_host("http://127.0.0.1:8000/").as_deref(), Some("127.0.0.1"));
        assert_eq!(web_host("file:///C:/page.html"), None);
        assert_eq!(web_host("about:blank"), None);
        assert_eq!(web_host("not a url"), None);
    }

    #[test]
    fn the_script_runs_on_its_host_only() {
        let s = guarded("www.youtube.com", "window.x = 1;");
        assert!(s.starts_with(MARKER));
        assert!(s.contains(r#"location.hostname === "www.youtube.com""#), "{s}");
        assert!(s.contains("try {") && s.contains("catch (e)"), "{s}");
        assert!(s.contains("window.x = 1;"));
        // A host cannot end the string it is compared with.
        assert!(guarded(r#"a"b"#, "").contains(r#""a\"b""#));
    }

    #[test]
    fn a_site_gets_its_script_once() {
        let mut sites = SiteScripts::<u32>::default();
        let with = |_: &str| "window.x = 1;".to_string();
        let script = sites.script_for("https://site.test/a", with).unwrap();
        assert!(script.contains(r#""site.test""#));
        assert_eq!(sites.script_for("https://site.test/b", with), None);
        assert!(sites.script_for("https://other.test/", with).is_some());
    }

    #[test]
    fn a_site_without_scriptlets_is_asked_again() {
        let mut sites = SiteScripts::<u32>::default();
        assert_eq!(sites.script_for("https://site.test/", |_| String::new()), None);
        // The lists arrived, or the site left the allowlist.
        assert!(sites.script_for("https://site.test/", |_| "x();".into()).is_some());
    }

    #[test]
    fn other_addresses_get_nothing() {
        let mut sites = SiteScripts::<u32>::default();
        assert_eq!(sites.script_for("file:///page.html", |_| "x();".into()), None);
    }

    #[test]
    fn clearing_forgets_the_sites_and_hands_the_handles_back() {
        let mut sites = SiteScripts::default();
        sites.script_for("https://site.test/", |_| "x();".into());
        sites.keep(7u32);
        assert_eq!(sites.clear(), vec![7]);
        assert!(sites.clear().is_empty());
        assert!(sites.script_for("https://site.test/", |_| "x();".into()).is_some());
    }
}
```

Dans `crates/infrastructure/src/lib.rs`, après `pub mod script_storage;` :

```rust
pub mod site_scripts;
```

- [ ] **Step 2: Les voir échouer**

Run: `rtk cargo test -p infrastructure site_scripts`
Expected: FAIL (panique `not yet implemented`).

- [ ] **Step 3: Implémenter (remplacer les `todo!()`)**

```rust
pub fn web_host(url: &str) -> Option<String> {
    let address = WebAddress::parse(url).ok()?;
    matches!(address.scheme(), "http" | "https")
        .then(|| address.host().map(str::to_owned))
        .flatten()
}

pub fn guarded(host: &str, script: &str) -> String {
    let host = serde_json::to_string(host).expect("a string");
    format!("{MARKER}\nif (location.hostname === {host}) {{ try {{\n{script}\n}} catch (e) {{}} }}\n")
}
```

```rust
    pub fn script_for(&mut self, url: &str, scriptlets: impl FnOnce(&str) -> String) -> Option<String> {
        let host = web_host(url)?;
        if self.hosts.contains(&host) {
            return None;
        }
        let script = scriptlets(url);
        if script.is_empty() {
            return None;
        }
        let guarded = guarded(&host, &script);
        self.hosts.insert(host);
        Some(guarded)
    }

    pub fn keep(&mut self, handle: H) {
        self.handles.push(handle);
    }

    pub fn clear(&mut self) -> Vec<H> {
        self.hosts.clear();
        std::mem::take(&mut self.handles)
    }
```

- [ ] **Step 4: Lancer les tests**

Run: `rtk cargo test -p infrastructure site_scripts`
Expected: PASS (6 tests). Si `WebAddress::parse` refuse `file:`/`about:` par une erreur, c'est le résultat voulu (`None`).

- [ ] **Step 5: Commit**

```bash
rtk git add crates/infrastructure/src/site_scripts.rs crates/infrastructure/src/lib.rs
rtk git commit -m "feat(blocker): scriptlet scripts guarded by site, once per view"
```

---

### Task 4 : Windows, enregistrement au départ de la navigation

**Files:**
- Create: `crates/infrastructure/src/webview2_scriptlets.rs`
- Modify: `crates/infrastructure/src/lib.rs`, `crates/infrastructure/src/wry_engine.rs`

- [ ] **Step 1: Le module WebView2**

`crates/infrastructure/src/webview2_scriptlets.rs` :

```rust
//! Windows: the ad blocker's scriptlets, added at the start of each
//! top-level navigation (`NavigationStarting`), before the new document
//! exists, so they run before the page's own scripts.

use std::cell::RefCell;
use std::rc::Rc;

use webview2_com::{
    AddScriptToExecuteOnDocumentCreatedCompletedHandler, NavigationStartingEventHandler, take_pwstr,
};
use windows_core::{HSTRING, PWSTR};
use wry::{WebView, WebViewExtWindows};

use crate::blocker::BlockerHandle;
use crate::site_scripts::SiteScripts;

/// A tab's sites, with the ids WebView2 gave their scripts.
pub type Sites = Rc<RefCell<SiteScripts<String>>>;

/// Adds the scriptlets of each site `view` navigates to, once per site.
pub fn attach(view: &WebView, blocker: BlockerHandle, sites: Sites) -> windows_core::Result<()> {
    let webview = view.webview();
    let handler = NavigationStartingEventHandler::create(Box::new(move |sender, args| {
        let (Some(sender), Some(args)) = (sender, args) else {
            return Ok(());
        };
        let mut uri = PWSTR::null();
        unsafe { args.Uri(&mut uri)? };
        let uri = take_pwstr(uri);
        let Some(script) = sites
            .borrow_mut()
            .script_for(&uri, |url| blocker.scriptlets(url))
        else {
            return Ok(());
        };
        let ids = sites.clone();
        let done = AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(
            move |result, id| {
                match result {
                    Ok(()) => ids.borrow_mut().keep(id),
                    Err(e) => log::warn!("scriptlets: adding: {e}"),
                }
                Ok(())
            },
        ));
        if let Err(e) = unsafe {
            sender.AddScriptToExecuteOnDocumentCreated(&HSTRING::from(script.as_str()), &done)
        } {
            log::warn!("scriptlets: adding: {e}");
        }
        Ok(())
    }));
    let mut token = 0;
    unsafe { webview.add_NavigationStarting(&handler, &mut token) }
}

/// Removes `sites`' scripts from `view` and forgets its sites (the ad
/// blocker's lists or settings changed).
pub fn clear(view: &WebView, sites: &Sites) {
    let webview = view.webview();
    for id in sites.borrow_mut().clear() {
        if let Err(e) = unsafe { webview.RemoveScriptToExecuteOnDocumentCreated(&HSTRING::from(id)) } {
            log::warn!("scriptlets: removing: {e}");
        }
    }
}
```

Dans `lib.rs`, avec les autres modules `webview2_*` :

```rust
#[cfg(all(windows, feature = "desktop"))]
mod webview2_scriptlets;
```

Si la signature de la closure de `NavigationStartingEventHandler::create` ou de `AddScriptToExecuteOnDocumentCreatedCompletedHandler::create` ne correspond pas (webview2-com 0.39), se caler sur `webview2_address.rs` et `page_injection.rs`, qui utilisent les mêmes formes.

- [ ] **Step 2: Le brancher dans `WryEngine`**

Dans la struct `WryEngine` (`wry_engine.rs`, à côté de `blocked`) :

```rust
    /// Windows: the sites each tab has the ad blocker's scriptlets for.
    #[cfg(windows)]
    scriptlet_sites: HashMap<TabId, crate::webview2_scriptlets::Sites>,
    /// When the scriptlets added so far must go (new lists, new settings).
    scriptlet_changes: crate::blocker::BlockerChanges,
```

Dans `WryEngine::new`, à côté de `blocker_changes: Default::default(),` :

```rust
            #[cfg(windows)]
            scriptlet_sites: HashMap::new(),
            scriptlet_changes: Default::default(),
```

Dans la création d'un onglet, juste après le bloc `webview2_address::attach` :

```rust
        #[cfg(windows)]
        {
            let sites = crate::webview2_scriptlets::Sites::default();
            if let Err(e) =
                crate::webview2_scriptlets::attach(&view, self.blocker.clone(), sites.clone())
            {
                log::warn!("scriptlets of tab {tab}: {e}");
            }
            self.scriptlet_sites.insert(tab, sites);
        }
```

Dans `pump`, avant `self.measure_panel();` :

```rust
        // New lists or settings: the scriptlets added so far go; each site
        // gets the right ones on its next load.
        if self.scriptlet_changes.changed(&self.blocker) {
            self.clear_scriptlets();
        }
```

et la méthode (dans `impl WryEngine`, près de `measure_panel`) :

```rust
    fn clear_scriptlets(&mut self) {
        #[cfg(windows)]
        for (tab, sites) in &self.scriptlet_sites {
            if let Some(view) = self.views.get(tab) {
                crate::webview2_scriptlets::clear(view, sites);
            }
        }
    }
```

Là où un onglet est fermé (chercher `self.blocked.remove(` dans `wry_engine.rs`), ajouter :

```rust
        #[cfg(windows)]
        self.scriptlet_sites.remove(&tab);
```

- [ ] **Step 3: Compiler et lancer tous les tests**

Run: `rtk cargo clippy --all-targets`
Expected: aucune alerte.
Run: `rtk cargo test --workspace`
Expected: PASS.

- [ ] **Step 4: Vérifier sur une build de dev (Windows)**

Les listes uBlock et `resources.json` doivent être téléchargées : une fenêtre privée ne télécharge pas, on lance donc un profil de test jetable (le dossier `blocker/` est commun aux profils ; `due` est vrai puisque les nouveaux fichiers manquent).

```powershell
rtk cargo build
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS='--remote-debugging-port=9333'
$p = Start-Process .\target\debug\browsingUnicorns.exe -ArgumentList '--profile','scriptlets-test','https://www.youtube.com/watch?v=aqz-KE-bpKQ' -PassThru
```

Attendre que `%APPDATA%\browsing-unicorns\blocker\resources.json` et `ublock-quick-fixes.txt` existent (refermer et relancer l'app une fois s'ils arrivent après le premier chargement : les scriptlets s'appliquent dès la page suivante). Puis, via le DevTools (port 9333, `Runtime.evaluate` sur la page YouTube) :

- `!!(window.ytInitialPlayerResponse && window.ytInitialPlayerResponse.adPlacements && window.ytInitialPlayerResponse.adPlacements.length)` → attendu `false` ;
- comparer avec le bouclier coupé pour le site (« Bloquer sur ce site » désactivé, la page se recharge) : la même expression vaut `true` sur une vidéo monétisée, ce qui prouve que c'est bien le scriptlet qui agit ;
- `Page.captureScreenshot` après ~10 s de lecture : pas de pub.

Fermer l'app, supprimer le profil de test (`%APPDATA%\browsing-unicorns\profiles\scriptlets-test`), ne pas toucher au reste.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/infrastructure/src/webview2_scriptlets.rs crates/infrastructure/src/lib.rs crates/infrastructure/src/wry_engine.rs
rtk git commit -m "feat(blocker): scriptlets injected at navigation start (Windows)"
```

---

### Task 5 : macOS et Linux

**Files:**
- Create: `crates/infrastructure/src/webkit_scriptlets.rs`
- Modify: `crates/infrastructure/src/lib.rs`, `crates/infrastructure/src/wry_engine.rs`

Ces plateformes ne peuvent pas être lancées depuis Windows : le code doit compiler en CI (`.github/workflows/ci.yml`), et suivre de près `page_injection.rs`, qui fait déjà la même chose avec les mêmes API.

- [ ] **Step 1: Le module WebKit**

`crates/infrastructure/src/webkit_scriptlets.rs` :

```rust
//! macOS, Linux: the ad blocker's scriptlets, added from wry's navigation
//! handler (WebKit's navigation decision), before the new document exists.

use std::cell::RefCell;
use std::rc::Rc;

use crate::blocker::BlockerHandle;
use crate::site_scripts::SiteScripts;

pub use platform::Native;

/// A tab's sites and its web view, set once the view exists.
#[derive(Clone, Default)]
pub struct Sites {
    sites: Rc<RefCell<SiteScripts<platform::Handle>>>,
    native: Rc<RefCell<Option<Native>>>,
}

impl Sites {
    /// The web view, once built.
    pub fn set_native(&self, native: Native) {
        *self.native.borrow_mut() = Some(native);
    }

    /// From wry's navigation handler: adds `url`'s site's scriptlets.
    pub fn navigating(&self, url: &str, blocker: &BlockerHandle) {
        let native = self.native.borrow();
        let Some(native) = native.as_ref() else {
            return;
        };
        let Some(script) = self
            .sites
            .borrow_mut()
            .script_for(url, |u| blocker.scriptlets(u))
        else {
            return;
        };
        let handle = platform::add(native, &script);
        self.sites.borrow_mut().keep(handle);
    }

    /// Removes the scripts and forgets the sites.
    pub fn clear(&self) {
        let handles = self.sites.borrow_mut().clear();
        if let Some(native) = self.native.borrow().as_ref() {
            platform::remove(native, handles);
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use webkit2gtk::{
        UserContentInjectedFrames, UserContentManager, UserContentManagerExt, UserScript,
        UserScriptInjectionTime,
    };

    pub type Native = UserContentManager;
    pub type Handle = UserScript;

    pub fn add(manager: &Native, script: &str) -> Handle {
        let user_script = UserScript::new(
            script,
            UserContentInjectedFrames::AllFrames,
            UserScriptInjectionTime::Start,
            &[],
            &[],
        );
        manager.add_script(&user_script);
        user_script
    }

    pub fn remove(manager: &Native, handles: Vec<Handle>) {
        for script in handles {
            manager.remove_script(&script);
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_foundation::NSString;
    use objc2_web_kit::{WKUserContentController, WKUserScript, WKUserScriptInjectionTime};

    use crate::site_scripts::MARKER;

    pub type Native = Retained<WKUserContentController>;
    /// Found again by their [`MARKER`].
    pub type Handle = ();

    pub fn add(controller: &Native, script: &str) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        unsafe {
            let user_script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                WKUserScript::alloc(mtm),
                &NSString::from_str(script),
                WKUserScriptInjectionTime::AtDocumentStart,
                false,
            );
            controller.addUserScript(&user_script);
        }
    }

    pub fn remove(controller: &Native, _handles: Vec<Handle>) {
        unsafe {
            // WebKit removes all the scripts or none: keep the others.
            let others: Vec<Retained<WKUserScript>> = controller
                .userScripts()
                .to_vec()
                .into_iter()
                .filter(|s| !s.source().to_string().starts_with(MARKER))
                .collect();
            controller.removeAllUserScripts();
            for s in &others {
                controller.addUserScript(s);
            }
        }
    }
}
```

Dans `lib.rs` :

```rust
#[cfg(all(any(target_os = "macos", target_os = "linux"), feature = "desktop"))]
mod webkit_scriptlets;
```

Vérifier que `page_injection.rs` (macOS) garde nos scripts quand il remplace les siens : il retire ceux qui commencent par `page_scripts::MARKER` ; `site_scripts::MARKER` est différent, donc les nôtres restent. Le confirmer en lisant `page_scripts::MARKER`.

- [ ] **Step 2: Le brancher dans `WryEngine`**

Champ :

```rust
    /// macOS, Linux: the sites each tab has the ad blocker's scriptlets for.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    scriptlet_sites: HashMap<TabId, crate::webkit_scriptlets::Sites>,
```

initialisé à `HashMap::new()` sous le même `cfg` dans `new`.

Dans la création d'un onglet, à côté du `#[cfg(windows)] let builder = { … with_navigation_handler … }` existant :

```rust
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let sites = crate::webkit_scriptlets::Sites::default();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        let builder = {
            let (sites, blocker) = (sites.clone(), self.blocker.clone());
            builder.with_navigation_handler(move |url| {
                sites.navigating(&url, &blocker);
                true
            })
        };
```

Après `build_as_child` (près de `self.rule_lists.attach(&view);`) :

```rust
        #[cfg(target_os = "linux")]
        {
            use wry::WebViewExtUnix;
            use webkit2gtk::WebViewExt;
            if let Some(manager) = view.webview().user_content_manager() {
                sites.set_native(manager);
            }
        }
        #[cfg(target_os = "macos")]
        {
            use wry::WebViewExtMacOS;
            sites.set_native(view.manager());
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        self.scriptlet_sites.insert(tab, sites);
```

Note : la toute première navigation (`load_url` juste après) passe par le gestionnaire de navigation **après** `set_native`, donc elle est couverte.

Dans `clear_scriptlets` :

```rust
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        for sites in self.scriptlet_sites.values() {
            sites.clear();
        }
```

À la fermeture d'un onglet :

```rust
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        self.scriptlet_sites.remove(&tab);
```

- [ ] **Step 3: Compiler pour Windows (le code WebKit est exclu) et pousser sur une branche pour la CI**

Run: `rtk cargo clippy --all-targets` → aucune alerte.
La compilation macOS/Linux est vérifiée par la CI au push de la branche de travail (`rtk gh run list`), à corriger jusqu'au vert avant la fusion.

- [ ] **Step 4: Commit**

```bash
rtk git add crates/infrastructure/src/webkit_scriptlets.rs crates/infrastructure/src/lib.rs crates/infrastructure/src/wry_engine.rs
rtk git commit -m "feat(blocker): scriptlets injected at navigation start (macOS, Linux)"
```

---

### Task 6 : Android

**Files:**
- Modify: `crates/mobile/src/extensions.rs`, `crates/mobile/src/tests.rs`
- Modify: `android/app/src/main/java/io/github/ajustor/browsingunicorns/WebViews.kt`, `BrowserController.kt`

- [ ] **Step 1: Test Rust qui échoue**

Dans `crates/mobile/src/tests.rs`, après `the_ad_blocker_blocks_and_counts` :

```rust
#[test]
fn the_ad_blocker_gives_a_sites_scriptlets() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, _host) = browser(&dir, "phone", &http);
    assert_eq!(b.scriptlets("https://news.test/".into()), "");
    {
        let mut answers = http.0.lock().unwrap();
        answers.insert(
            infrastructure::blocker::LISTS[2].1.to_owned(),
            b"news.test##+js(say, hello)\n".to_vec(),
        );
        answers.insert(
            infrastructure::blocker::RESOURCES.1.to_owned(),
            br#"[{"name":"say.js","aliases":[],"kind":{"mime":"application/javascript"},"content":"d2luZG93LnNhaWQgPSAie3sxfX0iOw=="}]"#.to_vec(),
        );
    }
    b.update_blocker_lists();
    let script = b.scriptlets("https://news.test/a".into());
    assert!(script.contains(r#"location.hostname === "news.test""#), "{script}");
    assert!(script.contains("window.said"), "{script}");
    assert_eq!(b.scriptlets("https://other.test/".into()), "");
    b.set_blocker(false).unwrap();
    assert_eq!(b.scriptlets("https://news.test/".into()), "");
}
```

Run: `rtk cargo test -p mobile scriptlets`
Expected: FAIL à la compilation (`scriptlets` inconnu).

- [ ] **Step 2: L'export Rust**

Dans `crates/mobile/src/extensions.rs`, dans le bloc `#[uniffi::export] impl MobileBrowser`, après `intercept` :

```rust
    /// The ad blocker's scriptlets for a page about to load at `url`, run
    /// on its host only; empty when there are none (blocker off, site
    /// allowlisted, no list yet). Registered by the app at document start.
    pub fn scriptlets(&self, url: String) -> String {
        let Some(host) = infrastructure::site_scripts::web_host(&url) else {
            return String::new();
        };
        let script = self.blocker.scriptlets(&url);
        if script.is_empty() {
            return String::new();
        }
        infrastructure::site_scripts::guarded(&host, &script)
    }
```

Run: `rtk cargo test -p mobile`
Expected: PASS.

- [ ] **Step 3: Kotlin, enregistrement par origine**

Dans `WebViews.kt`, à côté de `injected` :

```kotlin
    /** The ad blocker's scriptlets added to each tab, by host. */
    private val scriptlets = HashMap<ULong, HashMap<String, ScriptHandler>>()

    /** Older WebViews: each tab's scriptlets for its next page, run in onPageStarted. */
    private val pendingScriptlets = HashMap<ULong, String>()

    /**
     * Before [url] loads in [tab]: its site's scriptlets, at document start
     * for that origin only (once per host).
     */
    private fun prepareScriptlets(tab: ULong, view: WebView, url: String) {
        val uri = android.net.Uri.parse(url)
        val scheme = uri.scheme ?: return
        val host = uri.host ?: return
        if (scheme != "http" && scheme != "https") return
        val added = scriptlets.getOrPut(tab) { HashMap() }
        if (host in added) return
        val script = runCatching { browser.scriptlets(url) }.getOrDefault("")
        if (script.isEmpty()) return
        if (documentStart) {
            added[host] = WebViewCompat.addDocumentStartJavaScript(view, script, setOf("$scheme://$host"))
        } else {
            pendingScriptlets[tab] = script
        }
    }

    /** The ad blocker changed: the scriptlets added so far go. */
    fun clearScriptlets() {
        for (added in scriptlets.values) added.values.forEach { it.remove() }
        scriptlets.clear()
        pendingScriptlets.clear()
    }
```

Appels :
- `openView`, juste avant `view.loadUrl(url)` : `prepareScriptlets(tab, view, url)` ;
- `navigate` : remplacer le corps par
  ```kotlin
          val view = views[tab] ?: return
          prepareScriptlets(tab, view, url)
          view.loadUrl(url)
  ```
- `shouldOverrideUrlLoading`, en tête de la méthode :
  ```kotlin
                if (request.isForMainFrame) prepareScriptlets(tab, view, request.url.toString())
  ```
- `onPageStarted`, après la ligne des vieux WebViews :
  ```kotlin
                pendingScriptlets.remove(tab)?.let { view.evaluateJavascript(it, null) }
  ```
- `closeView` : `scriptlets.remove(tab)` et `pendingScriptlets.remove(tab)`.

Dans `BrowserController.kt`, `setBlocker` et `setBlockerForSite` :

```kotlin
    fun setBlocker(enabled: Boolean) {
        act {
            setBlocker(enabled)
            views.clearScriptlets()
            reload()
        }
    }
```

(même ajout de `views.clearScriptlets()` avant `reload()` dans `setBlockerForSite`). Vérifier le nom réel du champ `WebViews` dans `BrowserController` (`views`, utilisé par `openTabs`).

- [ ] **Step 4: Compiler l'app Android**

Run (depuis `android/`) : `./gradlew :app:assembleDebug` (génère aussi les liaisons uniffi).
Expected: BUILD SUCCESSFUL. Si la tâche Gradle n'est pas disponible localement, s'appuyer sur la CI.

- [ ] **Step 5: Commit**

```bash
rtk git add crates/mobile/src/extensions.rs crates/mobile/src/tests.rs android/app/src/main/java/io/github/ajustor/browsingunicorns/WebViews.kt android/app/src/main/java/io/github/ajustor/browsingunicorns/BrowserController.kt
rtk git commit -m "feat(android): the ad blocker's scriptlets at document start"
```

---

### Task 7 : bouclier, changelog

**Files:**
- Modify: `src/ui/shield.rs:98,105`, `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/Shield.kt:93`, `CHANGELOG.md`

- [ ] **Step 1: Nom des listes**

`src/ui/shield.rs` : remplacer les deux `"Lists: EasyList, EasyPrivacy` par `"Lists: EasyList, EasyPrivacy, uBlock filters`. Si un test de `shield.rs` compare ce texte, l'adapter de la même façon.
`Shield.kt` : `"Lists: EasyList, EasyPrivacy, uBlock filters · "`.

- [ ] **Step 2: CHANGELOG**

Sous `# Changelog` et son paragraphe, avant `## [0.15.1]`, ajouter :

```markdown
## [Non publié]

### Ajouté

- Le bloqueur de pubs retire les pubs de YouTube : il charge en plus les
  listes d'uBlock Origin (*uBlock filters*, *Quick fixes*) et leurs
  scriptlets, de petits scripts lancés avant ceux de la page pour en
  retirer les pubs. Ils valent pour tous les sites que ces listes
  couvrent, sur ordinateur et Android ; « Bloquer sur ce site » les coupe
  comme le reste.
```

- [ ] **Step 3: Vérification finale**

Run: `rtk cargo clippy --all-targets` → aucune alerte.
Run: `rtk cargo test --workspace` → PASS.

- [ ] **Step 4: Commit**

```bash
rtk git add src/ui/shield.rs android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/Shield.kt CHANGELOG.md
rtk git commit -m "docs: changelog and shield for the uBlock lists"
```
