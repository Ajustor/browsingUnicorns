//! Storage of the scripts the browser runs in pages (`chrome.storage` of
//! extensions, `GM_setValue` of user scripts), and the bridge protocol their
//! messages use.
//!
//! A message is JSON: `{"t": token, "id": n, "owner": "ext:<id>"|"us:<id>",
//! "op": "get"|"set"|"remove"|"clear", "area": "local"|"sync"|"session",
//! "keys": [..]|null, "items": {..}, "ch": channel}`. The reply: `{"id": n,
//! "ch": channel, "result": ..}` or `{"id": n, "ch": channel, "error": ".."}`. The token, written only into the scripts
//! the browser injects, keeps pages from reading or writing this storage.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// The values of one script, for one area.
type Store = Map<String, Value>;

/// A store is written once its changes stop for this long...
const WRITE_AFTER: Duration = Duration::from_millis(500);
/// ...or at the latest this long after the first one.
const WRITE_WITHIN: Duration = Duration::from_secs(2);

pub struct ScriptStorage {
    dir: PathBuf,
    token: String,
    /// Loaded stores, by file name.
    cache: HashMap<String, Store>,
    /// Writes the stores, off the UI thread; started at the first change.
    writer: Option<Writer>,
}

/// What the writer is asked.
enum Job {
    /// The store's file and its new content.
    Write(PathBuf, String),
    /// Deletes the files in the folder whose name starts so.
    Forget(PathBuf, String),
    /// Answers once everything asked before is done.
    Flush(Sender<()>),
}

/// A thread that writes the stores: the last content of each, once changes
/// pause, so that a script setting a value many times writes it once.
struct Writer {
    tx: Option<Sender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl Writer {
    fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let thread = std::thread::spawn(move || write_loop(&rx));
        Self {
            tx: Some(tx),
            thread: Some(thread),
        }
    }

    fn send(&self, job: Job) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(job);
        }
    }
}

impl Drop for Writer {
    /// What is waiting is written before it goes (the browser closing).
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn write_loop(rx: &Receiver<Job>) {
    let mut waiting: HashMap<PathBuf, String> = HashMap::new();
    // When the first waiting change came.
    let mut since: Option<Instant> = None;
    loop {
        let job = match since {
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
            Some(first) => {
                let left = WRITE_WITHIN.saturating_sub(first.elapsed());
                rx.recv_timeout(WRITE_AFTER.min(left))
            }
        };
        match job {
            Ok(Job::Write(path, text)) => {
                waiting.insert(path, text);
                since.get_or_insert_with(Instant::now);
                continue;
            }
            Ok(Job::Forget(dir, prefix)) => {
                waiting.retain(|path, _| !file_starts_with(path, &prefix));
                forget_files(&dir, &prefix);
                continue;
            }
            Ok(Job::Flush(done)) => {
                write_all(&mut waiting);
                since = None;
                let _ = done.send(());
            }
            Err(RecvTimeoutError::Timeout) => {
                write_all(&mut waiting);
                since = None;
            }
            Err(RecvTimeoutError::Disconnected) => {
                write_all(&mut waiting);
                return;
            }
        }
    }
}

fn file_starts_with(path: &Path, prefix: &str) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with(prefix))
}

fn write_all(waiting: &mut HashMap<PathBuf, String>) {
    for (path, text) in waiting.drain() {
        let written = path
            .parent()
            .map_or(Ok(()), crate::files::create_dir_all)
            .map_err(|e| e.to_string())
            .and_then(|()| crate::files::write(&path, &text).map_err(|e| e.to_string()));
        if let Err(e) = written {
            log::warn!("script storage {}: {e}", path.display());
        }
    }
}

fn forget_files(dir: &Path, prefix: &str) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(prefix) {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}

#[derive(Deserialize)]
pub struct Message {
    pub t: String,
    /// The script's channel, echoed in the reply.
    #[serde(default)]
    pub ch: Option<String>,
    pub id: u64,
    pub owner: String,
    pub op: String,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub keys: Option<Value>,
    #[serde(default)]
    pub items: Option<Map<String, Value>>,
}

/// A new random token for this run of the browser.
pub fn new_token() -> String {
    use chacha20poly1305::aead::{OsRng, rand_core::RngCore};
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    data_encoding::HEXLOWER.encode(&bytes)
}

/// The token of `owner`'s scripts (`ext:<id>`, `us:<id>`, the ad
/// blocker's), from the run's `secret`: each owner's scripts get only
/// theirs, so a page that takes one (from the scripts injected into it)
/// can only speak as that owner, never as another extension. The secret
/// itself is never in a page.
pub fn owner_token(secret: &str, owner: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"browsing-unicorns page token\0");
    hash.update(secret.as_bytes());
    hash.update([0]);
    hash.update(owner.as_bytes());
    // Half of it: as long as a run's secret, and not extendable.
    data_encoding::HEXLOWER.encode(&hash.finalize()[..16])
}

pub fn valid_owner(owner: &str) -> bool {
    let (kind, id) = match owner.split_once(':') {
        Some(parts) => parts,
        None => return false,
    };
    matches!(kind, "ext" | "us")
        && !id.is_empty()
        && id.len() <= 64
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

impl ScriptStorage {
    pub fn new(dir: impl Into<PathBuf>, token: String) -> Self {
        Self {
            dir: dir.into(),
            token,
            cache: HashMap::new(),
            writer: None,
        }
    }

    /// Waits until every change made so far is written.
    pub fn flush(&self) {
        if let Some(writer) = &self.writer {
            let (tx, rx) = mpsc::channel();
            writer.send(Job::Flush(tx));
            let _ = rx.recv();
        }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    fn store(&mut self, file: &str) -> &mut Store {
        let path = self.dir.join(file);
        self.cache.entry(file.to_owned()).or_insert_with(|| {
            fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default()
        })
    }

    /// Has `file`'s store written in the background (soon, once its changes
    /// pause).
    fn save(&mut self, file: &str) -> Result<(), String> {
        let Some(store) = self.cache.get(file) else {
            return Ok(());
        };
        let text = serde_json::to_string(store).map_err(|e| e.to_string())?;
        self.writer
            .get_or_insert_with(Writer::start)
            .send(Job::Write(self.dir.join(file), text));
        Ok(())
    }

    /// Answers a message of a page script; `None` when it is not one (bad
    /// token or format): the page gets no answer.
    pub fn handle(&mut self, body: &str) -> Option<String> {
        let message: Message = serde_json::from_str(body).ok()?;
        if !valid_owner(&message.owner) || message.t != owner_token(&self.token, &message.owner) {
            return None;
        }
        let (id, ch) = (message.id, message.ch.clone());
        let mut reply = match self.apply(message) {
            Ok((result, _)) => json!({"id": id, "result": result}),
            Err(e) => json!({"id": id, "error": e}),
        };
        if let Some(ch) = ch {
            reply["ch"] = ch.into();
        }
        Some(reply.to_string())
    }

    /// Whether `op` is a storage operation.
    pub fn is_storage_op(op: &str) -> bool {
        matches!(op, "get" | "set" | "remove" | "clear")
    }

    /// Runs a storage operation (token already checked). Returns the result
    /// and, for a change, `chrome.storage.onChanged`'s argument and the area.
    pub fn apply(&mut self, m: Message) -> Result<(Value, Option<(Value, String)>), String> {
        let area = match m.area.as_deref().unwrap_or("local") {
            a @ ("local" | "sync" | "session") => a,
            other => return Err(format!("unknown storage area {other}")),
        };
        let file = format!("{}.{area}.json", m.owner.replace(':', "-"));
        // `session` is kept in memory only.
        let persist = area != "session";
        let before = |store: &Store, keys: &[String]| -> Map<String, Value> {
            keys.iter()
                .filter_map(|k| store.get(k).map(|v| (k.clone(), v.clone())))
                .collect()
        };
        let changes = |old: Map<String, Value>, new: &Store, keys: &[String]| -> Value {
            let mut out = Map::new();
            for k in keys {
                let (o, n) = (old.get(k), new.get(k));
                if o == n {
                    continue;
                }
                let mut change = Map::new();
                if let Some(o) = o {
                    change.insert("oldValue".into(), o.clone());
                }
                if let Some(n) = n {
                    change.insert("newValue".into(), n.clone());
                }
                out.insert(k.clone(), Value::Object(change));
            }
            Value::Object(out)
        };
        let area_name = area.to_owned();
        let changed = |c: Value| match &c {
            Value::Object(m) if m.is_empty() => None,
            _ => Some((c, area_name.clone())),
        };
        match m.op.as_str() {
            "get" => {
                let store = self.store(&file);
                Ok(Value::Object(match m.keys {
                    None | Some(Value::Null) => store.clone(),
                    Some(Value::String(k)) => {
                        store.get(&k).map(|v| (k, v.clone())).into_iter().collect()
                    }
                    Some(Value::Array(keys)) => keys
                        .iter()
                        .filter_map(|k| k.as_str())
                        .filter_map(|k| store.get(k).map(|v| (k.to_owned(), v.clone())))
                        .collect(),
                    // An object: its values are the defaults.
                    Some(Value::Object(defaults)) => defaults
                        .into_iter()
                        .map(|(k, d)| {
                            let v = store.get(&k).cloned().unwrap_or(d);
                            (k, v)
                        })
                        .collect(),
                    Some(_) => return Err("bad keys".into()),
                }))
                .map(|v| (v, None))
            }
            "set" => {
                let items = m.items.ok_or("nothing to set")?;
                let keys: Vec<String> = items.keys().cloned().collect();
                let store = self.store(&file);
                let old = before(store, &keys);
                for (k, v) in items {
                    store.insert(k, v);
                }
                let diff = changes(old, store, &keys);
                if persist {
                    self.save(&file)?;
                }
                Ok((Value::Null, changed(diff)))
            }
            "remove" => {
                let keys: Vec<String> = match m.keys {
                    Some(Value::String(k)) => vec![k],
                    Some(Value::Array(keys)) => keys
                        .into_iter()
                        .filter_map(|k| k.as_str().map(str::to_owned))
                        .collect(),
                    _ => return Err("bad keys".into()),
                };
                let store = self.store(&file);
                let old = before(store, &keys);
                for k in &keys {
                    store.remove(k);
                }
                let diff = changes(old, store, &keys);
                if persist {
                    self.save(&file)?;
                }
                Ok((Value::Null, changed(diff)))
            }
            "clear" => {
                let store = self.store(&file);
                let keys: Vec<String> = store.keys().cloned().collect();
                let old = before(store, &keys);
                store.clear();
                let diff = changes(old, store, &keys);
                if persist {
                    self.save(&file)?;
                }
                Ok((Value::Null, changed(diff)))
            }
            other => Err(format!("unknown operation {other}")),
        }
    }

    /// Deletes what `owner` stored (a removed extension or script).
    pub fn forget(&mut self, owner: &str) {
        let prefix = format!("{}.", owner.replace(':', "-"));
        self.cache.retain(|file, _| !file.starts_with(&prefix));
        // Through the writer: a write of these files still waiting there
        // must not bring them back.
        match &self.writer {
            Some(writer) => writer.send(Job::Forget(self.dir.clone(), prefix)),
            None => forget_files(&self.dir, &prefix),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn send(s: &mut ScriptStorage, msg: Value) -> Value {
        serde_json::from_str(&s.handle(&msg.to_string()).expect("an answer")).unwrap()
    }

    #[test]
    fn storage_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ScriptStorage::new(dir.path(), "tok".into());
        let owner = "ext:ddkjiahejlhfcafbddmgiahcphecmpfh";
        let base =
            |op: &str| json!({"t": owner_token("tok", owner), "id": 7, "owner": owner, "op": op});

        let mut set = base("set");
        set["items"] = json!({"a": 1, "b": {"x": true}});
        assert_eq!(send(&mut s, set), json!({"id": 7, "result": null}));

        let mut get = base("get");
        get["keys"] = json!(["a", "missing"]);
        assert_eq!(send(&mut s, get)["result"], json!({"a": 1}));
        let mut defaults = base("get");
        defaults["keys"] = json!({"a": 0, "c": "d"});
        assert_eq!(send(&mut s, defaults)["result"], json!({"a": 1, "c": "d"}));

        // Kept on disk (written in the background, at the latest when it
        // goes).
        drop(s);
        let mut again = ScriptStorage::new(dir.path(), "tok".into());
        assert_eq!(
            send(&mut again, base("get"))["result"],
            json!({"a": 1, "b": {"x": true}})
        );

        let mut remove = base("remove");
        remove["keys"] = json!("a");
        send(&mut again, remove);
        assert_eq!(
            send(&mut again, base("get"))["result"],
            json!({"b": {"x": true}})
        );

        // Areas and owners are separate; session is not saved.
        let mut session = base("set");
        session["area"] = json!("session");
        session["items"] = json!({"s": 1});
        send(&mut again, session);
        let mut other = base("get");
        other["owner"] = json!("us:wide-1234");
        other["t"] = json!(owner_token("tok", "us:wide-1234"));
        assert_eq!(send(&mut again, other)["result"], json!({}));
        assert!(
            !dir.path()
                .join(format!("{}.session.json", owner.replace(':', "-")))
                .exists()
        );

        send(&mut again, base("clear"));
        assert_eq!(send(&mut again, base("get"))["result"], json!({}));
        again.forget(owner);
        again.flush();
        assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn changes_are_written_once_they_pause() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ScriptStorage::new(dir.path(), "tok".into());
        let file = dir.path().join("us-a.local.json");
        for i in 0..20 {
            let set = json!({"t": owner_token("tok", "us:a"), "id": i, "owner": "us:a", "op": "set", "items": {"n": i}});
            send(&mut s, set);
        }
        s.flush();
        assert_eq!(
            serde_json::from_str::<Value>(&fs::read_to_string(&file).unwrap()).unwrap(),
            json!({"n": 19})
        );
        // A forgotten owner's waiting write does not bring its file back.
        let set = json!({"t": owner_token("tok", "us:a"), "id": 99, "owner": "us:a", "op": "set", "items": {"n": 0}});
        send(&mut s, set);
        s.forget("us:a");
        drop(s);
        assert!(!file.exists());
    }

    #[test]
    fn refuses_strangers() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = ScriptStorage::new(dir.path(), "tok".into());
        let msg = |t: &str, owner: &str| {
            json!({"t": t, "id": 1, "owner": owner, "op": "get"}).to_string()
        };
        let token = |owner: &str| owner_token("tok", owner);
        assert!(s.handle(&msg("wrong", "ext:abc")).is_none());
        // The run's secret itself, or another owner's token: refused.
        assert!(s.handle(&msg("tok", "ext:abc")).is_none());
        assert!(s.handle(&msg(&token("us:abc"), "ext:abc")).is_none());
        assert!(
            s.handle(&msg(&token("ext:../../x"), "ext:../../x"))
                .is_none()
        );
        assert!(s.handle(&msg(&token("page:abc"), "page:abc")).is_none());
        assert!(s.handle("not json").is_none());
        assert!(s.handle(&msg(&token("us:abc"), "us:abc")).is_some());
        assert_ne!(token("us:abc"), token("us:abd"));
        assert_eq!(token("us:abc").len(), 32);
        assert_eq!(new_token().len(), 32);
    }
}
