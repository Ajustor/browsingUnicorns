# Built-in ad blocker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `docs/superpowers/specs/2026-11-16-ad-blocker-design.md`: EasyList + EasyPrivacy blocking (network + site-specific cosmetic filters) on Windows, macOS, Linux and Android, with a per-site shield and a counter.

**Architecture:** A `blocker` module in `crates/infrastructure` wraps `adblock::Engine` (adblock-rust 0.13): lists downloaded and cached under the app's data root, engine built off the UI thread and shared as `Arc<Blocker>` behind an `Arc<RwLock<…>>` handle. The existing request hooks consult it after the extensions' rules (Windows `webview2_blocking.rs`, Android `MobileBrowser::intercept`), WebKit gets its content-blocking conversion through `webkit_rules.rs`, a page script asks the hub for the site's cosmetic selectors. Settings (`domain::Settings`) gain `blocker` and `blocker_allowlist`. The shield is an egui toolbar button + panel on desktop, a Compose icon + dialog on Android.

**Tech Stack:** Rust 2024, adblock 0.13 (feature `content-blocking`), egui 0.31, wry 0.57 / WebView2, UniFFI (mobile crate), Kotlin + Compose (Android).

**Conventions (read first):**
- Windows machine, PowerShell for cargo (the Bash tool's PATH lacks `sccache`). Android: `android\gradlew.bat` from `android`; regenerate the UniFFI bindings after changing the mobile crate's API (commands in `scripts/build-android.ps1`: `cargo build -p mobile` then `cargo run -q -p mobile --features cli --bin uniffi-bindgen -- generate --library target\debug\mobile.dll --language kotlin --no-format --out-dir android\app\src\main\java`).
- Branch `master`. One commit per task, conventional subject, short English body. The user is the only author: **no `Co-Authored-By` trailer**. Do not push.
- Each task ends green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -q --workspace`; for Android tasks also `.\gradlew.bat -q :app:compileDebugKotlin :app:testDebugUnitTest :app:lintVitalRelease`.
- macOS/Linux-only code (`#[cfg(any(target_os = "macos", target_os = "linux"))]`) can't be compiled here: keep it small, put the logic it calls in platform-independent, tested functions, and say in the report that CI will compile it.
- Never launch the GUI; the coordinator does.
- adblock-rust API: read the crate sources in `~/.cargo/registry/src/*/adblock-0.13*/` (`src/engine.rs`: `Engine::from_filter_set`, `check_network_request`, `url_cosmetic_resources`, `serialize`/`deserialize`; `src/request.rs`: `Request::new(url, source_url, request_type)`; `src/content_blocking.rs`) — the snippets below show intent; adapt names to the real API.

---

### Task 1: Settings fields

**Files:** `crates/domain/src/settings.rs`, `crates/infrastructure/src/settings_store.rs`, `crates/infrastructure/src/dto.rs` (sync record, if settings are carried there), their tests.

- [ ] Add to `Settings`: `pub blocker: bool` (default `true`) and `pub blocker_allowlist: Vec<String>` (default empty). Add `Settings::blocks_on(&self, host: &str) -> bool`: `false` if `!blocker` or if `host` equals an allowlisted host or ends with `.{allowlisted}`; host compare in ASCII lowercase, leading `www.` ignored on both sides.
- [ ] `SettingsFile` (TOML) and the sync DTO: new fields with `#[serde(default)]` (`blocker` defaulting to `true` through a `fn yes() -> bool`), so older files and older devices' records still load.
- [ ] Tests: `blocks_on` (subdomain, `www.`, case, disabled), an old TOML without the fields loads with defaults, round trip, sync merge keeps the newer record's allowlist.
- [ ] Commit `feat(settings): ad blocker switch and per-site allowlist`.

### Task 2: The blocker engine

**Files:** create `crates/infrastructure/src/blocker.rs` (+ `blocker/lists.rs` if it grows); `crates/infrastructure/Cargo.toml` (`adblock = { version = "0.13", features = ["content-blocking"] }` — check `default-features`: keep `embedded-domain-resolver`, `full-regex-handling`; `single-thread` is fine since the engine is used behind a lock), `lib.rs`.

- [ ] `pub struct Blocker { engine: adblock::Engine, updated: SystemTime }` with:
  - `pub fn from_lists(lists: &[&str]) -> Option<Self>` (parse with `FilterSet::new(false)` + `add_filters(…, ParseOptions::default())`; `None` when no rule parsed);
  - `pub fn should_block(&self, url: &str, page_url: &str, kind: ResourceKind) -> bool` — maps our `domain::ResourceType` (or `None`) to adblock's request type strings (`"script"`, `"image"`, `"stylesheet"`, `"xmlhttprequest"`, `"sub_frame"`, `"media"`, `"font"`, `"ping"`, `"websocket"`, `"other"`); `MainFrame` → never block; honours exceptions (adblock's result `matched && exception.is_none()` — check `BlockerResult`);
  - `pub fn cosmetic_css(&self, url: &str) -> String` — from `url_cosmetic_resources(url).hide_selectors` (site-specific only; ignore `generichide` details except honouring it), `"sel1,sel2{display:none!important}"` in chunks of ~500 selectors, empty when none;
  - `pub fn serialize(&self) -> Vec<u8>` / `pub fn deserialize(bytes) -> Option<Self>`.
- [ ] `pub struct BlockerHandle(Arc<RwLock<Option<Arc<Blocker>>>>)` — `get()`, `set()`; cloned into every hook.
- [ ] `pub struct BlockerLists { dir: PathBuf, download: Download (reuse extensions' type: Fn(&str) -> Result<Vec<u8>, String>) }`:
  - `load_cached(&self) -> Option<Blocker>`: `engine.dat` if it deserializes, else rebuild from `easylist.txt` + `easyprivacy.txt` if present;
  - `refresh(&self) -> Result<Blocker, String>`: download both lists, keep the old file when a download fails or parses to nothing, write files atomically (`files::write`-style temp + rename), rebuild, write `engine.dat`, record `last_check` (unix seconds) in `state.json`;
  - `due(&self) -> bool`: no `last_check` or older than a day.
  - Lists: `https://easylist.to/easylist/easylist.txt`, `https://easylist.to/easylist/easyprivacy.txt`.
- [ ] Data root: `<data dir>/browsing-unicorns/blocker/` — the default profile's dir (`infrastructure::profile_dir()`) is the app root; use `profile_dir().map(|d| d.join("blocker"))` on desktop; the mobile crate passes its own data dir.
- [ ] Tests (inline lists, no network): ad URL blocked, normal URL allowed, `@@` exception allowed, main frame never blocked, `cosmetic_css("https://a.test/")` contains `a.test##.ad` selector and not `b.test`'s, serialize → deserialize decides the same, `refresh` with a fake download keeps the old list when the new one is empty/fails, `due` after a day.
- [ ] Commit `feat(blocker): EasyList and EasyPrivacy engine (adblock-rust)`.

### Task 3: Windows hook, counter, startup

**Files:** `crates/infrastructure/src/webview2_blocking.rs`, `wry_engine.rs`, `src/bootstrap.rs`, `src/ui/app.rs`.

- [ ] `webview2_blocking::attach` takes also `blocker: BlockerHandle`, `settings: Arc<RwLock<Settings>>` (or a small `Arc<RwLock<BlockerPolicy { enabled, allowlist }>>` updated from the app when settings change) and `blocked: Rc<Cell<u32>>`. After the extensions' rules allow a request (or when they're empty), if the policy blocks on the page's host and `blocker.should_block(url, page, type)` → same 403 empty response, `blocked += 1`. Main-frame navigation of the tab resets `blocked` to 0 (the navigation handler already updates `page`).
- [ ] Attach it on every tab, whether or not the browser runs extensions itself (today it's only attached when `builtin_extensions()`); the extensions' rules part stays as is.
- [ ] `WryEngine`: `blocked: HashMap<TabId, Rc<Cell<u32>>>`; `pub fn blocked_count(&self, tab) -> Option<u32>` (`None` on macOS/Linux).
- [ ] Bootstrap: `BlockerHandle::default()`, load the cached engine synchronously if cheap (deserialize) else on a thread; if `due()`, refresh on a thread; on success `handle.set(...)` and wake the UI. Re-check `due()` when the app regains focus (a `last_check` like the updater's).
- [ ] Commit `feat(blocker): block ads and trackers in WebView2, count per tab`.

### Task 4: Cosmetic filters through a page script

**Files:** `crates/infrastructure/src/page_scripts/` (a small built-in script, injected into every tab like user scripts, with the `blocker` owner), `extension_hub.rs` (op `blocker.cosmetic`), `runtime.js` if needed.

- [ ] Script: at `document_start`, `send("blocker", "blocker.cosmetic", { url: location.href })` → CSS string; if non-empty, append a `<style>` (re-append if the page removes it is not needed). Top frame only.
- [ ] Hub: `Hub::set_blocker(handle, policy)`; op `blocker.cosmetic` (any context) → `""` if policy says no for the URL's host, else `blocker.cosmetic_css(url)`. `blocker` must pass `valid_owner` — extend it to accept `"blocker"` or reuse an existing owner scheme; keep the token check.
- [ ] Tests: hub answers CSS for a listed site, empty when allowlisted or disabled.
- [ ] Commit `feat(blocker): hide ad slots (site-specific cosmetic filters)`.

### Task 5: WebKit (macOS, Linux)

**Files:** `crates/infrastructure/src/blocker.rs` (conversion), `webkit_rules.rs`.

- [ ] Platform-independent `pub fn webkit_rules(lists: &[&str], allowlist: &[String]) -> String` (JSON): adblock's `content_blocking` conversion of the network rules, plus, per allowlisted host, `{"trigger":{"url-filter":".*","if-domain":["*<host>"]},"action":{"type":"ignore-previous-rules"}}` at the end. Tests on the JSON (Windows runs them).
- [ ] `webkit_rules.rs`: compile it as one more content rule list (identifier `bu-blocker`) next to the extensions' lists, recompiled when the lists or the allowlist change; added to every view. Small `#[cfg]` glue only.
- [ ] Commit `feat(blocker): WebKit content rules (macOS, Linux)`.

### Task 6: Desktop shield

**Files:** `src/ui/toolbar.rs`, create `src/ui/shield.rs`, `src/ui/command.rs`, `src/ui/app.rs`.

- [ ] Toolbar: shield button (`egui_phosphor` `SHIELD_CHECK` when blocking on this site, `SHIELD_SLASH` when not) right after the padlock; badge with the count when > 0 (small rounded label over the icon's corner, `theme::CYAN` background, `theme::NIGHT` text).
- [ ] Click toggles `Command::ToggleShield`; the panel (`TopBottomPanel::top("shield")` under the toolbar, like `site_info.rs`; close the site info bar when opening it and vice versa) shows the spec's lines; `Command::SetBlockerForSite(host, bool)`, `Command::SetBlocker(bool)` update `Settings` through `browser.update_settings`, refresh the policy handed to the engine and the hub, and reload the active tab.
- [ ] Commit `feat(blocker): toolbar shield with counter and per-site switch`.

### Task 7: Android

**Files:** `crates/mobile/src/lib.rs` (+ tests), `android/app/src/main/java/io/github/ajustor/browsingunicorns/ui/App.kt`, `BrowserController.kt`, `WebViews.kt`.

- [ ] Mobile crate: own `BlockerHandle`, lists under `<dataDir>/blocker`, loaded/refreshed on a thread at start (the `download` of the injected `HttpClient`), `intercept` consults it after the extensions' rules (main frame never), per-tab counter (atomic, reset when a main-frame request for the tab arrives); expose `blocker_info(tab) -> BlockerInfo { enabled, site_allowed, blocked, lists_updated: Option<i64> }` and `set_blocker(enabled)`, `set_blocker_for_site(host, enabled)`; cosmetic CSS through the same page script.
- [ ] Kotlin: shield icon in the address bar's trailing slot (before reload), dialog with the same content; toggles call the API then reload.
- [ ] Commit `feat(android): built-in ad blocker and shield`.

### Task 8: Docs

- [ ] README features (🛡️ line), CHANGELOG `[Unreleased]` → « Ajouté : bloqueur de pubs intégré… » in French like the rest.
- [ ] Commit `docs: built-in ad blocker`.
