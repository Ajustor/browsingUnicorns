//! The built-in ad blocker: EasyList, EasyPrivacy and uBlock Origin's own
//! lists through adblock-rust (Brave's engine), with uBlock Origin's
//! scriptlets for the lists' `+js(...)` rules, TwitchAdSolutions' vaft on
//! Twitch and our own script on Netflix. The lists are downloaded and cached under the app's data folder
//! with the compiled engine; the request
//! hooks (WebView2, Android), the cosmetic page script and the scriptlet
//! injection consult it through a [`BlockerHandle`].

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions, RuleTypes, parse_filter};
use adblock::request::Request;
use adblock::resources::{PermissionMask, Resource};
use domain::{ResourceType, Settings, WebAddress, allowlisted};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::extensions::Download;
use crate::extensions::safari::MAX_RULES;
use crate::files;

/// The lists: file name in the cache, address, whether their scriptlets are
/// trusted (uBlock Origin's own lists may use the `trusted-*` ones, as
/// uBlock Origin lets them).
pub const LISTS: [(&str, &str, bool); 4] = [
    (
        "easylist.txt",
        "https://easylist.to/easylist/easylist.txt",
        false,
    ),
    (
        "easyprivacy.txt",
        "https://easylist.to/easylist/easyprivacy.txt",
        false,
    ),
    (
        "ublock-filters.txt",
        "https://ublockorigin.github.io/uAssets/filters/filters.txt",
        true,
    ),
    (
        "ublock-quick-fixes.txt",
        "https://ublockorigin.github.io/uAssets/filters/quick-fixes.txt",
        true,
    ),
];

/// The code of the scriptlets the lists name: uBlock Origin's, converted to
/// adblock-rust's format each day by the project
/// (`.github/workflows/ublock-resources.yml`): file name in the cache,
/// address.
pub const RESOURCES: (&str, &str) = (
    "ublock-resources.json",
    "https://github.com/Ajustor/browsingUnicorns/releases/download/ublock-resources/ublock-resources.json",
);

/// vaft (TwitchAdSolutions, ryanbr's fork): swaps Twitch's ad breaks for an
/// ad-free stream; run on Twitch's pages: file name in the cache, address.
pub const TWITCH: (&str, &str) = (
    "twitch-vaft.user.js",
    "https://raw.githubusercontent.com/ryanbr/TwitchAdSolutions/master/vaft/vaft.user.js",
);

/// Netflix's ads plan: ad breaks muted and sped up, the pause ad closed.
/// Ours, shipped with the app: Netflix's ads come with the film, no list
/// blocks them.
const NETFLIX: &str = include_str!("netflix_ads.js");

const ENGINE_FILE: &str = "engine.dat";
const STATE_FILE: &str = "state.json";
/// How often the lists are checked.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Selectors per CSS rule: one invalid selector drops its whole rule.
const SELECTORS_PER_RULE: usize = 500;
/// An address no list names: its cosmetic selectors are the generic ones.
const NO_SITE: &str = "https://no-site.invalid/";

/// A compiled set of lists.
pub struct Blocker {
    engine: Engine,
    /// Selectors adblock hands every site; left out, only the site-specific
    /// ones are applied.
    generic: HashSet<String>,
    /// When the lists were downloaded.
    updated: SystemTime,
    /// vaft's code ([`TWITCH`]), run on Twitch.
    twitch: Option<String>,
}

/// `list` has at least one rule adblock understands.
fn has_rules(list: &str) -> bool {
    list.lines()
        .any(|line| parse_filter(line, false, ParseOptions::default()).is_ok())
}

/// The least code vaft has after its header. The real file is about 216 KB;
/// a download cut short could still carry a whole header, and a syntax error
/// in vaft would take uBlock's Twitch scriptlets, injected in the same guarded
/// block, down with it. Well under the real size, well over a stub.
const VAFT_MIN_BODY: usize = 10_000;

/// `script` is vaft: a userscript header naming it, ended, then enough code.
fn is_vaft(script: &str) -> bool {
    const END: &str = "// ==/UserScript==";
    let script = script.trim_start_matches('\u{feff}');
    let Some(end) = script.find(END) else {
        return false;
    };
    let (header, body) = (&script[..end], &script[end + END.len()..]);
    let mut lines = header.lines().map(str::trim);
    lines.next() == Some("// ==UserScript==")
        && body.len() > VAFT_MIN_BODY
        && lines
            .filter_map(|l| l.strip_prefix("// @name"))
            // The key is `@name` itself, not `@namespace`.
            .filter(|rest| rest.starts_with(char::is_whitespace))
            .any(|name| name.contains("vaft"))
}

/// `url` is on `site` (`twitch.tv`) or one of its subdomains.
fn on_site(url: &str, site: &str) -> bool {
    WebAddress::parse(url)
        .ok()
        .and_then(|a| a.host().map(str::to_ascii_lowercase))
        // `www.twitch.tv.` is the same host.
        .map(|h| h.trim_end_matches('.').to_owned())
        .is_some_and(|h| {
            h.strip_suffix(site)
                .is_some_and(|sub| sub.is_empty() || sub.ends_with('.'))
        })
}

/// `url` is on Twitch.
fn on_twitch(url: &str) -> bool {
    on_site(url, "twitch.tv")
}

/// `url` is on Netflix.
fn on_netflix(url: &str) -> bool {
    on_site(url, "netflix.com")
}

/// adblock's name for a request type; `None` for the page itself, never
/// blocked.
fn request_type(kind: Option<ResourceType>) -> Option<&'static str> {
    Some(match kind {
        Some(ResourceType::MainFrame) => return None,
        Some(ResourceType::SubFrame) => "sub_frame",
        Some(ResourceType::Stylesheet) => "stylesheet",
        Some(ResourceType::Script) => "script",
        Some(ResourceType::Image) => "image",
        Some(ResourceType::Font) => "font",
        Some(ResourceType::Object) => "object",
        Some(ResourceType::XmlHttpRequest) => "xmlhttprequest",
        Some(ResourceType::Ping) => "ping",
        Some(ResourceType::CspReport) => "csp_report",
        Some(ResourceType::Media) => "media",
        Some(ResourceType::WebSocket) => "websocket",
        Some(ResourceType::WebTransport | ResourceType::WebBundle | ResourceType::Other) | None => {
            "other"
        }
    })
}

impl Blocker {
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

    /// Gives the engine the scriptlets' code ([`RESOURCES`]); false when
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

    /// Runs vaft ([`TWITCH`]) on Twitch; false when `script` is not vaft.
    pub fn use_twitch(&mut self, script: &str) -> bool {
        let vaft = is_vaft(script);
        if vaft {
            self.twitch = Some(script.to_owned());
        } else {
            log::warn!("blocker: the Twitch script is not vaft");
        }
        vaft
    }

    fn new(engine: Engine, updated: SystemTime) -> Self {
        let generic = engine.url_cosmetic_resources(NO_SITE).hide_selectors;
        Self {
            engine,
            generic,
            updated,
            twitch: None,
        }
    }

    /// When the lists were downloaded.
    pub fn updated(&self) -> SystemTime {
        self.updated
    }

    /// Whether the lists block `url`, loaded by the page `page_url` as
    /// `kind`, `@@` exceptions included. The page itself never is.
    pub fn should_block(&self, url: &str, page_url: &str, kind: Option<ResourceType>) -> bool {
        let Some(kind) = request_type(kind) else {
            return false;
        };
        Request::new(url, page_url, kind, "GET")
            .is_ok_and(|r| self.engine.check_network_request(&r).should_block())
    }

    /// The script the lists' `+js(...)` rules inject into `url`'s pages
    /// (their exceptions included), then vaft on Twitch, our script on
    /// Netflix; empty when none.
    pub fn scriptlets(&self, url: &str) -> String {
        let mut script = self.engine.url_cosmetic_resources(url).injected_script;
        if !script.is_empty() {
            // uBlock's scriptlets read the globals its own injection declares
            // (`safe-self.fn`, `prevent-xhr.fn`); adblock-rust's does not.
            script = format!("const scriptletGlobals = {{}};\n{script}\n");
        }
        if let Some(vaft) = self.twitch.as_deref().filter(|_| on_twitch(url)) {
            script.push_str(vaft);
        }
        if on_netflix(url) {
            script.push_str(NETFLIX);
        }
        script
    }

    /// The style sheet hiding the ad slots the lists name for `url`'s site,
    /// empty when none.
    pub fn cosmetic_css(&self, url: &str) -> String {
        let resources = self.engine.url_cosmetic_resources(url);
        let mut selectors: Vec<&str> = resources
            .hide_selectors
            .iter()
            .filter(|s| !self.generic.contains(*s))
            .map(String::as_str)
            .collect();
        selectors.sort_unstable();
        selectors
            .chunks(SELECTORS_PER_RULE)
            .map(|chunk| format!("{}{{display:none!important}}", chunk.join(",")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The compiled engine, to start without parsing the lists again.
    pub fn serialize(&self) -> Vec<u8> {
        self.engine.serialize()
    }

    /// An engine from [`Self::serialize`]; `None` when unreadable (another
    /// version of adblock-rust, corrupt file).
    pub fn deserialize(bytes: &[u8]) -> Option<Self> {
        let mut engine = Engine::default();
        engine.deserialize(bytes).ok()?;
        Some(Self::new(engine, UNIX_EPOCH))
    }
}

/// Where the blocker applies, from the settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockerPolicy {
    pub enabled: bool,
    /// Hosts where it is off (each covers its subdomains).
    pub allowlist: Vec<String>,
}

impl Default for BlockerPolicy {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl BlockerPolicy {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            enabled: settings.blocker,
            allowlist: settings.blocker_allowlist.clone(),
        }
    }

    /// Whether the blocker works on the page at `page_url`.
    pub fn applies_to(&self, page_url: &str) -> bool {
        self.enabled
            && !WebAddress::parse(page_url)
                .ok()
                .and_then(|a| a.host().map(|h| allowlisted(&self.allowlist, h)))
                .unwrap_or(false)
    }
}

/// The current engine (none until the lists arrive) and policy, shared by
/// the hooks; the engine is replaced at once when the lists are updated.
#[derive(Clone, Default)]
pub struct BlockerHandle {
    engine: Arc<RwLock<Option<Arc<Blocker>>>>,
    /// Counts the engines set, to notice a new one.
    generation: Arc<AtomicU64>,
    policy: Arc<RwLock<BlockerPolicy>>,
}

impl BlockerHandle {
    pub fn get(&self) -> Option<Arc<Blocker>> {
        self.engine
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set(&self, blocker: Blocker) {
        *self.engine.write().unwrap_or_else(|p| p.into_inner()) = Some(Arc::new(blocker));
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Changes each time an engine is set.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub fn policy(&self) -> BlockerPolicy {
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    pub fn set_policy(&self, policy: BlockerPolicy) {
        *self.policy.write().unwrap_or_else(|p| p.into_inner()) = policy;
    }

    /// Whether `url`, loaded by `page_url` as `kind`, is blocked: the policy
    /// applies to the page and the lists block it.
    pub fn should_block(&self, url: &str, page_url: &str, kind: Option<ResourceType>) -> bool {
        let Some(blocker) = self.get() else {
            return false;
        };
        self.policy
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .applies_to(page_url)
            && blocker.should_block(url, page_url, kind)
    }

    /// The style sheet hiding `url`'s ad slots; empty when the policy does
    /// not apply or no list is loaded.
    pub fn cosmetic_css(&self, url: &str) -> String {
        match self.get() {
            Some(blocker) if self.policy().applies_to(url) => blocker.cosmetic_css(url),
            _ => String::new(),
        }
    }

    /// The scriptlets for `url`'s pages; empty when the policy does not
    /// apply or no list is loaded.
    pub fn scriptlets(&self, url: &str) -> String {
        match self.get() {
            Some(blocker) if self.policy().applies_to(url) => blocker.scriptlets(url),
            _ => String::new(),
        }
    }
}

/// Notices when the engine or the policy of a handle changed (WebKit's
/// content rules are then made again).
#[derive(Default)]
pub struct BlockerChanges {
    seen: Option<(u64, BlockerPolicy)>,
}

impl BlockerChanges {
    /// Whether `handle` changed since the last call (true the first time).
    pub fn changed(&mut self, handle: &BlockerHandle) -> bool {
        let now = (handle.generation(), handle.policy());
        if self.seen.as_ref() == Some(&now) {
            return false;
        }
        self.seen = Some(now);
        true
    }
}

/// The lists' network rules as a WebKit content rule list (JSON), then, per
/// allowlisted host, a rule cancelling them on its pages (and its
/// subdomains'). Cosmetic filters are left to the page script.
pub fn webkit_rules(lists: &[&str], allowlist: &[String]) -> String {
    let mut set = FilterSet::new(true);
    let options = ParseOptions {
        rule_types: RuleTypes::NetworkOnly,
        ..ParseOptions::default()
    };
    for list in lists {
        set.add_filter_list(list.to_string(), options);
    }
    let hosts: Vec<String> = allowlist
        .iter()
        .map(|h| {
            let h = h.trim().trim_end_matches('.').to_ascii_lowercase();
            h.strip_prefix("www.").map(str::to_owned).unwrap_or(h)
        })
        .filter(|h| !h.is_empty() && h.is_ascii())
        .collect();
    let mut rules: Vec<Value> = set
        .into_content_blocking()
        .map(|(rules, _)| rules)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| serde_json::to_value(r).ok())
        .collect();
    let room = MAX_RULES.saturating_sub(hosts.len());
    if rules.len() > room {
        log::warn!(
            "blocker: {} WebKit rules, only the first {room} are kept",
            rules.len()
        );
        rules.truncate(room);
    }
    for host in hosts {
        rules.push(json!({
            "trigger": {"url-filter": ".*", "if-domain": [format!("*{host}")]},
            "action": {"type": "ignore-previous-rules"},
        }));
    }
    Value::Array(rules).to_string()
}

/// WebKit's rules for the lists cached in `dir` under `policy`; `None` when
/// the blocker is off or no list is cached yet.
pub fn webkit_rules_in(dir: &Path, policy: &BlockerPolicy) -> Option<String> {
    if !policy.enabled {
        return None;
    }
    let lists = cached_lists(dir);
    if lists.is_empty() {
        return None;
    }
    let json = webkit_rules(
        &lists.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
        &policy.allowlist,
    );
    Some(json)
}

/// The lists cached in `dir`, with whether their scriptlets are trusted.
fn cached_lists(dir: &Path) -> Vec<(bool, String)> {
    LISTS
        .iter()
        .filter_map(|(name, _, trusted)| {
            let text = fs::read_to_string(dir.join(name)).ok()?;
            Some((*trusted, text))
        })
        .collect()
}

/// The cache files the app expects: the lists, the resources and vaft.
fn known_files() -> Vec<String> {
    LISTS
        .iter()
        .map(|(name, ..)| *name)
        .chain([RESOURCES.0, TWITCH.0])
        .map(str::to_owned)
        .collect()
}

/// `state.json`: when the lists were last checked and last downloaded (unix
/// seconds), and the files the app knew then.
#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    #[serde(default)]
    last_check: Option<u64>,
    #[serde(default)]
    updated: Option<u64>,
    #[serde(default)]
    lists: Vec<String>,
}

fn unix(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The lists' cache folder: the raw lists, the compiled engine, the state.
pub struct BlockerLists {
    dir: PathBuf,
    download: Download,
}

impl BlockerLists {
    pub fn new(dir: impl Into<PathBuf>, download: Download) -> Self {
        Self {
            dir: dir.into(),
            download,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn state(&self) -> State {
        fs::read_to_string(self.dir.join(STATE_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    fn save_state(&self, state: &State) {
        let path = self.dir.join(STATE_FILE);
        let text = serde_json::to_string(state).expect("plain struct");
        if let Err(e) = files::write(&path, &text) {
            log::warn!("blocker: {e}");
        }
    }

    fn save_engine(&self, blocker: &Blocker) {
        if let Err(e) = files::write_bytes(&self.dir.join(ENGINE_FILE), &blocker.serialize()) {
            log::warn!("blocker: {e}");
        }
    }

    /// The engine from the cached lists, with the cached resources and vaft.
    fn build(&self) -> Option<Blocker> {
        let lists = cached_lists(&self.dir);
        let mut blocker = Blocker::compile(
            &lists
                .iter()
                .map(|(t, l)| (*t, l.as_str()))
                .collect::<Vec<_>>(),
        )?;
        self.load_resources(&mut blocker);
        Some(blocker)
    }

    /// Hands `blocker` the cached scriptlets' code and vaft, when there are
    /// some (`engine.dat` keeps neither).
    fn load_resources(&self, blocker: &mut Blocker) {
        if let Ok(json) = fs::read_to_string(self.dir.join(RESOURCES.0)) {
            blocker.use_resources(&json);
        }
        if let Ok(vaft) = fs::read_to_string(self.dir.join(TWITCH.0)) {
            blocker.use_twitch(&vaft);
        }
    }

    /// The cached engine (`engine.dat`), or one rebuilt from the cached
    /// lists when it is unreadable; `None` when nothing is cached.
    pub fn load_cached(&self) -> Option<Blocker> {
        self.cached(true)
    }

    /// [`Self::load_cached`] that writes nothing (a private window).
    pub fn read_cached(&self) -> Option<Blocker> {
        self.cached(false)
    }

    fn cached(&self, save: bool) -> Option<Blocker> {
        let engine_path = self.dir.join(ENGINE_FILE);
        let mut blocker = match fs::read(&engine_path)
            .ok()
            .and_then(|b| Blocker::deserialize(&b))
        {
            Some(mut blocker) => {
                self.load_resources(&mut blocker);
                blocker
            }
            None => {
                let blocker = self.build()?;
                if save {
                    self.save_engine(&blocker);
                }
                blocker
            }
        };
        blocker.updated = match self.state().updated {
            Some(secs) => UNIX_EPOCH + Duration::from_secs(secs),
            None => fs::metadata(&engine_path)
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH),
        };
        Some(blocker)
    }

    /// Downloads the lists and rebuilds the engine. A list that fails to
    /// download or has no rule keeps its cached copy; an error when none
    /// could be downloaded.
    pub fn refresh(&self) -> Result<Blocker, String> {
        let mut fetched = 0;
        for (name, url, _) in LISTS {
            let text = (self.download)(url)
                .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()));
            match text {
                Ok(text) if has_rules(&text) => {
                    files::write(&self.dir.join(name), &text).map_err(|e| e.to_string())?;
                    fetched += 1;
                }
                Ok(_) => log::warn!("blocker: {url} has no rule, the cached copy is kept"),
                Err(e) => log::warn!("blocker: {url}: {e}"),
            }
        }
        let (name, url) = RESOURCES;
        match (self.download)(url)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
        {
            Ok(json)
                if serde_json::from_str::<Vec<Resource>>(&json).is_ok_and(|r| !r.is_empty()) =>
            {
                if let Err(e) = files::write(&self.dir.join(name), &json) {
                    log::warn!("blocker: {e}");
                }
            }
            Ok(_) => log::warn!("blocker: {url} is unreadable, the cached copy is kept"),
            Err(e) => log::warn!("blocker: {url}: {e}"),
        }
        let (name, url) = TWITCH;
        match (self.download)(url)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
        {
            Ok(script) if is_vaft(&script) => {
                if let Err(e) = files::write(&self.dir.join(name), &script) {
                    log::warn!("blocker: {e}");
                }
            }
            Ok(_) => log::warn!("blocker: {url} is not vaft, the cached copy is kept"),
            Err(e) => log::warn!("blocker: {url}: {e}"),
        }
        // Brave's resources, cached under this name before uBlock's.
        let _ = fs::remove_file(self.dir.join("resources.json"));
        if fetched == 0 {
            return Err("no list could be downloaded".into());
        }
        let now = SystemTime::now();
        let mut blocker = self.build().ok_or("no list")?;
        blocker.updated = now;
        self.save_engine(&blocker);
        self.save_state(&State {
            last_check: Some(unix(now)),
            updated: Some(unix(now)),
            lists: known_files(),
        });
        Ok(blocker)
    }

    /// The lists were never checked, not for a day, or a list is new to the
    /// app since the last check (an update added it).
    pub fn due(&self) -> bool {
        self.due_at(SystemTime::now())
    }

    fn due_at(&self, now: SystemTime) -> bool {
        let state = self.state();
        // A source that keeps failing is not retried before the day is up.
        let new = known_files().iter().any(|f| !state.lists.contains(f));
        new || state
            .last_check
            .is_none_or(|t| unix(now).saturating_sub(t) >= CHECK_EVERY.as_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "\
[Adblock Plus 2.0]
! Title: test
||ads.test^
/banner/*
@@||ads.test/allowed.js
a.test##.ad
b.test##.other-ad
##div[data-generic-ad]
";

    fn blocker() -> Blocker {
        Blocker::from_lists(&[LIST]).unwrap()
    }

    const PAGE: &str = "https://site.test/";

    #[test]
    fn ads_are_blocked_and_the_rest_allowed() {
        let b = blocker();
        assert!(b.should_block("https://ads.test/x.js", PAGE, Some(ResourceType::Script)));
        assert!(b.should_block(
            "https://cdn.test/banner/1.png",
            PAGE,
            Some(ResourceType::Image)
        ));
        assert!(!b.should_block("https://cdn.test/app.js", PAGE, Some(ResourceType::Script)));
    }

    #[test]
    fn exceptions_are_honoured() {
        assert!(!blocker().should_block(
            "https://ads.test/allowed.js",
            PAGE,
            Some(ResourceType::Script)
        ));
    }

    #[test]
    fn the_page_itself_is_never_blocked() {
        assert!(!blocker().should_block(
            "https://ads.test/",
            "https://ads.test/",
            Some(ResourceType::MainFrame)
        ));
    }

    #[test]
    fn an_empty_list_gives_no_blocker() {
        assert!(Blocker::from_lists(&["! only a comment\n", ""]).is_none());
    }

    #[test]
    fn cosmetic_css_is_the_sites_own() {
        let b = blocker();
        let css = b.cosmetic_css("https://www.a.test/page");
        assert_eq!(css, ".ad{display:none!important}");
        assert!(!css.contains("other-ad"));
        assert!(!css.contains("generic"));
        assert_eq!(b.cosmetic_css("https://c.test/"), "");
    }

    #[test]
    fn cosmetic_css_is_split_into_rules() {
        let list: String = (0..SELECTORS_PER_RULE + 1)
            .map(|i| format!("a.test##.ad{i}\n"))
            .collect();
        let css = Blocker::from_lists(&[&list])
            .unwrap()
            .cosmetic_css("https://a.test/");
        assert_eq!(css.lines().count(), 2);
    }

    #[test]
    fn a_deserialized_engine_decides_the_same() {
        let b = Blocker::deserialize(&blocker().serialize()).unwrap();
        assert!(b.should_block("https://ads.test/x.js", PAGE, Some(ResourceType::Script)));
        assert!(!b.should_block(
            "https://ads.test/allowed.js",
            PAGE,
            Some(ResourceType::Script)
        ));
        assert_eq!(
            b.cosmetic_css("https://a.test/"),
            ".ad{display:none!important}"
        );
        assert!(Blocker::deserialize(b"not an engine").is_none());
    }

    #[test]
    fn the_handle_follows_the_policy() {
        let handle = BlockerHandle::default();
        let ad = "https://ads.test/x.js";
        assert!(!handle.should_block(ad, PAGE, Some(ResourceType::Script)));
        handle.set(blocker());
        assert!(handle.should_block(ad, PAGE, Some(ResourceType::Script)));
        assert!(!handle.cosmetic_css("https://a.test/").is_empty());

        handle.set_policy(BlockerPolicy {
            enabled: true,
            allowlist: vec!["site.test".into(), "a.test".into()],
        });
        assert!(!handle.should_block(ad, "https://www.site.test/", Some(ResourceType::Script)));
        assert!(handle.cosmetic_css("https://a.test/").is_empty());

        handle.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        assert!(!handle.should_block(ad, PAGE, Some(ResourceType::Script)));
    }

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

    #[test]
    fn refresh_downloads_and_caches_the_lists() {
        let dir = tempfile::tempdir().unwrap();
        // Everything is served, so nothing is missing afterwards.
        let mut answers: Vec<(&str, &'static str)> =
            LISTS.iter().map(|(_, url, _)| (*url, LIST)).collect();
        answers.push((RESOURCES.1, RESOURCES_JSON));
        let lists = BlockerLists::new(dir.path(), fake(&answers));
        assert!(lists.load_cached().is_none());
        assert!(lists.due());
        let b = lists.refresh().unwrap();
        assert!(b.should_block("https://ads.test/x.js", PAGE, Some(ResourceType::Script)));
        assert!(!lists.due());

        let cached = lists.load_cached().unwrap();
        assert!(cached.should_block("https://ads.test/x.js", PAGE, Some(ResourceType::Script)));
        assert_eq!(unix(cached.updated()), unix(b.updated()));
    }

    #[test]
    fn a_failed_or_empty_download_keeps_the_cached_list() {
        let dir = tempfile::tempdir().unwrap();
        BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST)]))
            .refresh()
            .unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, "! no rule\n")]));
        assert!(lists.refresh().is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("easylist.txt")).unwrap(),
            LIST
        );
        let b = lists.load_cached().unwrap();
        assert!(b.should_block("https://ads.test/x.js", PAGE, Some(ResourceType::Script)));
    }

    #[test]
    fn the_engine_is_rebuilt_from_the_lists_when_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST)]));
        lists.refresh().unwrap();
        fs::write(dir.path().join(ENGINE_FILE), b"garbage").unwrap();
        let b = lists.read_cached().unwrap();
        assert!(b.should_block("https://ads.test/x.js", PAGE, Some(ResourceType::Script)));
        assert_eq!(fs::read(dir.path().join(ENGINE_FILE)).unwrap(), b"garbage");
        let b = lists.load_cached().unwrap();
        assert!(b.should_block("https://ads.test/x.js", PAGE, Some(ResourceType::Script)));
        assert_ne!(fs::read(dir.path().join(ENGINE_FILE)).unwrap(), b"garbage");
    }

    #[test]
    fn network_rules_become_webkit_blocks() {
        let json: Value = serde_json::from_str(&webkit_rules(&[LIST], &[])).unwrap();
        let rules = json.as_array().unwrap();
        assert!(rules.iter().any(|r| r["action"]["type"] == "block"
            && r["trigger"]["url-filter"].as_str().unwrap().contains("ads")));
        // Cosmetic filters are left to the page script.
        assert!(
            rules
                .iter()
                .all(|r| r["action"]["type"] != "css-display-none")
        );
    }

    #[test]
    fn allowlisted_hosts_cancel_the_rules_last() {
        let allowlist = vec!["WWW.LeMonde.fr".to_string(), "".to_string()];
        let json: Value = serde_json::from_str(&webkit_rules(&[LIST], &allowlist)).unwrap();
        let last = json.as_array().unwrap().last().unwrap().clone();
        assert_eq!(
            last,
            json!({
                "trigger": {"url-filter": ".*", "if-domain": ["*lemonde.fr"]},
                "action": {"type": "ignore-previous-rules"},
            })
        );
    }

    #[test]
    fn webkit_rules_follow_the_policy_and_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let on = BlockerPolicy::default();
        assert!(webkit_rules_in(dir.path(), &on).is_none());
        fs::write(dir.path().join("easylist.txt"), LIST).unwrap();
        assert!(webkit_rules_in(dir.path(), &on).is_some());
        let off = BlockerPolicy {
            enabled: false,
            ..on
        };
        assert!(webkit_rules_in(dir.path(), &off).is_none());
    }

    #[test]
    fn changes_are_noticed_once() {
        let handle = BlockerHandle::default();
        let mut changes = BlockerChanges::default();
        assert!(changes.changed(&handle));
        assert!(!changes.changed(&handle));
        handle.set(blocker());
        assert!(changes.changed(&handle));
        assert!(!changes.changed(&handle));
        handle.set_policy(BlockerPolicy {
            enabled: true,
            allowlist: vec!["a.test".into()],
        });
        assert!(changes.changed(&handle));
        assert!(!changes.changed(&handle));
    }

    /// A scriptlet: sets `window.said` to its first argument.
    const RESOURCES_JSON: &str = r#"[{"name":"say.js","aliases":[],"kind":{"mime":"application/javascript"},"content":"d2luZG93LnNhaWQgPSAie3sxfX0iOw=="}]"#;

    const SCRIPTLET_LIST: &str = "site.test##+js(say, hello)
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
    fn scriptlets_find_the_globals_ublock_declares() {
        // Reads `scriptletGlobals`, as uBlock's `safe-self.fn` does.
        let json = r#"[{"name":"glob.js","aliases":[],"kind":{"mime":"application/javascript"},"content":"d2luZG93LnNlY3JldCA9IHNjcmlwdGxldEdsb2JhbHMuYmNTZWNyZXQ7"}]"#;
        let mut b = Blocker::from_lists(&["site.test##+js(glob)\n"]).unwrap();
        assert!(b.use_resources(json));
        let script = b.scriptlets("https://site.test/");
        let declared = script.find("const scriptletGlobals = {};").expect(&script);
        let used = script.find("scriptletGlobals.bcSecret").expect(&script);
        assert!(declared < used, "{script}");
        assert_eq!(b.scriptlets("https://other.test/"), "");
    }

    #[test]
    fn without_resources_there_is_no_scriptlet() {
        let b = Blocker::from_lists(&[SCRIPTLET_LIST]).unwrap();
        assert_eq!(b.scriptlets("https://site.test/"), "");
        assert!(
            !Blocker::from_lists(&[SCRIPTLET_LIST])
                .unwrap()
                .use_resources("[]")
        );
        assert!(
            !Blocker::from_lists(&[SCRIPTLET_LIST])
                .unwrap()
                .use_resources("nope")
        );
    }

    #[test]
    fn a_cached_engine_gets_its_resources_back() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(
            dir.path(),
            fake(&[(LISTS[2].1, SCRIPTLET_LIST), (RESOURCES.1, RESOURCES_JSON)]),
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
    fn the_former_resources_file_goes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("resources.json"), "[]").unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST)]));
        lists.refresh().unwrap();
        assert!(!dir.path().join("resources.json").exists());
        // Nothing to remove: no harm.
        lists.refresh().unwrap();
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
    fn a_list_unknown_to_the_last_refresh_makes_the_lists_due() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[]));
        let now = SystemTime::now();
        // Checked a minute ago by an app that did not know all the lists
        // (an update added some, or an old state.json): due again.
        lists.save_state(&State {
            last_check: Some(unix(now)),
            lists: vec!["easylist.txt".into()],
            ..State::default()
        });
        assert!(lists.due_at(now + Duration::from_secs(60)));
        lists.save_state(&State {
            last_check: Some(unix(now)),
            ..State::default()
        });
        assert!(lists.due_at(now + Duration::from_secs(60)));
        lists.save_state(&State {
            last_check: Some(unix(now)),
            lists: known_files(),
            ..State::default()
        });
        assert!(!lists.due_at(now + Duration::from_secs(60)));
    }

    #[test]
    fn a_source_that_keeps_failing_does_not_make_the_lists_due() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST)]));
        lists.refresh().unwrap();
        assert!(!dir.path().join("ublock-filters.txt").exists());
        assert!(!lists.due_at(SystemTime::now() + Duration::from_secs(60)));
    }

    /// A stand-in for vaft: its header, then code.
    const VAFT_HEADER: &str = "// ==UserScript==
// @name         TwitchAdSolutions (vaft)
// @match        *://*.twitch.tv/*
// ==/UserScript==
";

    /// A stand-in as long as the real one is not, but past the minimum.
    static VAFT: std::sync::LazyLock<&'static str> = std::sync::LazyLock::new(|| {
        let body = format!(
            "(function() {{ window.vaft = true; }})();\n{}",
            "// padding\n".repeat(1_000)
        );
        Box::leak(format!("{VAFT_HEADER}{body}").into_boxed_str())
    });

    fn with_vaft() -> Blocker {
        let mut b = blocker();
        assert!(b.use_twitch(*VAFT));
        b
    }

    #[test]
    fn vaft_comes_on_twitch_only() {
        let b = with_vaft();
        for url in [
            "https://www.twitch.tv/x",
            "https://player.twitch.tv/?channel=x",
            "https://twitch.tv/",
        ] {
            assert!(b.scriptlets(url).contains("window.vaft"), "{url}");
        }
        for url in [
            "https://nottwitch.tv/",
            "https://twitch.tv.evil.test/",
            "https://site.test/",
        ] {
            assert_eq!(b.scriptlets(url), "", "{url}");
        }
        // The prelude is uBlock's scriptlets' only.
        assert!(
            !b.scriptlets("https://www.twitch.tv/x")
                .contains("scriptletGlobals")
        );
    }

    #[test]
    fn vaft_follows_ublocks_scriptlets() {
        let mut b = Blocker::from_lists(&["twitch.tv##+js(say, hello)\n"]).unwrap();
        assert!(b.use_resources(RESOURCES_JSON));
        assert!(b.use_twitch(*VAFT));
        let script = b.scriptlets("https://www.twitch.tv/x");
        let prelude = script.find("const scriptletGlobals").expect(&script);
        let ublock = script.find("window.said").expect(&script);
        let vaft = script.find("window.vaft").expect(&script);
        assert!(prelude < ublock && ublock < vaft, "{script}");
    }

    #[test]
    fn only_vaft_is_used() {
        let mut b = blocker();
        assert!(!b.use_twitch("<html>404</html>"));
        assert!(!b.use_twitch("// ==UserScript==\n// @name Other\n// ==/UserScript==\n"));
        assert!(!b.use_twitch("(function() {})();\n// @name vaft\n"));
        assert_eq!(b.scriptlets("https://www.twitch.tv/x"), "");
    }

    #[test]
    fn a_truncated_vaft_is_refused() {
        let mut b = blocker();
        // A valid header with a stub body, or a header that never ends.
        assert!(!b.use_twitch(&format!(
            "{VAFT_HEADER}(function() {{ window.vaft = true; }})();"
        )));
        assert!(!b.use_twitch("// ==UserScript==\n// @name vaft\n(function() {"));
        assert_eq!(b.scriptlets("https://www.twitch.tv/x"), "");
        assert!(b.use_twitch(*VAFT));
    }

    #[test]
    fn vaft_is_named_exactly() {
        let mut b = blocker();
        let body = "// padding\n".repeat(1_000);
        let other = format!(
            "// ==UserScript==\n// @name Other\n// @namespace https://github.com/vaft\n// ==/UserScript==\n{body}"
        );
        assert!(!b.use_twitch(&other));
    }

    #[test]
    fn a_trailing_dot_host_is_twitch() {
        assert!(on_twitch("https://www.twitch.tv./x"));
        assert!(!on_twitch("https://nottwitch.tv./"));
    }

    #[test]
    fn the_netflix_script_comes_on_netflix_only() {
        let b = blocker();
        for url in [
            "https://www.netflix.com/watch/1",
            "https://netflix.com/",
            "https://www.netflix.com./browse",
        ] {
            assert!(b.scriptlets(url).contains("__buNetflixAds"), "{url}");
        }
        for url in [
            "https://notnetflix.com/",
            "https://netflix.com.evil.test/",
            "https://www.twitch.tv/x",
        ] {
            assert_eq!(b.scriptlets(url), "", "{url}");
        }
    }

    #[test]
    fn the_netflix_script_follows_the_policy() {
        let handle = BlockerHandle::default();
        handle.set(blocker());
        assert!(!handle.scriptlets("https://www.netflix.com/watch/1").is_empty());
        handle.set_policy(BlockerPolicy {
            enabled: true,
            allowlist: vec!["netflix.com".into()],
        });
        assert_eq!(handle.scriptlets("https://www.netflix.com/watch/1"), "");
    }

    #[test]
    fn the_handle_gives_vaft_under_the_policy() {
        let handle = BlockerHandle::default();
        handle.set(with_vaft());
        assert!(!handle.scriptlets("https://www.twitch.tv/x").is_empty());
        handle.set_policy(BlockerPolicy {
            enabled: true,
            allowlist: vec!["twitch.tv".into()],
        });
        assert_eq!(handle.scriptlets("https://www.twitch.tv/x"), "");
        handle.set_policy(BlockerPolicy {
            enabled: false,
            allowlist: Vec::new(),
        });
        assert_eq!(handle.scriptlets("https://www.twitch.tv/x"), "");
    }

    #[test]
    fn vaft_is_cached_and_survives_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST), (TWITCH.1, *VAFT)]));
        let b = lists.refresh().unwrap();
        assert!(
            b.scriptlets("https://www.twitch.tv/x")
                .contains("window.vaft")
        );
        assert_eq!(
            fs::read_to_string(dir.path().join(TWITCH.0)).unwrap(),
            *VAFT
        );
        // From engine.dat, then from the lists.
        let b = lists.load_cached().unwrap();
        assert!(
            b.scriptlets("https://www.twitch.tv/x")
                .contains("window.vaft")
        );
        fs::write(dir.path().join(ENGINE_FILE), b"garbage").unwrap();
        let b = lists.read_cached().unwrap();
        assert!(
            b.scriptlets("https://www.twitch.tv/x")
                .contains("window.vaft")
        );
    }

    #[test]
    fn without_vaft_twitch_gets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST)]));
        let b = lists.refresh().unwrap();
        assert!(!dir.path().join(TWITCH.0).exists());
        assert_eq!(b.scriptlets("https://www.twitch.tv/x"), "");
    }

    #[test]
    fn a_download_that_is_not_vaft_is_not_kept() {
        let dir = tempfile::tempdir().unwrap();
        BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST), (TWITCH.1, "<html>")]))
            .refresh()
            .unwrap();
        assert!(!dir.path().join(TWITCH.0).exists());
        // The cached copy stays.
        BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST), (TWITCH.1, *VAFT)]))
            .refresh()
            .unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[(EASYLIST, LIST), (TWITCH.1, "<html>")]));
        let b = lists.refresh().unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(TWITCH.0)).unwrap(),
            *VAFT
        );
        assert!(
            b.scriptlets("https://www.twitch.tv/x")
                .contains("window.vaft")
        );
    }

    #[test]
    fn vaft_is_a_known_file() {
        assert!(known_files().contains(&TWITCH.0.to_string()));
    }

    #[test]
    fn the_lists_are_due_after_a_day() {
        let dir = tempfile::tempdir().unwrap();
        let lists = BlockerLists::new(dir.path(), fake(&[]));
        let now = SystemTime::now();
        lists.save_state(&State {
            last_check: Some(unix(now)),
            lists: known_files(),
            ..State::default()
        });
        assert!(!lists.due_at(now + Duration::from_secs(60)));
        assert!(lists.due_at(now + CHECK_EVERY));
    }
}
