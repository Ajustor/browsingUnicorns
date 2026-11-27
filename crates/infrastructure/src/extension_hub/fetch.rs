//! `fetch` of the extensions' own pages to their permitted hosts, made by
//! the browser off the UI thread.

use serde_json::json;

use super::{Answer, Call, Hub, HubHost, unsupported};
use crate::extension_fetch::{self, FetchRequest};
use crate::extensions::extension_dir;

impl Hub {
    pub(super) fn fetch_call(&mut self, call: &Call<'_>) -> Answer {
        let Call {
            from, ctx, id, ch, ..
        } = *call;
        let ext = call.ext;
        let field = |k: &str| call.field(k);
        match call.op {
            "net.fetch" => {
                // As in Chrome: the extension's own pages, to the hosts its
                // permissions cover. Anything else: the page's own fetch.
                let ext = ext?;
                let url = field("url").as_str().unwrap_or("").to_owned();
                let fetcher = self.fetcher.clone().filter(|_| {
                    ctx != "content"
                        && extension_fetch::allowed(&extension_dir(&self.extensions_dir, ext), &url)
                });
                let Some(fetcher) = fetcher else {
                    return Some(Err("not permitted".into()));
                };
                let request = FetchRequest {
                    url,
                    method: field("method")
                        .as_str()
                        .unwrap_or("GET")
                        .to_ascii_uppercase(),
                    headers: field("headers")
                        .as_array()
                        .map(|pairs| {
                            pairs
                                .iter()
                                .filter_map(|p| {
                                    Some((
                                        p.get(0)?.as_str()?.to_owned(),
                                        p.get(1)?.as_str()?.to_owned(),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    body: field("body")
                        .as_str()
                        .and_then(|b| data_encoding::BASE64.decode(b.as_bytes()).ok()),
                };
                let (tx, wake, to, ch) = (
                    self.fetched_tx.clone(),
                    self.wake.clone(),
                    from.clone(),
                    ch.clone(),
                );
                std::thread::spawn(move || {
                    let result = fetcher(request).map(|r| {
                        json!({
                            "url": r.url,
                            "status": r.status,
                            "statusText": r.status_text,
                            "headers": r.headers,
                            "body": data_encoding::BASE64.encode(&r.body),
                        })
                    });
                    if tx.send((to, id, ch, result)).is_ok() {
                        wake();
                    }
                });
                None
            }
            op => unsupported(op),
        }
    }

    /// Hands finished requests to the pages that made them.
    pub(super) fn deliver_fetched(&mut self, host: &mut dyn HubHost) {
        while let Ok((to, id, ch, result)) = self.fetched_rx.try_recv() {
            Self::reply(host, &to, id, &ch, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use application::ports::PageSource;
    use domain::TabId;
    use serde_json::json;

    use super::super::tests::{Host, ext, hub, msg};
    use super::*;

    #[test]
    fn extension_pages_fetch_their_hosts_without_cors() {
        let dir = tempfile::tempdir().unwrap();
        let mut hub = hub(dir.path());
        let ext_dir = extension_dir(&dir.path().join("ext"), &ext());
        fs::create_dir_all(&ext_dir).unwrap();
        fs::write(
            ext_dir.join("manifest.json"),
            r#"{"host_permissions": ["https://api.test/*"]}"#,
        )
        .unwrap();
        hub.set_fetcher(Arc::new(|r: FetchRequest| {
            Ok(extension_fetch::FetchResponse {
                url: r.url,
                status: 200,
                status_text: "OK".into(),
                headers: vec![("content-type".into(), "text/plain".into())],
                body: format!(
                    "{} {}",
                    r.method,
                    String::from_utf8(r.body.unwrap_or_default()).unwrap()
                )
                .into_bytes(),
            })
        }));
        let mut host = Host::default();
        let bg = PageSource::Background(ext());
        let body = data_encoding::BASE64.encode(b"hi");

        hub.handle(
            &bg,
            &msg(
                1,
                "background",
                "net.fetch",
                json!({"url": "https://api.test/x", "method": "post", "body": body}),
            ),
            &mut host,
        );
        // Made off the UI thread: wait for it.
        for _ in 0..100 {
            hub.pump_native(&mut host);
            if !host.delivered.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let (to, reply) = host.delivered.pop().unwrap();
        assert_eq!(to, bg);
        assert_eq!(reply["result"]["status"], 200);
        let got = data_encoding::BASE64
            .decode(reply["result"]["body"].as_str().unwrap().as_bytes())
            .unwrap();
        assert_eq!(got, b"POST hi");

        // Another host, or a content script: the page's own fetch.
        hub.handle(
            &bg,
            &msg(
                2,
                "background",
                "net.fetch",
                json!({"url": "https://evil.test/"}),
            ),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["error"], "not permitted");
        hub.handle(
            &PageSource::Tab(TabId::new(1)),
            &msg(
                3,
                "content",
                "net.fetch",
                json!({"url": "https://api.test/x"}),
            ),
            &mut host,
        );
        assert_eq!(host.delivered.pop().unwrap().1["error"], "not permitted");
    }
}
