// Runtime of the scripts Browsing Unicorns runs where the web view has no
// extension engine: extensions' content scripts, extension pages
// (background, popup, options) and user scripts.
//
// Each script gets its own copy, wrapped in a function that defines TOKEN,
// CHANNEL (random, per script) and BRIDGE: nothing here is visible to the
// page but the hook Rust answers through (`__buReply`), which the page cannot
// replace and which only delivers to the channels the scripts registered.

const pending = new Map();
let nextId = 1;
const eventHandlers = [];

function deliver(reply) {
  const p = pending.get(reply.id);
  if (!p) return;
  pending.delete(reply.id);
  if (reply.error !== undefined) p.reject(new Error(reply.error));
  else p.resolve(reply.result);
}

// The hook: replies go to their channel, events to every script of the
// extension they are for.
if (!Object.getOwnPropertyDescriptor(window, "__buReply")) {
  const channels = new Map();
  const listeners = [];
  const hook = (msg) => {
    if (!msg) return;
    if (msg.event !== undefined) {
      const to = listeners.filter((l) => l.owner === msg.owner);
      // A message reaches every script of the extension in the page: the
      // first answer wins, "no answer" only once none of them has one.
      if (msg.event === "message") msg.answer = { left: to.length, done: false };
      for (const l of to) l.f(msg);
      return;
    }
    const to = channels.get(msg.ch);
    if (to) to(msg);
  };
  Object.defineProperty(hook, "register", {
    value: (ch, f) => { if (!channels.has(ch)) channels.set(ch, f); },
  });
  Object.defineProperty(hook, "listen", {
    value: (ch, owner, f) => { if (channels.has(ch)) listeners.push({ owner, f }); },
  });
  Object.defineProperty(window, "__buReply", {
    value: hook,
    writable: false,
    configurable: false,
    enumerable: false,
  });
}
window.__buReply.register(CHANNEL, deliver);

function onEvents(owner, f) {
  if (eventHandlers.length === 0) window.__buReply.listen(CHANNEL, owner, (msg) => eventHandlers.forEach((h) => h(msg)));
  eventHandlers.push(f);
}

let post;
if (BRIDGE === "android") {
  const bridge = window.BUBridge;
  post = (msg) => {
    if (!bridge) return deliver({ id: msg.id, error: "no bridge" });
    try {
      // The answer comes back through the hook, like on the desktop.
      bridge.call(JSON.stringify(msg));
    } catch (e) {
      deliver({ id: msg.id, error: String(e) });
    }
  };
} else {
  const ipc = window.ipc;
  const postMessage = ipc && ipc.postMessage ? ipc.postMessage.bind(ipc) : null;
  post = (msg) => {
    if (postMessage) postMessage(JSON.stringify(msg));
    else deliver({ id: msg.id, error: "no bridge" });
  };
}

let context = "content";

function send(owner, op, fields) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    post(Object.assign({ t: TOKEN, ch: CHANNEL, id, owner, ctx: context, op }, fields));
  });
}

// --- when and where ----------------------------------------------------------

const isTop = window === window.top;

// A pattern from the bundle; an invalid one matches nothing.
function re(source, flags) {
  try {
    return new RegExp(source, flags);
  } catch (e) {
    console.warn("[Browsing Unicorns] bad pattern", source, e);
    return /$^/;
  }
}

function matches(entry, href) {
  return entry.match.some((r) => r.test(href)) && !entry.exclude.some((r) => r.test(href));
}

function at(runAt, fn) {
  const run = () => {
    try {
      fn();
    } catch (e) {
      console.error("[Browsing Unicorns]", e);
    }
  };
  if (runAt === "document_start") return run();
  if (runAt === "document_end") {
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", run, { once: true });
    else run();
    return;
  }
  // document_idle: after the DOM is ready, once the page had a moment.
  const later = () => setTimeout(run, 0);
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", later, { once: true });
  else later();
}

function addStyle(css) {
  const style = document.createElement("style");
  style.textContent = css;
  const attach = () => (document.head || document.documentElement).appendChild(style);
  if (document.documentElement) attach();
  else
    new MutationObserver((_, observer) => {
      if (document.documentElement) {
        observer.disconnect();
        attach();
      }
    }).observe(document, { childList: true });
  return style;
}

function event() {
  const listeners = new Set();
  return {
    addListener: (f) => listeners.add(f),
    removeListener: (f) => listeners.delete(f),
    hasListener: (f) => listeners.has(f),
    hasListeners: () => listeners.size > 0,
    fire: (...args) => {
      const results = [];
      listeners.forEach((f) => {
        try {
          results.push(f(...args));
        } catch (e) {
          console.error(e);
        }
      });
      return results;
    },
  };
}

// --- chrome.* ---------------------------------------------------------------------

function withCallback(promise, callback, api) {
  if (typeof callback !== "function") return promise;
  promise.then(
    (v) => callback(v),
    (e) => {
      api.runtime.lastError = { message: e.message };
      try {
        callback();
      } finally {
        api.runtime.lastError = undefined;
      }
    },
  );
}

// Arguments of an API call, the trailing callback apart.
function split(args) {
  const list = Array.from(args);
  const cb = typeof list[list.length - 1] === "function" ? list.pop() : undefined;
  return [list, cb];
}

function noop(api, value) {
  return (...args) => {
    const [, cb] = split(args);
    return withCallback(Promise.resolve(value), cb, api);
  };
}

function makeChrome(entry) {
  const owner = entry.owner;
  const id = owner.slice(4);
  const page = context !== "content";
  const api = {};
  const call = (op, fields, cb) => withCallback(send(owner, op, fields), cb, api);

  // storage
  const changed = event();
  const area = (name) => ({
    get: (keys, cb) => {
      if (typeof keys === "function") [keys, cb] = [null, keys];
      return call("get", { area: name, keys: keys === undefined ? null : keys }, cb);
    },
    set: (items, cb) => call("set", { area: name, items }, cb),
    remove: (keys, cb) => call("remove", { area: name, keys }, cb),
    clear: (cb) => call("clear", { area: name }, cb),
    getBytesInUse: noop(api, 0),
    setAccessLevel: noop(api, undefined),
    onChanged: event(),
    QUOTA_BYTES: 10485760,
  });
  api.storage = { local: area("local"), sync: area("sync"), session: area("session"), managed: area("managed"), onChanged: changed };

  // i18n
  const messages = entry.messages || {};
  api.i18n = {
    getMessage(name, subs) {
      const key = String(name).toLowerCase();
      if (key === "@@extension_id") return id;
      if (key === "@@ui_locale") return navigator.language.replace("-", "_");
      const m = messages[key];
      if (!m) return "";
      const list = subs === undefined ? [] : Array.isArray(subs) ? subs : [subs];
      let text = m.message || "";
      for (const [ph, def] of Object.entries(m.placeholders || {})) {
        const content = String(def.content || "").replace(/\$(\d)/g, (_, i) => list[i - 1] ?? "");
        text = text.replace(new RegExp("\\$" + ph + "\\$", "gi"), content);
      }
      return text.replace(/\$(\d)/g, (_, i) => list[i - 1] ?? "");
    },
    getUILanguage: () => navigator.language,
    getAcceptLanguages: noop(api, [navigator.language]),
    detectLanguage: noop(api, { isReliable: false, languages: [] }),
  };

  // runtime
  const onMessage = event();

  // Native messaging: ports to programs on the computer, by id once the
  // browser opened them.
  const nativePorts = new Map();
  const withLastError = (message, f) => {
    api.runtime.lastError = { message };
    try {
      f();
    } finally {
      api.runtime.lastError = undefined;
    }
  };
  const connectNative = (app) => {
    let id = null;
    let closed = false;
    const queue = [];
    const port = { name: String(app), sender: undefined, onMessage: event(), onDisconnect: event() };
    const close = (error) => {
      if (closed) return;
      closed = true;
      if (id !== null) nativePorts.delete(id);
      if (error) withLastError(error, () => port.onDisconnect.fire(port));
      else port.onDisconnect.fire(port);
    };
    port.postMessage = (msg) => {
      if (closed) throw new Error("Attempting to use a disconnected port object");
      // As in Chrome: what reaches the host is the message's JSON.
      const value = JSON.parse(JSON.stringify(msg === undefined ? null : msg));
      if (id === null) queue.push(value);
      else send(owner, "native.post", { port: id, msg: value }).catch((e) => close(e.message));
    };
    port.disconnect = () => {
      if (closed) return;
      closed = true;
      if (id !== null) {
        nativePorts.delete(id);
        send(owner, "native.disconnect", { port: id }).catch(() => {});
      }
    };
    port._event = (msg) => {
      if (msg.event === "native") port.onMessage.fire(msg.msg, port);
      else close(msg.error);
    };
    send(owner, "native.connect", { app: String(app) }).then(
      (portId) => {
        if (closed) {
          send(owner, "native.disconnect", { port: portId }).catch(() => {});
          return;
        }
        id = portId;
        nativePorts.set(id, port);
        for (const value of queue.splice(0)) {
          send(owner, "native.post", { port: id, msg: value }).catch((e) => close(e.message));
        }
      },
      (e) => close(e.message),
    );
    return port;
  };
  // Ports between the extension's pages and scripts (runtime.connect,
  // tabs.connect), by id once the browser opened them.
  const ports = new Map();
  const makePort = (name, sender) => {
    let id = null;
    let closed = false;
    const queue = [];
    const port = { name: String(name || ""), sender, onMessage: event(), onDisconnect: event() };
    const post = (value) => send(owner, "port.post", { port: id, msg: value }).catch(() => {});
    port._open = (portId) => {
      if (closed) {
        send(owner, "port.disconnect", { port: portId }).catch(() => {});
        return;
      }
      id = portId;
      ports.set(id, port);
      for (const value of queue.splice(0)) post(value);
    };
    port._close = (error) => {
      if (closed) return;
      closed = true;
      if (id !== null) ports.delete(id);
      if (error) withLastError(error, () => port.onDisconnect.fire(port));
      else port.onDisconnect.fire(port);
    };
    port.postMessage = (msg) => {
      if (closed) throw new Error("Attempting to use a disconnected port object");
      const value = JSON.parse(JSON.stringify(msg === undefined ? null : msg));
      if (id === null) queue.push(value);
      else post(value);
    };
    port.disconnect = () => {
      if (closed) return;
      closed = true;
      if (id !== null) {
        ports.delete(id);
        send(owner, "port.disconnect", { port: id }).catch(() => {});
      }
    };
    return port;
  };
  const openPort = (fields, name) => {
    const port = makePort(name);
    send(owner, "port.connect", Object.assign({ name: String(name || "") }, fields)).then(
      (portId) => port._open(portId),
      (e) => port._close(e.message),
    );
    return port;
  };
  Object.defineProperty(api, "_connectTab", {
    value: (tabId, info) => openPort({ tabId }, info && info.name),
    enumerable: false,
  });

  // One message, its first answer, then the port closes.
  const sendNativeMessage = (app, message, cb) => {
    const answer = new Promise((resolve, reject) => {
      const port = connectNative(app);
      let done = false;
      port.onMessage.addListener((reply) => {
        done = true;
        port.disconnect();
        resolve(reply);
      });
      port.onDisconnect.addListener(() => {
        if (done) return;
        reject(new Error((api.runtime.lastError && api.runtime.lastError.message) || "Native host has exited."));
      });
      port.postMessage(message);
    });
    return withCallback(answer, cb, api);
  };

  api.runtime = {
    id,
    lastError: undefined,
    getURL: (path) => entry.origin + "/" + String(path).replace(/^\//, ""),
    getManifest: () => entry.manifest || {},
    sendMessage: (...args) => {
      const [list, cb] = split(args);
      // (extensionId?, message, options?)
      const payload = list.length >= 2 && typeof list[0] === "string" ? list[1] : list[0];
      return call("sendMessage", { payload }, cb);
    },
    connect: (...args) => {
      // (extensionId?, connectInfo?)
      const info = typeof args[0] === "string" ? args[1] : args[0];
      return openPort({}, info && info.name);
    },
    connectNative,
    sendNativeMessage,
    onMessage,
    onMessageExternal: event(),
    onConnect: event(),
    onConnectExternal: event(),
    onInstalled: event(),
    onStartup: event(),
    onSuspend: event(),
    onUpdateAvailable: event(),
    getPlatformInfo: noop(api, {
      os: /Android/.test(navigator.userAgent) ? "android" : /Mac/.test(navigator.platform) ? "mac" : /Win/.test(navigator.platform) ? "win" : "linux",
      arch: "x86-64",
      nacl_arch: "x86-64",
    }),
    openOptionsPage: (cb) => {
      const m = entry.manifest || {};
      const options = (m.options_ui && m.options_ui.page) || m.options_page;
      return withCallback(options ? send(owner, "tabs.create", { props: { url: api.runtime.getURL(options) } }) : Promise.resolve(), cb, api);
    },
    setUninstallURL: noop(api, undefined),
    reload: () => location.reload(),
    requestUpdateCheck: noop(api, ["no_update"]),
    getContexts: noop(api, []),
  };
  api.extension = {
    getURL: api.runtime.getURL,
    inIncognitoContext: false,
    getBackgroundPage: () => null,
    getViews: () => [],
    isAllowedIncognitoAccess: noop(api, false),
    isAllowedFileSchemeAccess: noop(api, false),
  };

  // Rust's events for this extension.
  onEvents(owner, (msg) => {
    if (msg.event === "message") {
      // Shared by the extension's scripts in this page (see the hook).
      const answer = msg.answer || { left: 1, done: false };
      let async = false;
      const sendResponse = (r) => {
        if (answer.done) return;
        answer.done = true;
        send(owner, "respond", { rid: msg.rid, payload: r === undefined ? null : r }).catch(() => {});
      };
      for (const ret of onMessage.fire(msg.payload, msg.sender || {}, sendResponse)) {
        if (ret === true) async = true;
        else if (ret && typeof ret.then === "function") {
          async = true;
          ret.then(sendResponse, () => sendResponse(undefined));
        }
      }
      if (!async) answer.left -= 1;
      if (!answer.done && answer.left <= 0) {
        answer.done = true;
        send(owner, "respond", { rid: msg.rid, none: true }).catch(() => {});
      }
    } else if (msg.event === "storage") {
      changed.fire(msg.changes, msg.area);
      const a = api.storage[msg.area];
      if (a) a.onChanged.fire(msg.changes);
    } else if (msg.event === "tabs" && api.tabs && api.tabs[msg.name]) {
      api.tabs[msg.name].fire(...msg.args);
    } else if (msg.event === "windows" && api.windows && api.windows[msg.name]) {
      api.windows[msg.name].fire(...msg.args);
    } else if (msg.event === "connect") {
      const port = makePort(msg.name, msg.sender || {});
      port._open(msg.port);
      api.runtime.onConnect.fire(port);
    } else if (msg.event === "port") {
      const port = ports.get(msg.port);
      if (port) port.onMessage.fire(msg.msg, port);
    } else if (msg.event === "portDisconnect") {
      const port = ports.get(msg.port);
      if (port) port._close(msg.error);
    } else if (msg.event === "native" || msg.event === "nativeDisconnect") {
      const port = nativePorts.get(msg.port);
      if (port) port._event(msg);
    }
  });

  if (page) extensionApis(api, entry, call);
  // Content scripts tell the browser which tabs to deliver messages to.
  else send(owner, "hello", {}).catch(() => {});
  return api;
}

// Bytes to base64 and back, for request and response bodies.
function toBase64(bytes) {
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) text += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  return btoa(text);
}
function fromBase64(b64) {
  const text = atob(b64 || "");
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i);
  return bytes;
}

// As in Chrome, an extension's pages reach the hosts of their permissions
// without CORS: those requests go through the browser. Same-origin ones,
// and the ones the browser refuses ("not permitted"), stay the page's own.
function hostFetch(call) {
  if (window.fetch.__buHost) return;
  const pageFetch = window.fetch.bind(window);
  window.fetch = async (input, init) => {
    const request = new Request(input, init);
    const url = new URL(request.url, location.href);
    if (url.origin === location.origin || !/^https?:$/.test(url.protocol)) return pageFetch(input, init);
    const noBody = request.method === "GET" || request.method === "HEAD";
    const body = noBody ? null : toBase64(new Uint8Array(await request.clone().arrayBuffer()));
    let answer;
    try {
      answer = await call("net.fetch", {
        url: url.href,
        method: request.method,
        headers: Array.from(request.headers.entries()),
        body,
      });
    } catch (e) {
      if (e && e.message === "not permitted") return pageFetch(input, init);
      throw new TypeError("Failed to fetch");
    }
    const empty = request.method === "HEAD" || [101, 204, 205, 304].includes(answer.status);
    const response = new Response(empty ? null : fromBase64(answer.body), {
      status: answer.status,
      statusText: answer.statusText,
      headers: answer.headers,
    });
    Object.defineProperty(response, "url", { value: answer.url });
    return response;
  };
  window.fetch.__buHost = true;
}

// What only extension pages (background, popup, options) have.
function extensionApis(api, entry, call) {
  const ev = event;
  const manifest = entry.manifest || {};
  hostFetch(call);
  api.tabs = {
    TAB_ID_NONE: -1,
    query: (query, cb) => call("tabs.query", { query: query || {} }, cb),
    connect: (tabId, info) => api._connectTab(tabId, info),
    get: (tabId, cb) => call("tabs.get", { tabId }, cb),
    getCurrent: noop(api, undefined),
    create: (props, cb) => call("tabs.create", { props: props || {} }, cb),
    update: (...args) => {
      const [list, cb] = split(args);
      const [tabId, props] = typeof list[0] === "number" ? list : [null, list[0]];
      return call("tabs.update", { tabId, props: props || {} }, cb);
    },
    remove: (tabIds, cb) => call("tabs.remove", { tabIds: [].concat(tabIds) }, cb),
    reload: (...args) => {
      const [list, cb] = split(args);
      return call("tabs.reload", { tabId: typeof list[0] === "number" ? list[0] : null }, cb);
    },
    sendMessage: (...args) => {
      const [list, cb] = split(args);
      return call("tabs.sendMessage", { tabId: list[0], payload: list[1] }, cb);
    },
    executeScript: (...args) => {
      const [list, cb] = split(args);
      const [tabId, details] = typeof list[0] === "number" ? list : [null, list[0]];
      return call("scripting.executeScript", { tabId, code: details.code || null, files: details.file ? [details.file] : [] }, cb);
    },
    insertCSS: (...args) => {
      const [list, cb] = split(args);
      const [tabId, details] = typeof list[0] === "number" ? list : [null, list[0]];
      return call("scripting.insertCSS", { tabId, css: details.code || null, files: details.file ? [details.file] : [] }, cb);
    },
    onUpdated: ev(),
    onActivated: ev(),
    onRemoved: ev(),
    onCreated: ev(),
    onReplaced: ev(),
    onHighlighted: ev(),
    onMoved: ev(),
    onAttached: ev(),
    onDetached: ev(),
    onZoomChange: ev(),
  };
  // The browser's window (1), and a window per tab windows.create opened:
  // Rust keeps them, windows.onRemoved fires when such a tab closes.
  const windowInfo = (op, windowId) => (...args) => {
    const [list, cb] = split(args);
    const fields = { info: (windowId ? list[1] : list[0]) || {} };
    if (windowId) fields.windowId = list[0];
    return call(op, fields, cb);
  };
  api.windows = {
    WINDOW_ID_CURRENT: -2,
    WINDOW_ID_NONE: -1,
    getCurrent: windowInfo("windows.getCurrent", false),
    getLastFocused: windowInfo("windows.getLastFocused", false),
    getAll: windowInfo("windows.getAll", false),
    get: windowInfo("windows.get", true),
    create: (...args) => {
      const [list, cb] = split(args);
      return call("windows.create", { data: list[0] || {} }, cb);
    },
    update: windowInfo("windows.update", true),
    remove: (windowId, cb) => call("windows.remove", { windowId }, cb),
    onFocusChanged: ev(),
    onCreated: ev(),
    onRemoved: ev(),
  };
  // Alarms: timers of this page (not kept across restarts).
  const alarms = new Map();
  const onAlarm = ev();
  const describe = (a) => a && { name: a.name, scheduledTime: a.scheduledTime, periodInMinutes: a.periodInMinutes };
  api.alarms = {
    create: (...args) => {
      const [list, cb] = split(args);
      const [name, info] = typeof list[0] === "string" ? [list[0], list[1] || {}] : ["", list[0] || {}];
      const old = alarms.get(name);
      if (old) clearTimeout(old.timer);
      const period = info.periodInMinutes ? info.periodInMinutes * 60000 : null;
      const first = info.when ? Math.max(0, info.when - Date.now()) : (info.delayInMinutes || info.periodInMinutes || 0) * 60000;
      const alarm = { name, scheduledTime: Date.now() + first, periodInMinutes: info.periodInMinutes };
      const fire = () => {
        onAlarm.fire(describe(alarm));
        if (period) {
          alarm.scheduledTime = Date.now() + period;
          alarm.timer = setTimeout(fire, period);
        } else alarms.delete(name);
      };
      alarm.timer = setTimeout(fire, first);
      alarms.set(name, alarm);
      return withCallback(Promise.resolve(), cb, api);
    },
    get: (...args) => {
      const [list, cb] = split(args);
      return withCallback(Promise.resolve(describe(alarms.get(list[0] || ""))), cb, api);
    },
    getAll: (cb) => withCallback(Promise.resolve([...alarms.values()].map(describe)), cb, api),
    clear: (...args) => {
      const [list, cb] = split(args);
      const a = alarms.get(list[0] || "");
      if (a) clearTimeout(a.timer);
      return withCallback(Promise.resolve(alarms.delete(list[0] || "")), cb, api);
    },
    clearAll: (cb) => {
      alarms.forEach((a) => clearTimeout(a.timer));
      alarms.clear();
      return withCallback(Promise.resolve(true), cb, api);
    },
    onAlarm,
  };
  // No toolbar button: the popup opens from the Extensions view.
  const action = {
    setBadgeText: noop(api, undefined),
    getBadgeText: noop(api, ""),
    setBadgeBackgroundColor: noop(api, undefined),
    getBadgeBackgroundColor: noop(api, [0, 0, 0, 0]),
    setBadgeTextColor: noop(api, undefined),
    setIcon: noop(api, undefined),
    setTitle: noop(api, undefined),
    getTitle: noop(api, ""),
    setPopup: noop(api, undefined),
    getPopup: noop(api, ""),
    enable: noop(api, undefined),
    disable: noop(api, undefined),
    isEnabled: noop(api, true),
    openPopup: noop(api, undefined),
    getUserSettings: noop(api, { isOnToolbar: false }),
    onClicked: ev(),
  };
  api.action = action;
  api.browserAction = action;
  api.pageAction = Object.assign({ show: noop(api, undefined), hide: noop(api, undefined) }, action);
  let menuId = 0;
  const menus = {
    create: (props, cb) => {
      if (typeof cb === "function") setTimeout(cb, 0);
      return (props && props.id) || ++menuId;
    },
    update: noop(api, undefined),
    remove: noop(api, undefined),
    removeAll: noop(api, undefined),
    onClicked: ev(),
    ContextType: {},
  };
  api.contextMenus = menus;
  api.menus = menus;
  // What the manifest asks for, plus the optional permissions granted since
  // (kept for all the extension's pages). A permission whose API the
  // browser lacks (`privacy`...) is never granted: an extension checks it
  // before calling that API.
  const GRANTED_KEY = "__bu_granted_permissions";
  // In the extension's own origin only: a content script's localStorage is
  // the visited site's.
  const granted = () => {
    let extra = [];
    try {
      if (context !== "content") extra = JSON.parse(localStorage.getItem(GRANTED_KEY) || "[]");
    } catch (e) {}
    return new Set([...(manifest.permissions || []), ...extra]);
  };
  const saveGranted = (set) => {
    const base = new Set(manifest.permissions || []);
    if (context === "content") return;
    try {
      localStorage.setItem(GRANTED_KEY, JSON.stringify([...set].filter((p) => !base.has(p))));
    } catch (e) {}
  };
  const NAMESPACED = [
    "privacy", "proxy", "history", "bookmarks", "sessions", "tabGroups", "topSites",
    "downloads", "cookies", "management", "notifications", "idle", "webNavigation",
    "contextMenus", "alarms", "scripting", "offscreen", "sidePanel", "declarativeNetRequest",
    "browsingData", "contentSettings", "debugger", "fontSettings", "gcm", "identity",
    "pageCapture", "power", "printerProvider", "search", "system.cpu", "system.memory",
    "system.storage", "tabCapture", "tts", "ttsEngine", "wallpaper", "webRequest",
  ];
  const available = (p) => !NAMESPACED.includes(p) || api[p] !== undefined;
  const wanted = (o) => ((o && o.permissions) || []).map(String);
  api.permissions = {
    contains: (o, cb) => {
      const have = granted();
      const ok = wanted(o).every((p) => have.has(p) && available(p));
      return withCallback(Promise.resolve(ok), cb, api);
    },
    request: (o, cb) => {
      const asked = wanted(o);
      const ok = asked.every(available);
      if (ok) {
        const have = granted();
        asked.forEach((p) => have.add(p));
        saveGranted(have);
      }
      return withCallback(Promise.resolve(ok), cb, api);
    },
    remove: (o, cb) => {
      const have = granted();
      const base = new Set(manifest.permissions || []);
      const removable = wanted(o).filter((p) => !base.has(p));
      removable.forEach((p) => have.delete(p));
      saveGranted(have);
      return withCallback(Promise.resolve(removable.length > 0), cb, api);
    },
    getAll: (cb) =>
      withCallback(
        Promise.resolve({ permissions: [...granted()].filter(available), origins: manifest.host_permissions || [] }),
        cb,
        api,
      ),
    onAdded: ev(),
    onRemoved: ev(),
  };
  api.scripting = {
    executeScript: (details, cb) => {
      const target = details.target || {};
      const code = details.func ? "(" + details.func.toString() + ").apply(null, " + JSON.stringify(details.args || []) + ")" : null;
      return call("scripting.executeScript", { tabId: target.tabId, code, files: details.files || [] }, cb);
    },
    insertCSS: (details, cb) => call("scripting.insertCSS", { tabId: (details.target || {}).tabId, css: details.css || null, files: details.files || [] }, cb),
    removeCSS: noop(api, undefined),
    registerContentScripts: (scripts, cb) => call("scripting.registerContentScripts", { scripts }, cb),
    updateContentScripts: (scripts, cb) => call("scripting.updateContentScripts", { scripts }, cb),
    unregisterContentScripts: (filter, cb) => {
      if (typeof filter === "function") [filter, cb] = [undefined, filter];
      return call("scripting.unregisterContentScripts", { ids: filter && filter.ids ? filter.ids : null }, cb);
    },
    getRegisteredContentScripts: (filter, cb) => {
      if (typeof filter === "function") [filter, cb] = [undefined, filter];
      return call("scripting.getRegisteredContentScripts", { ids: filter && filter.ids ? filter.ids : null }, cb);
    },
    ExecutionWorld: { ISOLATED: "ISOLATED", MAIN: "MAIN" },
  };
  api.declarativeNetRequest = {
    MAX_NUMBER_OF_STATIC_RULESETS: 100,
    MAX_NUMBER_OF_ENABLED_STATIC_RULESETS: 50,
    MAX_NUMBER_OF_DYNAMIC_RULES: 30000,
    MAX_NUMBER_OF_SESSION_RULES: 5000,
    MAX_NUMBER_OF_REGEX_RULES: 1000,
    GUARANTEED_MINIMUM_STATIC_RULES: 30000,
    DYNAMIC_RULESET_ID: "_dynamic",
    SESSION_RULESET_ID: "_session",
    getEnabledRulesets: (cb) => call("dnr.getEnabledRulesets", {}, cb),
    updateEnabledRulesets: (options, cb) => call("dnr.updateEnabledRulesets", { options }, cb),
    getDynamicRules: (filter, cb) => {
      if (typeof filter === "function") [filter, cb] = [undefined, filter];
      return call("dnr.getRules", { kind: "dynamic", ids: filter && filter.ruleIds ? filter.ruleIds : null }, cb);
    },
    updateDynamicRules: (options, cb) => call("dnr.updateRules", { kind: "dynamic", options }, cb),
    getSessionRules: (filter, cb) => {
      if (typeof filter === "function") [filter, cb] = [undefined, filter];
      return call("dnr.getRules", { kind: "session", ids: filter && filter.ruleIds ? filter.ruleIds : null }, cb);
    },
    updateSessionRules: (options, cb) => call("dnr.updateRules", { kind: "session", options }, cb),
    getAvailableStaticRuleCount: noop(api, 30000),
    isRegexSupported: noop(api, { isSupported: false, reason: "syntaxError" }),
    setExtensionActionOptions: noop(api, undefined),
    getMatchedRules: noop(api, { rulesMatchedInfo: [] }),
    testMatchOutcome: noop(api, { matchedRules: [] }),
    onRuleMatchedDebug: ev(),
  };
  api.webNavigation = {
    getAllFrames: noop(api, []),
    getFrame: noop(api, null),
    onBeforeNavigate: ev(),
    onCommitted: ev(),
    onDOMContentLoaded: ev(),
    onCompleted: ev(),
    onErrorOccurred: ev(),
    onHistoryStateUpdated: ev(),
    onReferenceFragmentUpdated: ev(),
    onCreatedNavigationTarget: ev(),
  };
  api.notifications = {
    create: (...args) => {
      const [list, cb] = split(args);
      const opts = typeof list[0] === "string" ? list[1] : list[0];
      console.info("[notification]", opts && opts.title, opts && opts.message);
      return withCallback(Promise.resolve(typeof list[0] === "string" ? list[0] : "n" + Date.now()), cb, api);
    },
    clear: noop(api, true),
    getAll: noop(api, {}),
    onClicked: ev(),
    onClosed: ev(),
    onButtonClicked: ev(),
  };
  api.commands = { getAll: noop(api, []), onCommand: ev() };
  api.offscreen = { createDocument: noop(api, undefined), closeDocument: noop(api, undefined), hasDocument: noop(api, false), Reason: {} };
  api.userScripts = { register: noop(api, undefined), unregister: noop(api, undefined), getScripts: noop(api, []), update: noop(api, undefined), configureWorld: noop(api, undefined) };
  api.sidePanel = { setOptions: noop(api, undefined), setPanelBehavior: noop(api, undefined), open: noop(api, undefined) };
  api.idle = { queryState: noop(api, "active"), setDetectionInterval: () => {}, onStateChanged: ev() };
  api.management = { getSelf: noop(api, { id: api.runtime.id, name: manifest.name, enabled: true, installType: "normal" }) };
  api.downloads = { download: (o, cb) => call("tabs.create", { props: { url: o.url } }, cb) };
  api.cookies = { get: noop(api, null), getAll: noop(api, []), set: noop(api, null), remove: noop(api, null), onChanged: ev() };
  api.webRequest = {
    onBeforeRequest: ev(),
    onBeforeSendHeaders: ev(),
    onSendHeaders: ev(),
    onHeadersReceived: ev(),
    onResponseStarted: ev(),
    onBeforeRedirect: ev(),
    onCompleted: ev(),
    onErrorOccurred: ev(),
    onAuthRequired: ev(),
  };
}

// --- extension pages ---------------------------------------------------------------

function extensionPage(entry) {
  context = entry.context;
  // Extensions tell their own pages by the scheme, Chrome's
  // chrome-extension: (Bitwarden's inline menu checks it): here they are
  // served over https, from the extension's origin.
  const protocol = Object.getOwnPropertyDescriptor(URL.prototype, "protocol");
  if (protocol && protocol.get) {
    Object.defineProperty(URL.prototype, "protocol", {
      configurable: true,
      enumerable: protocol.enumerable,
      get() {
        const p = protocol.get.call(this);
        return p === "https:" && this.origin === entry.origin ? "chrome-extension:" : p;
      },
      set: protocol.set,
    });
  }
  const api = makeChrome(entry);
  window.chrome = Object.assign(window.chrome || {}, api);
  window.browser = window.chrome;
  if (context === "background") {
    // Service worker globals, for MV3 workers run as a page.
    window.importScripts = (...urls) => {
      for (const url of urls) {
        const absolute = new URL(url, location.href).href;
        const xhr = new XMLHttpRequest();
        xhr.open("GET", absolute, false);
        xhr.send();
        // As in a worker: a missing script is an error, not code to run.
        if (xhr.status < 200 || xhr.status >= 300) {
          throw new DOMException(`Failed to load ${absolute} (${xhr.status})`, "NetworkError");
        }
        (0, eval)(xhr.responseText + "\n//# sourceURL=" + absolute);
      }
    };
    window.clients = {
      matchAll: () => Promise.resolve([]),
      claim: () => Promise.resolve(),
      get: () => Promise.resolve(undefined),
      openWindow: (url) => api.tabs.create({ url }),
    };
    window.registration = { scope: entry.origin + "/", active: null, unregister: () => Promise.resolve(false), showNotification: () => Promise.resolve() };
    window.skipWaiting = () => Promise.resolve();
  }
  send(entry.owner, "hello", {}).catch(() => {});
  if (context === "page") {
    // A click outside the popup closes it, as in other browsers (only the
    // popup's blur counts, on the browser side).
    window.addEventListener("blur", () =>
      setTimeout(() => {
        if (!document.hasFocus()) send(entry.owner, "blur", {}).catch(() => {});
      }, 50),
    );
  }
  if (context === "background") {
    // The messages the pages sent before the page could take them come
    // once it listens to them: its listeners may come in several times,
    // after an asynchronous start (Bitwarden's), so a moment after the last
    // one; a while after it loaded when it adds none.
    let ready = false;
    let readyTimer = null;
    const signalReady = () => {
      if (ready) return;
      ready = true;
      send(entry.owner, "ready", {}).catch(() => {});
    };
    const readySoon = (delay) => {
      clearTimeout(readyTimer);
      readyTimer = setTimeout(signalReady, delay);
    };
    const listen = api.runtime.onMessage.addListener;
    api.runtime.onMessage.addListener = (f) => {
      listen(f);
      readySoon(500);
    };
    // Once the extension's scripts ran: installed or started.
    window.addEventListener("load", () =>
      setTimeout(() => {
        if (entry.installReason) api.runtime.onInstalled.fire({ reason: entry.installReason });
        else api.runtime.onStartup.fire();
        if (readyTimer === null) readySoon(5000);
      }, 0),
    );
  }
}

// --- GM_* / GM.* for user scripts ----------------------------------------------

function makeGM(entry, values) {
  const owner = entry.owner;
  const save = (items) => send(owner, "set", { items }).catch((e) => console.error(e));
  const info = {
    script: entry.info,
    scriptHandler: "Browsing Unicorns",
    version: entry.handlerVersion,
  };
  const getValue = (k, d) => (Object.prototype.hasOwnProperty.call(values, k) ? values[k] : d);
  const setValue = (k, v) => { values[k] = v; save({ [k]: v }); };
  const deleteValue = (k) => { delete values[k]; send(owner, "remove", { keys: [k] }).catch(() => {}); };
  const listValues = () => Object.keys(values);
  const xhr = (d) => {
    const ctrl = new AbortController();
    fetch(d.url, { method: d.method || "GET", headers: d.headers, body: d.data, signal: ctrl.signal, credentials: d.anonymous ? "omit" : "include" })
      .then(async (r) => {
        const text = await r.text();
        const response = { status: r.status, statusText: r.statusText, responseText: text, response: text, finalUrl: r.url, readyState: 4, responseHeaders: [...r.headers].map(([k, v]) => k + ": " + v).join("\r\n") };
        d.onload && d.onload(response);
        d.onloadend && d.onloadend(response);
      })
      .catch((e) => { d.onerror && d.onerror({ error: String(e) }); d.onloadend && d.onloadend({ error: String(e) }); });
    return { abort: () => ctrl.abort() };
  };
  const g = {
    GM_info: info,
    GM_addStyle: addStyle,
    GM_getValue: getValue,
    GM_setValue: setValue,
    GM_deleteValue: deleteValue,
    GM_listValues: listValues,
    GM_xmlhttpRequest: xhr,
    GM_openInTab: (url) => window.open(url, "_blank"),
    GM_setClipboard: (text) => navigator.clipboard && navigator.clipboard.writeText(String(text)),
    GM_registerMenuCommand: () => 0,
    GM_unregisterMenuCommand: () => {},
    GM_notification: (d) => console.info("[GM_notification]", typeof d === "string" ? d : d && d.text),
    GM_log: (...a) => console.log(...a),
  };
  g.GM = {
    info,
    addStyle: (css) => Promise.resolve(addStyle(css)),
    getValue: (k, d) => Promise.resolve(getValue(k, d)),
    setValue: (k, v) => { setValue(k, v); return Promise.resolve(); },
    deleteValue: (k) => { deleteValue(k); return Promise.resolve(); },
    listValues: () => Promise.resolve(listValues()),
    xmlHttpRequest: xhr,
    openInTab: g.GM_openInTab,
    setClipboard: (t) => Promise.resolve(g.GM_setClipboard(t)),
    registerMenuCommand: g.GM_registerMenuCommand,
    notification: g.GM_notification,
  };
  return g;
}

// --- entries -----------------------------------------------------------------------

// Chrome runs extension scripts in an isolated world, where setting a
// field's `value` goes straight to the element. Here they share the page's
// world, where a framework (React) may have put its own `value` on the
// element to tell the user's typing from its own updates: a value an
// extension sets (a password manager filling a form) would pass for the
// framework's own, and the input event that follows would change nothing.
// So a value set from an extension's script (marked by its sourceURL) skips
// what the page put on the element.
const EXTENSION_SCRIPT = "bu-extension-script";

function isolateFieldValues() {
  if (Object.getOwnPropertyDescriptor(window, "__buFieldValues")) return;
  Object.defineProperty(window, "__buFieldValues", { value: true });
  const define = Object.defineProperty;
  const fields = [window.HTMLInputElement, window.HTMLTextAreaElement, window.HTMLSelectElement];
  const nativeOf = (el, prop) => {
    for (let p = Object.getPrototypeOf(el); p; p = Object.getPrototypeOf(p)) {
      const d = Object.getOwnPropertyDescriptor(p, prop);
      if (d) return d;
    }
    return undefined;
  };
  const fromExtension = () => {
    const limit = Error.stackTraceLimit;
    Error.stackTraceLimit = 4;
    const stack = new Error().stack || "";
    Error.stackTraceLimit = limit;
    // This function, the setter, then whoever set the value.
    return (stack.split("\n")[3] || "").includes(EXTENSION_SCRIPT);
  };
  const wrapped = function defineProperty(target, prop, desc) {
    if (
      (prop === "value" || prop === "checked") &&
      desc &&
      typeof desc.set === "function" &&
      fields.some((F) => F && target instanceof F)
    ) {
      const native = nativeOf(target, prop);
      const pageSet = desc.set;
      if (native && native.set) {
        desc = Object.assign({}, desc, {
          set(v) {
            if (fromExtension()) native.set.call(this, v);
            else pageSet.call(this, v);
          },
        });
      }
    }
    return define(target, prop, desc);
  };
  Object.defineProperty(wrapped, "toString", { value: () => define.toString() });
  Object.defineProperty = wrapped;
}

function contentScript(entry, code) {
  if (!entry.allFrames && !isTop) return;
  if (!matches(entry, location.href)) return;
  // Style sheets go in first, whatever the scripts' timing.
  for (const css of entry.css) addStyle(css);
  if (!code) return;
  isolateFieldValues();
  const api = makeChrome(entry);
  at(entry.runAt, () => code.call(window, api, api));
}

// A script an extension runs in the tab (`scripting.executeScript`): like
// its content scripts, it gets the extension's `chrome.*`; its result is
// returned.
function injected(entry, fn) {
  isolateFieldValues();
  const api = makeChrome(entry);
  return fn.call(window, api, api);
}

// The built-in ad blocker's site-specific cosmetic filters: the browser
// gives the style sheet hiding this site's ad slots (empty when the blocker
// is off here). Top frame only.
function blockerCosmetic() {
  if (!isTop) return;
  send("blocker", "blocker.cosmetic", { url: location.href }).then(
    (css) => {
      if (css) addStyle(css);
    },
    () => {},
  );
}

function userScript(entry, code) {
  if (entry.noFrames && !isTop) return;
  if (!matches(entry, location.href)) return;
  const usesValues = entry.grants.some((g) => /value/i.test(g));
  const start = (values) => {
    const g = makeGM(entry, values);
    at(entry.runAt, () =>
      code.call(window, g.GM, g.GM_info, g.GM_addStyle, g.GM_getValue, g.GM_setValue, g.GM_deleteValue, g.GM_listValues, g.GM_xmlhttpRequest, g.GM_openInTab, g.GM_setClipboard, g.GM_registerMenuCommand, g.GM_unregisterMenuCommand, g.GM_notification, g.GM_log, window),
    );
  };
  // Values are read once, before the script runs (GM_getValue is synchronous).
  if (usesValues) send(entry.owner, "get", { keys: null }).then(start, () => start({}));
  else start({});
}
