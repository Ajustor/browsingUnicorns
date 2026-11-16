# Private browsing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement `docs/superpowers/specs/2026-11-16-private-browsing-design.md`: a private window on desktop, private tabs on Android.

**Architecture:** Desktop: `--private` starts a window whose `Profile` uses in-memory repositories for history and session, a read-only bookmarks repository, an in-memory settings repository seeded from the profile's, a temporary WebView data dir deleted on exit, and no extensions, user scripts, sync or updates. The UI knows it is private (theme, notices, disabled views). Android: the Rust core marks tabs private (excluded from history, session, sync); Kotlin gives private tabs a separate WebView `Profile` and a Normal/Private switch in the tabs grid.

**Tech Stack:** Rust 2024, egui 0.31, wry 0.57 (WebView2 / WebKit), UniFFI, Kotlin + Compose, androidx.webkit 1.13 (`ProfileStore`, `WebViewCompat.setProfile`, `WebViewFeature.MULTI_PROFILE`).

**Conventions (read first):**
- Windows machine; cargo and Gradle through PowerShell (the Bash tool lacks `sccache`). Regenerate the UniFFI Kotlin bindings after changing the mobile crate's API (see `scripts/build-android.ps1`); never commit them.
- Branch `master` (your worktree). One commit per task, conventional subject, short English body. The user is the only author: **no `Co-Authored-By` trailer**. Do not push.
- Each task ends green: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -q --workspace`; Android tasks also `.\gradlew.bat -q :app:compileDebugKotlin :app:testDebugUnitTest :app:lintVitalRelease` (if `testDebugUnitTest` fails on `java.library.path`, strip quoted entries from PATH and use `--no-daemon`, as the last agent found).
- Never launch the GUI or an emulator.

---

### Task 1: In-memory and read-only repositories

**Files:** `crates/infrastructure/src/` (new `memory_store.rs` or inside `json_store.rs`), `crates/application/src/ports.rs` if a trait needs it, tests.

- [ ] `MemoryRepository<T: Clone + Default>`: `load` returns what was last saved (initially a given value), `save` keeps it in memory.
- [ ] `ReadOnlyRepository<T>`: wraps a real repository; `load` delegates, `save` returns `RepositoryError("Not available in a private window.")`.
- [ ] `infrastructure::private_profile(dir, settings: Settings) -> Profile`: history and session `MemoryRepository` (empty), bookmarks `ReadOnlyRepository<JsonRepository>` on `dir/bookmarks.json`, settings `MemoryRepository` seeded with the profile's settings.
- [ ] Tests: saving through the private profile leaves `history.json`/`session.json`/`settings.toml` untouched (compare bytes before/after), bookmark save errors with that message, settings changes are visible in memory.
- [ ] Commit `feat(private): in-memory and read-only profile storage`.

### Task 2: Temporary engine data, leftovers

**Files:** `crates/infrastructure/src/private_data.rs` (new), `lib.rs`.

- [ ] `PrivateDataDir::create() -> io::Result<Self>`: `std::env::temp_dir()/browsing-unicorns-private-<pid>-<random>` (random from `getrandom`/`rand` already in the tree, or the time's nanos); `path()`; `Drop` removes it recursively (log on failure).
- [ ] `clean_leftovers()`: for each `browsing-unicorns-private-<pid>-*` in temp whose pid is not a running process (Windows: `OpenProcess` via the `windows` crate or `tasklist`-free check with `sysinfo`-free approach — use `std::process::Command` only if nothing simpler; macOS/Linux: `/proc/<pid>` or `kill(pid, 0)` via `libc`), remove it. Keep the platform check in one small function.
- [ ] Tests: create → path exists → drop → gone; a fake leftover with an impossible pid is removed, one with the current pid is kept.
- [ ] Commit `feat(private): temporary web view data, removed at exit`.

### Task 3: `--private` on desktop

**Files:** `src/main.rs` (`Args`), `src/bootstrap.rs`, `src/ui/app.rs`, `src/ui/library.rs`, `src/ui/sync.rs`/`extensions.rs`/`profiles.rs` (gates), `src/ui/updates.rs`, `crates/infrastructure/src/profiles.rs` (`open_private_window`).

- [ ] `Args.private: bool` from `--private` (parse test).
- [ ] Bootstrap, when private: no `ProfileLock`; `clean_leftovers()` (also run it for normal windows); `PrivateDataDir` kept alive in the app (dropped after the web views at exit); `EngineFiles.data_dir` = its path; `private_profile(...)`; no `restore_session` (open the start page or home page); extensions and user scripts: empty `Extensions`/`UserScripts` handed to the engine, runners in a disabled state; sync runner not started; updater not checking (never `check_if_due`); blocker: load cached engine only, never refresh.
- [ ] `open_private_window(id)` (spawn exe with `--profile <id> --private`); `Command::OpenPrivateWindow`, Ctrl+Shift+N in `shortcuts.rs`, a mask button left of the avatar in `toolbar.rs`.
- [ ] Bookmark toggling in private: the error becomes the notice "Not available in a private window." (it already reports repository errors — check the text).
- [ ] Library views Sync, Extensions, Profiles show "Not available in a private window." when private.
- [ ] Window title ends with " (private)".
- [ ] Tests: `Args` parsing; a bootstrap-level test is not practical — cover the decision logic in a small pure function `fn private_setup(private: bool) -> Setup { restore_session, extensions, sync, updates, blocker_refresh }` with a test.
- [ ] Commit `feat(private): private window (Ctrl+Shift+N)`.

### Task 4: Private look (desktop)

**Files:** `src/ui/theme.rs`, `src/ui/tab_strip.rs`, `src/ui/app.rs`.

- [ ] `theme::Chrome { gradient: [Color32; 3], night: Color32, ... }` with `NORMAL` (today's values) and `PRIVATE` (`#2b2b33`, `#3a3a46`, `#2b2b33`, bars `#1f1f26`, hover `#34343f`); the tab strip, toolbar and bookmarks bar use the app's chrome instead of the constants directly. Keep the contrast tests and add the private palette to them.
- [ ] A "Private" pill with the mask icon at the left of the tabs when private.
- [ ] Commit `feat(private): dark grey look for private windows`.

### Task 5: Android private tabs (core)

**Files:** `crates/mobile/src/lib.rs` (+ tests), `crates/application/src/browser.rs` / `crates/domain/src/session.rs` if tabs need a `private` flag there.

- [ ] Tabs carry `private: bool` (domain `Tab` or a side set in the mobile crate — prefer the domain if `Session` snapshots/history recording live there: `SessionSnapshot` skips private tabs; `LoadFinished` on a private tab doesn't record history; the sync record skips them).
- [ ] UniFFI: `open_tab(url, private)`; `TabInfo.private`; `close_private_tabs()` (called at app stop).
- [ ] Tests: a private tab's load isn't in history, the session snapshot and the sync record don't contain it, closing private tabs leaves the normal ones.
- [ ] Commit `feat(android): private tabs in the core`.

### Task 6: Android private tabs (UI)

**Files:** `android/.../WebViews.kt`, `ui/Tabs.kt`, `ui/App.kt`, `BrowserController.kt`, `MainActivity.kt`.

- [ ] `WebViews`: for a private tab, before its first load, `WebViewCompat.setProfile(view, "bu-private")` (create through `ProfileStore.getInstance().getOrCreateProfile`); when the last private view is destroyed, and at app start, `ProfileStore.getInstance().deleteProfile("bu-private")` (catch `IllegalStateException` if still in use). Only when `WebViewFeature.isFeatureSupported(MULTI_PROFILE)`; expose `privateSupported`.
- [ ] Tabs grid: segmented "Normal / Private" switch in the header (hidden when not supported); `+` opens in the shown series; the bottom bar counter counts the active tab's series.
- [ ] Private look: when the active tab is private, top block gradient `#2b2b33 → #3a3a46 → #2b2b33`, bottom bar `#1f1f26`, mask icon in the address bar.
- [ ] `onStop`/app close: `close_private_tabs()` is NOT wanted on every stop (Android stops apps often); close them when the user swipes the app away is not detectable — so: keep them while the process lives; at process start, there are none (they're not in the session). Document this.
- [ ] Commit `feat(android): private tabs`.

### Task 7: Docs

- [ ] README (🕶️ line), CHANGELOG `[Unreleased]` in French like the rest.
- [ ] Commit `docs: private browsing`.
