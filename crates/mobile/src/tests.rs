use std::collections::BTreeMap;
use std::sync::Mutex;

use data_encoding::BASE64;

use super::*;

/// Records what Rust asks of the WebViews.
#[derive(Default)]
struct FakeHost(Mutex<Vec<String>>);

impl WebViewHost for FakeHost {
    fn open_view(&self, tab: u64, url: String) {
        self.0.lock().unwrap().push(format!("open {tab} {url}"));
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
    assert!(b.open_tab(Some("javascript:x".into())).is_err());
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

/// A CRX2 package of a blocker: `||ads.test^` blocked, `/ad.js` replaced by
/// its stand-in.
fn blocker_package() -> (domain::ExtensionId, Vec<u8>) {
    use std::io::Write;

    use sha2::{Digest, Sha256};

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
    let key = b"blocker key";
    let mut crx = b"Cr24".to_vec();
    for n in [2u32, key.len() as u32, 0] {
        crx.extend(n.to_le_bytes());
    }
    crx.extend(key);
    crx.extend(zip);
    (
        domain::ExtensionId::from_key_hash(&Sha256::digest(key)),
        crx,
    )
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
