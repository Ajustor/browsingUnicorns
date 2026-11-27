//! Native messaging (`runtime.connectNative`): ports to native hosts.

use application::ports::PageSource;
use serde_json::{Value, json};

use super::{Answer, Call, Hub, HubHost, unsupported};

use crate::native_messaging::{self, FromHost};

impl Hub {
    pub(super) fn native_call(&mut self, call: &Call<'_>) -> Answer {
        let Call {
            from, ctx, owner, ..
        } = *call;
        let ext = call.ext;
        let field = |k: &str| call.field(k);
        match call.op {
            "native.connect" => {
                let ext = ext?;
                // As in Chrome: extension pages only, and only with the
                // permission.
                if ctx == "content"
                    || !self
                        .catalog
                        .get(ext)
                        .is_some_and(|p| p.has_permission("nativeMessaging"))
                {
                    return Some(Err(native_messaging::NOT_FOUND.into()));
                }
                let name = field("app");
                let result = self
                    .native
                    .connect(name.as_str().unwrap_or(""), ext)
                    .map(|port| {
                        self.native_ports
                            .insert(port, (from.clone(), owner.to_owned()));
                        Value::from(port)
                    });
                Some(result)
            }
            "native.post" => {
                let port = field("port").as_u64()?;
                if self.native_ports.get(&port).map(|(_, o)| o.as_str()) != Some(owner) {
                    return Some(Err("Attempting to use a disconnected port object".into()));
                }
                Some(self.native.post(port, &field("msg")).map(|()| Value::Null))
            }
            "native.disconnect" => {
                let port = field("port").as_u64()?;
                if self.native_ports.get(&port).map(|(_, o)| o.as_str()) == Some(owner) {
                    self.native_ports.remove(&port);
                    self.native.disconnect(port);
                }
                Some(Ok(Value::Null))
            }
            op => unsupported(op),
        }
    }

    /// Hands what native hosts sent to the pages holding their ports.
    pub(super) fn poll_native(&mut self, host: &mut dyn HubHost) {
        for (port, event) in self.native.poll() {
            let Some((to, owner)) = self.native_ports.get(&port).cloned() else {
                continue;
            };
            let event = match event {
                FromHost::Message(msg) => {
                    json!({"event": "native", "owner": owner, "port": port, "msg": msg})
                }
                FromHost::Closed(error) => {
                    self.native_ports.remove(&port);
                    json!({"event": "nativeDisconnect", "owner": owner, "port": port, "error": error})
                }
            };
            host.deliver(&to, &event.to_string());
        }
    }

    /// Closes the native ports of the pages in `from`.
    pub(super) fn close_native_ports(&mut self, from: impl Fn(&PageSource, &str) -> bool) {
        let ports: Vec<u64> = self
            .native_ports
            .iter()
            .filter(|(_, (page, owner))| from(page, owner))
            .map(|(port, _)| *port)
            .collect();
        for port in ports {
            self.native_ports.remove(&port);
            self.native.disconnect(port);
        }
    }
}
