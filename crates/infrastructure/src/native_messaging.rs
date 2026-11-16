//! Chrome's native messaging: an extension with the `nativeMessaging`
//! permission talks to a program installed on the computer (a password
//! manager's desktop app...). The program declares itself in a manifest
//! (found as Chrome finds it), names the extensions it accepts, and speaks
//! JSON messages over stdin/stdout, each one after its length (4 bytes,
//! native byte order).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};

use domain::ExtensionId;
use serde_json::Value;

/// Chrome's answer when there is no such host.
pub const NOT_FOUND: &str = "Specified native messaging host not found.";

/// Largest message a host may send (Chrome's limit).
const MAX_FROM_HOST: u32 = 1024 * 1024;

/// A host's manifest, as far as it matters here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostManifest {
    pub path: PathBuf,
    pub allowed_origins: Vec<String>,
}

/// Host names: dot-separated lowercase words (Chrome's rule).
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        })
}

/// The origin hosts list in `allowed_origins` for extension `id`.
pub fn origin(id: &ExtensionId) -> String {
    format!("chrome-extension://{id}/")
}

/// Reads the manifest at `file`; a relative `path` is relative to it.
pub fn read_manifest(file: &Path, name: &str) -> Result<HostManifest, String> {
    let text = std::fs::read_to_string(file).map_err(|_| NOT_FOUND.to_owned())?;
    let value: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|e| format!("{}: {e}", file.display()))?;
    if value.get("name").and_then(Value::as_str) != Some(name) {
        return Err(format!("{}: not the manifest of {name}", file.display()));
    }
    if value.get("type").and_then(Value::as_str) != Some("stdio") {
        return Err(format!(
            "{}: only stdio hosts are supported",
            file.display()
        ));
    }
    let path = value
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{}: no path", file.display()))?;
    let path = PathBuf::from(path);
    let path = if path.is_absolute() {
        path
    } else {
        file.parent().unwrap_or(Path::new(".")).join(path)
    };
    let allowed_origins = value
        .get("allowed_origins")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Ok(HostManifest {
        path,
        allowed_origins,
    })
}

/// Where host `name` declares itself, in Chrome's order (Edge's and
/// Chromium's places too: hosts often register for several browsers).
fn manifest_files(name: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    #[cfg(all(windows, feature = "desktop"))]
    {
        for browser in [
            r"Software\Google\Chrome",
            r"Software\Microsoft\Edge",
            r"Software\Chromium",
        ] {
            let key = format!(r"{browser}\NativeMessagingHosts\{name}");
            files.extend(
                registry::default_values(&key)
                    .into_iter()
                    .map(PathBuf::from),
            );
        }
    }
    #[cfg(target_os = "macos")]
    {
        let dirs = ["Google/Chrome", "Microsoft Edge", "Chromium"];
        if let Some(home) = dirs::home_dir() {
            for d in dirs {
                files.push(
                    home.join("Library/Application Support")
                        .join(d)
                        .join("NativeMessagingHosts")
                        .join(format!("{name}.json")),
                );
            }
        }
        for d in ["Google/Chrome", "Microsoft/Edge", "Chromium"] {
            files.push(
                Path::new("/Library")
                    .join(d)
                    .join("NativeMessagingHosts")
                    .join(format!("{name}.json")),
            );
        }
    }
    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android")))]
    {
        if let Some(config) = dirs::config_dir() {
            for d in ["google-chrome", "microsoft-edge", "chromium"] {
                files.push(
                    config
                        .join(d)
                        .join("NativeMessagingHosts")
                        .join(format!("{name}.json")),
                );
            }
        }
        for d in [
            "/etc/opt/chrome/native-messaging-hosts",
            "/etc/opt/edge/native-messaging-hosts",
            "/etc/chromium/native-messaging-hosts",
        ] {
            files.push(Path::new(d).join(format!("{name}.json")));
        }
    }
    let _ = name;
    files
}

/// Finds host `name` and checks that it accepts extension `ext`.
pub fn find(name: &str, ext: &ExtensionId) -> Result<HostManifest, String> {
    if !valid_name(name) {
        return Err("Invalid native messaging host name specified.".into());
    }
    let mut last_error = NOT_FOUND.to_owned();
    for file in manifest_files(name) {
        match read_manifest(&file, name) {
            Ok(m) if m.allowed_origins.contains(&origin(ext)) => return Ok(m),
            Ok(_) => {
                last_error = "Access to the specified native messaging host is forbidden.".into()
            }
            Err(e) if e == NOT_FOUND => {}
            Err(e) => last_error = e,
        }
    }
    Err(last_error)
}

#[cfg(all(windows, feature = "desktop"))]
mod registry {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows_core::{HSTRING, PCWSTR};

    /// The default value of `key` under the user's then the machine's hive.
    pub fn default_values(key: &str) -> Vec<String> {
        [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE]
            .into_iter()
            .filter_map(|hive| read(hive, key))
            .collect()
    }

    fn read(hive: HKEY, key: &str) -> Option<String> {
        let key = HSTRING::from(key);
        let mut size = 0u32;
        unsafe {
            RegGetValueW(
                hive,
                &key,
                PCWSTR::null(),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&raw mut size),
            )
            .ok()
            .ok()?;
            let mut buf = vec![0u16; (size as usize).div_ceil(2)];
            RegGetValueW(
                hive,
                &key,
                PCWSTR::null(),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&raw mut size),
            )
            .ok()
            .ok()?;
            let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..len]))
        }
    }
}

/// Writes `message` with its length before it.
pub fn write_message(out: &mut impl Write, message: &Value) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(message)?;
    let len = u32::try_from(bytes.len()).map_err(|_| std::io::Error::other("message too big"))?;
    out.write_all(&len.to_ne_bytes())?;
    out.write_all(&bytes)?;
    out.flush()
}

/// Reads one message; `Ok(None)` at the end of the stream.
pub fn read_message(input: &mut impl Read) -> std::io::Result<Option<Value>> {
    let mut len = [0u8; 4];
    match input.read_exact(&mut len) {
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        r => r?,
    }
    let len = u32::from_ne_bytes(len);
    if len > MAX_FROM_HOST {
        return Err(std::io::Error::other("native message too big"));
    }
    let mut bytes = vec![0u8; len as usize];
    input.read_exact(&mut bytes)?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(std::io::Error::other)
}

/// What a host sent: a message, or the end (with why, if it failed).
#[derive(Debug)]
pub enum FromHost {
    Message(Value),
    Closed(Option<String>),
}

struct Port {
    child: Child,
    stdin: Option<ChildStdin>,
}

/// The ports open to native hosts, by id.
pub struct NativeMessaging {
    ports: HashMap<u64, Port>,
    next: u64,
    tx: Sender<(u64, FromHost)>,
    rx: Receiver<(u64, FromHost)>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Default for NativeMessaging {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            ports: HashMap::new(),
            next: 1,
            tx,
            rx,
            wake: Arc::new(|| {}),
        }
    }
}

impl NativeMessaging {
    /// Called when a host sends something (the UI then polls).
    pub fn set_waker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.wake = Arc::new(wake);
    }

    /// Starts host `name` for extension `ext`; the port's id.
    pub fn connect(&mut self, name: &str, ext: &ExtensionId) -> Result<u64, String> {
        let host = find(name, ext)?;
        self.spawn(&host.path, &origin(ext))
    }

    /// Starts `program` with Chrome's arguments.
    pub fn spawn(&mut self, program: &Path, origin: &str) -> Result<u64, String> {
        let mut command = Command::new(program);
        command
            .arg(origin)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(dir) = program.parent() {
            command.current_dir(dir);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt as _;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command
                .arg("--parent-window=0")
                .creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = command.spawn().map_err(|e| {
            log::warn!("native messaging: {}: {e}", program.display());
            "Error when communicating with the native messaging host.".to_owned()
        })?;
        let id = self.next;
        self.next += 1;
        let mut stdout = child.stdout.take().expect("piped");
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        std::thread::spawn(move || {
            let end = loop {
                match read_message(&mut stdout) {
                    Ok(Some(message)) => {
                        if tx.send((id, FromHost::Message(message))).is_err() {
                            return;
                        }
                        wake();
                    }
                    Ok(None) => break None,
                    Err(e) => break Some(e.to_string()),
                }
            };
            let error = end.map(|e| {
                log::warn!("native messaging: {e}");
                "Error when communicating with the native messaging host.".to_owned()
            });
            let _ = tx.send((id, FromHost::Closed(error)));
            wake();
        });
        let stdin = child.stdin.take();
        self.ports.insert(id, Port { child, stdin });
        Ok(id)
    }

    /// Sends `message` to port `id`'s host.
    pub fn post(&mut self, id: u64, message: &Value) -> Result<(), String> {
        let port = self
            .ports
            .get_mut(&id)
            .ok_or("Attempting to use a disconnected port object")?;
        let stdin = port
            .stdin
            .as_mut()
            .ok_or("Attempting to use a disconnected port object")?;
        write_message(stdin, message).map_err(|e| {
            log::warn!("native messaging: {e}");
            "Error when communicating with the native messaging host.".to_owned()
        })
    }

    /// Closes port `id`: the host gets the end of its input, then is stopped.
    pub fn disconnect(&mut self, id: u64) {
        if let Some(mut port) = self.ports.remove(&id) {
            drop(port.stdin.take());
            let _ = port.child.kill();
            let _ = port.child.wait();
        }
    }

    /// What the hosts sent since the last call. Closed ports are forgotten.
    pub fn poll(&mut self) -> Vec<(u64, FromHost)> {
        let events: Vec<_> = self.rx.try_iter().collect();
        for (id, event) in &events {
            if matches!(event, FromHost::Closed(_)) {
                self.disconnect(*id);
            }
        }
        events
    }
}

impl Drop for NativeMessaging {
    fn drop(&mut self) {
        let ids: Vec<u64> = self.ports.keys().copied().collect();
        for id in ids {
            self.disconnect(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names() {
        assert!(valid_name("com.8bit.bitwarden"));
        assert!(valid_name("org.keepassxc.keepassxc_browser"));
        assert!(!valid_name("Com.Upper"));
        assert!(!valid_name("a..b"));
        assert!(!valid_name("../evil"));
        assert!(!valid_name(""));
    }

    #[test]
    fn messages_round_trip() {
        let mut buf = Vec::new();
        write_message(&mut buf, &serde_json::json!({"command": "hi", "n": 1})).unwrap();
        assert_eq!(&buf[..4], &(buf.len() as u32 - 4).to_ne_bytes());
        let mut input = std::io::Cursor::new(buf);
        assert_eq!(
            read_message(&mut input).unwrap(),
            Some(serde_json::json!({"command": "hi", "n": 1}))
        );
        assert_eq!(read_message(&mut input).unwrap(), None);
    }

    #[test]
    fn refuses_huge_messages() {
        let mut input = std::io::Cursor::new((MAX_FROM_HOST + 1).to_ne_bytes().to_vec());
        assert!(read_message(&mut input).is_err());
    }

    #[test]
    fn reads_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("com.example.host.json");
        std::fs::write(
            &file,
            r#"{"name":"com.example.host","description":"x","path":"host.exe","type":"stdio",
               "allowed_origins":["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"]}"#,
        )
        .unwrap();
        let m = read_manifest(&file, "com.example.host").unwrap();
        assert_eq!(m.path, dir.path().join("host.exe"));
        assert_eq!(
            m.allowed_origins,
            ["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"]
        );
        assert!(read_manifest(&file, "com.other").is_err());
        assert_eq!(
            read_manifest(&dir.path().join("missing.json"), "com.example.host").unwrap_err(),
            NOT_FOUND
        );
    }

    #[test]
    fn unknown_hosts_are_not_found() {
        let ext = ExtensionId::parse("abcdefghijklmnopabcdefghijklmnop").unwrap();
        assert_eq!(
            find("com.example.surely_not_installed", &ext).unwrap_err(),
            NOT_FOUND
        );
        assert!(find("../x", &ext).is_err());
    }

    /// A real host: this crate's test binary has none, so a tiny echo
    /// script plays one (Windows: PowerShell is always there).
    #[cfg(windows)]
    #[test]
    fn talks_to_a_host() {
        let dir = tempfile::tempdir().unwrap();
        // Echoes one message back, then exits.
        let script = dir.path().join("echo.ps1");
        std::fs::write(
            &script,
            r#"$in = [Console]::OpenStandardInput(); $out = [Console]::OpenStandardOutput()
$len = New-Object byte[] 4; [void]$in.Read($len, 0, 4)
$n = [BitConverter]::ToInt32($len, 0); $buf = New-Object byte[] $n
$read = 0; while ($read -lt $n) { $read += $in.Read($buf, $read, $n - $read) }
$out.Write($len, 0, 4); $out.Write($buf, 0, $n); $out.Flush()"#,
        )
        .unwrap();
        let launcher = dir.path().join("echo.cmd");
        std::fs::write(
            &launcher,
            format!(
                "@powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\"\r\n",
                script.display()
            ),
        )
        .unwrap();
        let mut native = NativeMessaging::default();
        let id = native.spawn(&launcher, "chrome-extension://x/").unwrap();
        native.post(id, &serde_json::json!({"ping": true})).unwrap();
        let mut got = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline
            && !got.iter().any(|(_, e)| matches!(e, FromHost::Closed(_)))
        {
            got.extend(native.poll());
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            got.iter().any(|(i, e)| *i == id
                && matches!(e, FromHost::Message(m) if m == &serde_json::json!({"ping": true}))),
            "{got:?}"
        );
        assert!(got.iter().any(|(_, e)| matches!(e, FromHost::Closed(None))));
    }
}
