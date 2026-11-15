// Runtime of the scripts Browsing Unicorns runs in pages (extensions'
// content scripts, user scripts). Each script gets its own copy, wrapped in a
// function that defines TOKEN, CHANNEL (random, per script) and BRIDGE:
// nothing here is visible to the page but the reply hook, which it cannot
// replace and which only answers the channels the scripts registered.

const pending = new Map();
let nextId = 1;

function deliver(reply) {
  const p = pending.get(reply.id);
  if (!p) return;
  pending.delete(reply.id);
  if (reply.error !== undefined) p.reject(new Error(reply.error));
  else p.resolve(reply.result);
}

let post;
if (BRIDGE === "android") {
  const bridge = window.BUBridge;
  post = (msg) => {
    try {
      const reply = bridge ? JSON.parse(bridge.call(JSON.stringify(msg))) : null;
      if (reply) deliver(reply);
      else deliver({ id: msg.id, error: "no bridge" });
    } catch (e) {
      deliver({ id: msg.id, error: String(e) });
    }
  };
} else {
  const ipc = window.ipc;
  const postMessage = ipc && ipc.postMessage ? ipc.postMessage.bind(ipc) : null;
  if (!Object.getOwnPropertyDescriptor(window, "__buReply")) {
    const channels = new Map();
    const hook = (reply) => {
      const to = channels.get(reply && reply.ch);
      if (to) to(reply);
    };
    Object.defineProperty(hook, "register", {
      value: (ch, f) => { if (!channels.has(ch)) channels.set(ch, f); },
    });
    Object.defineProperty(window, "__buReply", {
      value: hook,
      writable: false,
      configurable: false,
      enumerable: false,
    });
  }
  window.__buReply.register(CHANNEL, deliver);
  post = (msg) => {
    if (postMessage) postMessage(JSON.stringify(msg));
    else deliver({ id: msg.id, error: "no bridge" });
  };
}

function send(owner, op, fields) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    post(Object.assign({ t: TOKEN, ch: CHANNEL, id, owner, op }, fields));
  });
}

// --- when and where ----------------------------------------------------------

const isTop = window === window.top;

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
    fire: (...args) => listeners.forEach((f) => { try { f(...args); } catch (e) { console.error(e); } }),
  };
}

// --- chrome.* for content scripts ---------------------------------------------

function withCallback(promise, callback, api) {
  if (typeof callback !== "function") return promise;
  promise.then(
    (v) => callback(v),
    (e) => { api.runtime.lastError = { message: e.message }; callback(); api.runtime.lastError = undefined; },
  );
}

function makeChrome(entry) {
  const owner = entry.owner;
  const api = {};
  const changed = event();
  const area = (name) => ({
    get: (keys, cb) => withCallback(send(owner, "get", { area: name, keys: keys === undefined ? null : keys }), cb, api),
    set: (items, cb) =>
      withCallback(
        send(owner, "set", { area: name, items }).then(() => {
          const changes = {};
          for (const k of Object.keys(items)) changes[k] = { newValue: items[k] };
          changed.fire(changes, name);
        }),
        cb,
        api,
      ),
    remove: (keys, cb) => withCallback(send(owner, "remove", { area: name, keys }), cb, api),
    clear: (cb) => withCallback(send(owner, "clear", { area: name }), cb, api),
    getBytesInUse: (_keys, cb) => withCallback(Promise.resolve(0), cb, api),
    onChanged: event(),
  });
  api.storage = { local: area("local"), sync: area("sync"), session: area("session"), onChanged: changed };
  const messages = entry.messages || {};
  api.i18n = {
    getMessage(name, subs) {
      const m = messages[String(name).toLowerCase()];
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
    detectLanguage: (_t, cb) => withCallback(Promise.resolve({ languages: [] }), cb, api),
  };
  const id = owner.slice(4);
  api.runtime = {
    id,
    lastError: undefined,
    getURL: (path) => "chrome-extension://" + id + "/" + String(path).replace(/^\//, ""),
    getManifest: () => entry.manifest || {},
    // No background page yet: messages get no answer.
    sendMessage: (...args) => {
      const cb = args.find((a) => typeof a === "function");
      return withCallback(Promise.resolve(undefined), cb, api);
    },
    connect: () => ({ postMessage() {}, disconnect() {}, onMessage: event(), onDisconnect: event(), name: "" }),
    onMessage: event(),
    onConnect: event(),
  };
  api.extension = { getURL: api.runtime.getURL, inIncognitoContext: false };
  return api;
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

function contentScript(entry, code) {
  if (!entry.allFrames && !isTop) return;
  if (!matches(entry, location.href)) return;
  // Style sheets go in first, whatever the scripts' timing.
  for (const css of entry.css) addStyle(css);
  if (!code) return;
  const api = makeChrome(entry);
  at(entry.runAt, () => code.call(window, api, api));
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

// A pattern from the bundle; an invalid one matches nothing.
function re(source, flags) {
  try {
    return new RegExp(source, flags);
  } catch (e) {
    console.warn("[Browsing Unicorns] bad pattern", source, e);
    return /$^/;
  }
}
