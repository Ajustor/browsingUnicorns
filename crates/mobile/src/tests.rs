use std::collections::BTreeMap;
use std::sync::Mutex;

use data_encoding::BASE64;

use super::*;

/// Records what Rust asks of the WebViews; the page scripts, the messages
/// to pages and the background pages apart.
#[derive(Default)]
struct FakeHost(
    Mutex<Vec<String>>,
    Mutex<Vec<String>>,
    Mutex<Vec<(PageTarget, String)>>,
    Mutex<Vec<BackgroundPage>>,
);

impl WebViewHost for FakeHost {
    fn open_view(&self, tab: u64, url: String, private: bool) {
        let kind = if private { "private " } else { "" };
        self.0
            .lock()
            .unwrap()
            .push(format!("open {kind}{tab} {url}"));
    }
    fn close_view(&self, tab: u64) {
        self.0.lock().unwrap().push(format!("close {tab}"));
    }
    fn show(&self, tab: Option<u64>) {
        self.0.lock().unwrap().push(format!("show {tab:?}"));
    }
    fn navigate(&self, tab: u64, url: String) {
        self.0.lock().unwrap().push(format!("navigate {tab} {url}"));
    }
    fn go_back(&self, tab: u64) {
        self.0.lock().unwrap().push(format!("back {tab}"));
    }
    fn go_forward(&self, tab: u64) {
        self.0.lock().unwrap().push(format!("forward {tab}"));
    }
    fn reload(&self, tab: u64) {
        self.0.lock().unwrap().push(format!("reload {tab}"));
    }
    fn set_page_scripts(&self, scripts: Vec<String>) {
        *self.1.lock().unwrap() = scripts;
    }
    fn post_to_page(&self, target: PageTarget, script: String) {
        self.2.lock().unwrap().push((target, script));
    }
    fn set_backgrounds(&self, pages: Vec<BackgroundPage>) {
        *self.3.lock().unwrap() = pages;
    }
    fn show_panel(&self, url: Option<String>) {
        self.0.lock().unwrap().push(format!("panel {url:?}"));
    }
    fn cookies(&self) -> Option<Vec<CookieInfo>> {
        None
    }
    fn set_cookies(&self, cookies: Vec<CookieInfo>) {
        self.0
            .lock()
            .unwrap()
            .push(format!("cookies {}", cookies.len()));
    }
}

impl FakeHost {
    /// The last message posted to a page, without the hook call around it.
    fn last_reply(&self) -> String {
        let (_, script) = self.2.lock().unwrap().last().cloned().unwrap();
        script
            .trim_start_matches("window.__buReply && window.__buReply(")
            .trim_end_matches(')')
            .to_owned()
    }
}

/// The relay's HTTP API in memory: blobs by URL.
#[derive(Default)]
struct FakeHttp(Mutex<BTreeMap<String, Vec<u8>>>);

impl SyncHttp for FakeHttp {
    fn put(&self, url: String, _token: String, body: Vec<u8>) -> Result<(), MobileError> {
        self.0.lock().unwrap().insert(url, body);
        Ok(())
    }
    fn get(&self, url: String, _token: String) -> Result<String, MobileError> {
        let prefix = format!("{url}/devices/");
        let devices: Vec<String> = self
            .0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(k, v)| {
                let id = k.strip_prefix(&prefix)?;
                Some(format!(r#"{{"id":"{id}","data":"{}"}}"#, BASE64.encode(v)))
            })
            .collect();
        Ok(format!(r#"{{"devices":[{}]}}"#, devices.join(",")))
    }
    fn delete(&self, url: String, _token: String) -> Result<(), MobileError> {
        self.0.lock().unwrap().remove(&url);
        Ok(())
    }
    fn download(&self, url: String) -> Result<Vec<u8>, MobileError> {
        self.0
            .lock()
            .unwrap()
            .get(&url)
            .cloned()
            .ok_or_else(|| MobileError::new("404"))
    }
}

fn browser(
    dir: &tempfile::TempDir,
    name: &str,
    http: &Arc<FakeHttp>,
) -> (Arc<MobileBrowser>, Arc<FakeHost>) {
    let host = Arc::new(FakeHost::default());
    let b = MobileBrowser::new(
        dir.path().join(name).to_string_lossy().into_owned(),
        name.into(),
        Some("https://relay.test".into()),
        host.clone(),
        http.clone(),
    );
    (b, host)
}

#[test]
fn tabs_and_navigation_go_through_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, host) = browser(&dir, "phone", &http);
    b.restore_session().unwrap();
    let state = b.state();
    assert_eq!(state.tabs.len(), 1);
    let id = state.active.unwrap();
    assert_eq!(
        host.0.lock().unwrap().clone(),
        vec![
            format!("open {id} https://duckduckgo.com/"),
            format!("show Some({id})")
        ]
    );

    b.on_load_started(id, "https://a.com/".into());
    assert!(b.state().tabs[0].loading);
    b.on_load_finished(id, "https://a.com/".into());
    b.on_title_changed(id, "A".into());
    let state = b.state();
    assert_eq!(state.active_url.as_deref(), Some("https://a.com/"));
    assert!(state.active_secure);
    assert_eq!(state.tabs[0].title, "A");
    assert_eq!(b.history(String::new(), 10).len(), 1);

    b.submit("rust-lang.org".into(), false).unwrap();
    assert!(
        host.0
            .lock()
            .unwrap()
            .contains(&format!("navigate {id} https://rust-lang.org/"))
    );
    b.on_load_started(id, "data:text/html,hi".into());
    assert!(b.open_tab(Some("javascript:x".into()), false).is_err());
}

#[test]
fn tabs_move_and_the_address_bar_suggests() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, _host) = browser(&dir, "phone", &http);
    b.restore_session().unwrap();
    let first = b.state().active.unwrap();
    b.on_load_finished(first, "https://duckduckgo.com/?q=rust+book".into());
    b.on_load_finished(first, "https://doc.rust-lang.org/book/".into());
    b.on_title_changed(first, "The Rust Book".into());
    let second = b.open_tab(None, false).unwrap();
    b.move_tab(second, 0);
    let order: Vec<u64> = b.state().tabs.iter().map(|t| t.id).collect();
    assert_eq!(order, vec![second, first]);

    let s = b.suggest("rust".into(), 8);
    assert_eq!(s.len(), 2);
    assert!(s[0].search && s[0].text == "rust book");
    assert_eq!(s[0].url, "https://duckduckgo.com/?q=rust+book");
    assert!(!s[1].search && s[1].text == "The Rust Book");
}

#[test]
fn private_tabs_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, host) = browser(&dir, "phone", &http);
    b.restore_session().unwrap();
    let normal = b.state().active.unwrap();
    b.on_load_finished(normal, "https://a.com/".into());
    let private = b
        .open_tab(Some("https://secret.test/".into()), true)
        .unwrap();
    assert!(
        host.0
            .lock()
            .unwrap()
            .contains(&format!("open private {private} https://secret.test/"))
    );
    b.on_load_started(private, "https://secret.test/".into());
    b.on_load_finished(private, "https://secret.test/".into());
    b.on_title_changed(private, "Secret".into());
    // A page of the private tab opens a private tab too.
    b.on_new_window(private, "https://secret.test/popup".into())
        .unwrap();
    let state = b.state();
    assert_eq!(state.tabs.iter().filter(|t| t.private).count(), 2);
    assert_eq!(state.active, Some(private), "the link opens behind");
    assert!(!state.tabs.iter().find(|t| t.id == normal).unwrap().private);

    let history = b.history(String::new(), 10);
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].url, "https://a.com/");

    b.persist().unwrap();
    let session = std::fs::read_to_string(dir.path().join("phone").join("session.json")).unwrap();
    assert!(session.contains("a.com") && !session.contains("secret"));

    let record = b.lock().browser.sync_record(new_device_id(), "phone");
    assert_eq!(record.tabs.len(), 1);
    assert_eq!(record.tabs[0].address.as_str(), "https://a.com/");
    assert!(
        record
            .history
            .iter()
            .all(|v| !v.address.as_str().contains("secret"))
    );

    b.close_private_tabs();
    let state = b.state();
    assert_eq!(state.tabs.len(), 1);
    assert_eq!(state.tabs[0].id, normal);
    assert_eq!(state.active, Some(normal));
}

#[test]
fn bookmarks_and_session_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    {
        let (b, _) = browser(&dir, "phone", &http);
        b.restore_session().unwrap();
        let id = b.state().active.unwrap();
        b.on_load_finished(id, "https://a.com/".into());
        assert!(b.toggle_bookmark().unwrap());
        assert!(b.state().bookmarked);
        b.persist().unwrap();
    }
    let (b, host) = browser(&dir, "phone", &http);
    assert_eq!(b.bookmarks().len(), 1);
    b.restore_session().unwrap();
    // The WebView is asked to reopen the page (it reports the load itself).
    let id = b.state().active.unwrap();
    assert!(
        host.0
            .lock()
            .unwrap()
            .contains(&format!("open {id} https://a.com/"))
    );
    b.remove_bookmark("https://a.com/".into()).unwrap();
    assert!(b.bookmarks().is_empty());
}

#[test]
fn two_devices_pair_and_sync() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (desktop, _) = browser(&dir, "desktop", &http);
    let (phone, _) = browser(&dir, "phone", &http);
    desktop.restore_session().unwrap();
    phone.restore_session().unwrap();

    assert!(!phone.sync_info().enabled);
    phone.sync_now().unwrap(); // off: nothing happens
    desktop.sync_turn_on().unwrap();
    let code = desktop.sync_info().code.unwrap();
    assert!(phone.sync_join("not a code".into()).is_err());
    phone
        .sync_join(desktop.sync_info().qr_text.unwrap())
        .unwrap();
    assert_eq!(phone.sync_info().code.as_deref(), Some(code.as_str()));

    let id = desktop.state().active.unwrap();
    desktop.on_load_finished(id, "https://desktop.test/".into());
    desktop.toggle_bookmark().unwrap();
    assert!(desktop.has_unsynced_changes());
    desktop.sync_now().unwrap();
    phone.sync_now().unwrap();

    assert_eq!(phone.bookmarks()[0].url, "https://desktop.test/");
    let info = phone.sync_info();
    assert!(info.last_sync_at.is_some());
    assert_eq!(info.devices.len(), 1);
    assert_eq!(info.devices[0].name, "desktop");
    assert_eq!(info.devices[0].tabs[0].url, "https://desktop.test/");
    // Nothing readable on the relay.
    assert!(
        http.0
            .lock()
            .unwrap()
            .values()
            .all(|b| !String::from_utf8_lossy(b).contains("desktop.test"))
    );

    phone.sync_turn_off().unwrap();
    assert!(!phone.sync_info().enabled);
    desktop.sync_now().unwrap();
    assert!(desktop.sync_info().devices.is_empty(), "the phone left");
}

/// A package of a blocker: `||ads.test^` blocked, `/ad.js` replaced by its
/// stand-in.
fn blocker_package() -> (domain::ExtensionId, Vec<u8>) {
    use std::io::Write;

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let files = [
        (
            "manifest.json",
            r#"{"name":"Blocker","version":"1","declarative_net_request":{"rule_resources":[{"id":"r","enabled":true,"path":"rules.json"}]}}"#,
        ),
        (
            "rules.json",
            r#"[{"id":1,"action":{"type":"block"},"condition":{"urlFilter":"||ads.test^"}},
                {"id":2,"priority":2,"action":{"type":"redirect","redirect":{"extensionPath":"/war/noop.js"}},
                 "condition":{"urlFilter":"/ad.js","resourceTypes":["script"]}}]"#,
        ),
        ("war/noop.js", "(function(){})();"),
    ];
    for (name, content) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    let zip = zip.finish().unwrap().into_inner();
    signed_crx(&zip)
}

/// A CRX3 around `zip`, signed by a new P-256 key, and the id it gets.
fn signed_crx(zip: &[u8]) -> (domain::ExtensionId, Vec<u8>) {
    use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, KeyPair};
    use sha2::{Digest, Sha256};

    /// A protobuf field of bytes.
    fn field(number: u64, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for mut v in [(number << 3) | 2, data.len() as u64] {
            while v >= 0x80 {
                out.push((v as u8) | 0x80);
                v >>= 7;
            }
            out.push(v as u8);
        }
        out.extend(data);
        out
    }

    let rng = ring::rand::SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
    let pair =
        EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng).unwrap();
    // The SubjectPublicKeyInfo of a P-256 key: a fixed prefix, the point.
    let mut key = vec![
        0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08,
        0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
    ];
    key.extend(pair.public_key().as_ref());
    let hash = Sha256::digest(&key);
    let signed_data = field(1, &hash[..16]);
    let message = [
        &b"CRX3 SignedData\x00"[..],
        &(signed_data.len() as u32).to_le_bytes(),
        &signed_data,
        zip,
    ]
    .concat();
    let signature = pair.sign(&rng, &message).unwrap();
    let mut header = field(3, &[field(1, &key), field(2, signature.as_ref())].concat());
    header.extend(field(10000, &signed_data));
    let mut crx = b"Cr24".to_vec();
    crx.extend(3u32.to_le_bytes());
    crx.extend((header.len() as u32).to_le_bytes());
    crx.extend(header);
    crx.extend(zip);
    (domain::ExtensionId::from_key_hash(&hash), crx)
}

#[test]
fn extensions_block_requests() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (id, package) = blocker_package();
    http.0
        .lock()
        .unwrap()
        .insert(infrastructure::extensions::store_package_url(&id), package);
    let (b, _) = browser(&dir, "phone", &http);
    let page = Some("https://news.test/".to_owned());
    let intercept = |url: &str, dest: Option<&str>| {
        b.intercept(
            1,
            url.into(),
            page.clone(),
            false,
            "GET".into(),
            None,
            dest.map(Into::into),
        )
    };
    assert_eq!(
        intercept("https://cdn.ads.test/x.png", None),
        Interception::Allow
    );

    assert!(b.install_extension("not a link".into()).is_err());
    let name = b
        .install_extension(format!(
            "https://chromewebstore.google.com/detail/blocker/{id}"
        ))
        .unwrap();
    assert_eq!(name, "Blocker 1");
    // Its files, for the WebViews.
    let file = b
        .extension_resource(
            format!("https://{id}.bu-ext.invalid/war/noop.js"),
            None,
            None,
            None,
        )
        .unwrap();
    assert_eq!(file.mime_type, "text/javascript");
    assert_eq!(file.data, b"(function(){})();");
    assert!(
        b.extension_resource("https://example.com/".into(), None, None, None)
            .is_none()
    );
    assert_eq!(b.extensions().len(), 1);
    assert_eq!(
        intercept("https://cdn.ads.test/x.png", None),
        Interception::Block
    );
    assert_eq!(
        intercept("https://news.test/style.css", Some("style")),
        Interception::Allow
    );
    match intercept("https://cdn.example/ad.js", Some("script")) {
        Interception::Serve { path, mime_type } => {
            assert!(path.ends_with("noop.js"), "{path}");
            assert_eq!(mime_type, "application/javascript");
        }
        other => panic!("{other:?}"),
    }
    // Top-level pages are left alone.
    assert_eq!(
        b.intercept(
            1,
            "https://ads.test/".into(),
            None,
            true,
            "GET".into(),
            None,
            None
        ),
        Interception::Allow
    );

    b.set_extension_enabled(id.to_string(), false).unwrap();
    assert_eq!(
        intercept("https://cdn.ads.test/x.png", None),
        Interception::Allow
    );
    b.set_extension_enabled(id.to_string(), true).unwrap();
    // Kept across restarts.
    drop(b);
    let (b, _) = browser(&dir, "phone", &http);
    assert_eq!(
        b.intercept(
            1,
            "https://cdn.ads.test/x.png".into(),
            page.clone(),
            false,
            "GET".into(),
            None,
            None
        ),
        Interception::Block
    );
    b.remove_extension(id.to_string()).unwrap();
    assert!(b.extensions().is_empty());
    assert_eq!(
        b.intercept(
            1,
            "https://cdn.ads.test/x.png".into(),
            page,
            false,
            "GET".into(),
            None,
            None
        ),
        Interception::Allow
    );
}

#[test]
fn user_scripts_run_in_pages_with_their_storage() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let url = "https://greasyfork.test/counter.user.js";
    http.0.lock().unwrap().insert(
        url.into(),
        b"// ==UserScript==\n// @name Counter\n// @version 2\n// @match https://*/*\n\
          // @grant GM_setValue\n// ==/UserScript==\nGM_setValue('n', 1);"
            .to_vec(),
    );
    let (b, host) = browser(&dir, "phone", &http);
    // The ad blocker's cosmetic filters and the password manager's script.
    assert_eq!(host.1.lock().unwrap().len(), 2);

    assert!(b.install_user_script("not a url".into()).is_err());
    assert_eq!(b.install_user_script(url.into()).unwrap(), "Counter 2");
    let scripts = host.1.lock().unwrap().clone();
    assert_eq!(scripts.len(), 3);
    assert!(scripts[2].contains("GM_setValue('n', 1);"));
    assert!(scripts[2].contains("const BRIDGE = \"android\";"));

    // The page script's storage goes through `page_message`, with its own
    // token (the ad blocker's, in the same page, is not it).
    let token_of = |script: &str| {
        script
            .split("const TOKEN = \"")
            .nth(1)
            .and_then(|rest| rest.split('"').next())
            .unwrap()
            .to_owned()
    };
    let blocker_token = token_of(&scripts[0]);
    let token = scripts[2]
        .split("const TOKEN = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    let id = b.user_scripts()[0].id.clone();
    let tab = PageTarget::Tab { tab: 1 };
    let set = format!(r#"{{"t":"{token}","id":1,"owner":"us:{id}","op":"set","items":{{"n":1}}}}"#);
    b.page_message(tab.clone(), set);
    assert_eq!(host.last_reply(), r#"{"id":1,"result":null}"#);
    let get = format!(r#"{{"t":"{token}","id":2,"owner":"us:{id}","op":"get"}}"#);
    b.page_message(tab.clone(), get);
    assert_eq!(host.last_reply(), r#"{"id":2,"result":{"n":1}}"#);
    let posted = host.2.lock().unwrap().len();
    b.page_message(
        tab.clone(),
        r#"{"t":"guess","id":3,"owner":"us:x","op":"get"}"#.into(),
    );
    b.page_message(
        tab,
        format!(r#"{{"t":"{blocker_token}","id":4,"owner":"us:{id}","op":"get"}}"#),
    );
    assert_eq!(
        host.2.lock().unwrap().len(),
        posted,
        "no answer to a stranger"
    );

    b.set_user_script_enabled(id.clone(), false).unwrap();
    assert_eq!(host.1.lock().unwrap().len(), 2);
    b.remove_user_script(id).unwrap();
    assert!(b.user_scripts().is_empty());
}

const LIST: &str = "[Adblock Plus 2.0]\n||ads.test^\nnews.test##.ad\n";

#[test]
fn updating_the_lists_says_when_the_ad_blocker_changed() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, _host) = browser(&dir, "phone", &http);
    // Offline, nothing cached: no engine.
    assert!(!b.update_blocker_lists());
    http.0.lock().unwrap().insert(
        infrastructure::blocker::LISTS[0].1.to_owned(),
        LIST.as_bytes().to_vec(),
    );
    assert!(b.update_blocker_lists());
    // Checked a moment ago: the same engine.
    assert!(!b.update_blocker_lists());
}

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
    assert!(
        script.contains(r#"location.hostname === "news.test""#),
        "{script}"
    );
    assert!(script.contains("window.said"), "{script}");
    assert_eq!(b.scriptlets("https://other.test/".into()), "");
    b.set_blocker(false).unwrap();
    assert_eq!(b.scriptlets("https://news.test/".into()), "");
}

#[test]
fn the_ad_blocker_blocks_and_counts() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, host) = browser(&dir, "phone", &http);
    let tab = b
        .open_tab(Some("https://news.test/".into()), false)
        .unwrap();
    b.on_load_finished(tab, "https://news.test/".into());
    let page = Some("https://news.test/".to_owned());
    let intercept = |url: &str, main_frame: bool| {
        b.intercept(
            tab,
            url.into(),
            (!main_frame).then(|| page.clone()).flatten(),
            main_frame,
            "GET".into(),
            None,
            Some("script".into()),
        )
    };
    // No list yet (offline): nothing blocked.
    b.update_blocker_lists();
    assert_eq!(
        intercept("https://ads.test/a.js", false),
        Interception::Allow
    );
    assert_eq!(b.blocker_info(tab).lists_updated, None);

    http.0.lock().unwrap().insert(
        infrastructure::blocker::LISTS[0].1.to_owned(),
        LIST.as_bytes().to_vec(),
    );
    b.update_blocker_lists();
    // Checked a moment ago: the next call loads nothing.
    b.update_blocker_lists();
    assert!(b.blocker_info(tab).lists_updated.is_some());
    assert_eq!(intercept("https://news.test/", true), Interception::Allow);
    assert_eq!(
        intercept("https://ads.test/a.js", false),
        Interception::Block
    );
    assert_eq!(
        intercept("https://ads.test/b.js", false),
        Interception::Block
    );
    assert_eq!(
        intercept("https://cdn.test/app.js", false),
        Interception::Allow
    );
    let info = b.blocker_info(tab);
    assert!(info.enabled && !info.site_allowed);
    assert_eq!(info.blocked, 2);
    // A new page counts again.
    assert_eq!(
        intercept("https://news.test/next", true),
        Interception::Allow
    );
    assert_eq!(b.blocker_info(tab).blocked, 0);

    // The page script asks the site's cosmetic filters.
    let scripts = host.1.lock().unwrap().clone();
    let token = scripts[0]
        .split("const TOKEN = \"")
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap()
        .to_owned();
    assert!(scripts[0].contains("blockerCosmetic();"));
    b.page_message(
        PageTarget::Tab { tab },
        format!(
            r#"{{"t":"{token}","ch":"c","id":1,"owner":"blocker","ctx":"content","op":"blocker.cosmetic","url":"https://news.test/"}}"#
        ),
    );
    assert!(host.last_reply().contains(".ad{display:none!important}"));

    b.set_blocker_for_site("www.news.test".into(), false)
        .unwrap();
    assert!(b.blocker_info(tab).site_allowed);
    assert_eq!(
        intercept("https://ads.test/a.js", false),
        Interception::Allow
    );
    b.set_blocker_for_site("news.test".into(), true).unwrap();
    assert_eq!(
        intercept("https://ads.test/a.js", false),
        Interception::Block
    );
    b.set_blocker(false).unwrap();
    assert!(!b.blocker_info(tab).enabled);
    assert_eq!(
        intercept("https://ads.test/a.js", false),
        Interception::Allow
    );
}

#[test]
fn errors_built_here_speak_the_language_set() {
    use i18n::{Lang, with_lang};
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, _) = browser(&dir, "phone", &http);
    let reason = |r: Result<(), MobileError>| r.unwrap_err().to_string();
    with_lang(Lang::Fr, || {
        assert_eq!(
            reason(b.sync_join("not a code".into())),
            "Ce code n’est pas valide\u{a0}: vérifiez-le sur l’autre appareil."
        );
        assert_eq!(
            reason(b.set_search_engine("Nope".into())),
            "Moteur de recherche inconnu\u{a0}: Nope"
        );
        assert_eq!(
            reason(b.install_extension("nothing".into()).map(drop)),
            "Collez un lien du Chrome Web Store ou l’identifiant d’une extension."
        );
    });
    with_lang(Lang::En, || {
        assert_eq!(
            reason(b.set_home_page("not an address".into())),
            "Not a web address: not an address"
        );
        assert_eq!(
            reason(b.remove_extension("x".into())),
            "Not an extension id."
        );
    });
}

#[test]
fn sign_ins_are_offered_saved_filled_and_the_setting_drops_the_script() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, host) = browser(&dir, "phone", &http);
    let tab = b.open_tab(None, false).unwrap();
    b.on_load_finished(tab, "https://a.com/login".into());
    // A frame's report (BUBridge is in every frame): no token, refused.
    let forged = r#"{"op":"credentials.submit","username":"evil","password":"x"}"#;
    b.page_message(PageTarget::Tab { tab }, forged.into());
    assert!(b.pending_credential().is_none());
    // The top frame's script has the token.
    let scripts = host.1.lock().unwrap().clone();
    let capture = scripts
        .iter()
        .find(|s| s.contains("captureSignIns();"))
        .unwrap();
    let token = capture
        .split("const TOKEN = \"")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    let submit =
        format!(r#"{{"t":"{token}","op":"credentials.submit","username":"me","password":"pw"}}"#);
    b.page_message(PageTarget::Tab { tab }, submit);
    let offer = b.pending_credential().unwrap();
    assert_eq!(
        (offer.origin.as_str(), offer.username.as_str()),
        ("https://a.com", "me")
    );
    b.answer_credential(CredentialChoice::Save).unwrap();
    assert!(b.pending_credential().is_none());
    assert_eq!(b.saved_credentials().len(), 1);

    b.on_load_finished(tab, "https://a.com/login".into());
    let (_, script) = host.2.lock().unwrap().last().cloned().unwrap();
    assert!(script.contains(r#"fillSignIn("https://a.com", "me", "pw")"#));

    assert!(b.general_settings().save_passwords);
    b.set_save_passwords(false).unwrap();
    assert!(!b.general_settings().save_passwords);
    assert_eq!(host.1.lock().unwrap().len(), 1, "the blocker's script only");
    b.remove_credential("https://a.com".into(), "me".into())
        .unwrap();
    assert!(b.saved_credentials().is_empty());
}

#[test]
fn a_crashed_page_waits_and_idle_tabs_sleep() {
    let dir = tempfile::tempdir().unwrap();
    let http = Arc::new(FakeHttp::default());
    let (b, host) = browser(&dir, "phone", &http);
    b.restore_session().unwrap();
    let first = b.state().active.unwrap();
    b.on_load_finished(first, "https://a.com/".into());
    let second = b.open_tab(Some("https://b.com/".into()), false).unwrap();
    b.on_load_finished(second, "https://b.com/".into());

    // Android killed the shown page's renderer: no reload behind its back.
    host.0.lock().unwrap().clear();
    b.on_page_crashed(second);
    assert!(b.state().active_crashed);
    assert_eq!(
        host.0.lock().unwrap().clone(),
        vec![format!("close {second}")]
    );
    b.reload();
    assert!(!b.state().active_crashed);
    assert!(
        host.0
            .lock()
            .unwrap()
            .contains(&format!("open {second} https://b.com/"))
    );

    // Short of memory: every tab but the one shown sleeps at once.
    assert_eq!(b.sleep_idle_tabs(0), 1);
    let state = b.state();
    assert!(state.tabs.iter().find(|t| t.id == first).unwrap().asleep);
    assert!(!state.tabs.iter().find(|t| t.id == second).unwrap().asleep);
}
