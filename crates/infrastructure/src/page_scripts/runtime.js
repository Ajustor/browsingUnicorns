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
      for (const l of listeners) if (l.owner === msg.owner) l.f(msg);
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
    try {
      const text = bridge ? bridge.call(JSON.stringify(msg)) : "";
      if (text) deliver(JSON.parse(text));
      else deliver({ id: msg.id, error: "no bridge" });
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
    connect: () => ({ name: "", postMessage() {}, disconnect() {}, onMessage: event(), onDisconnect: event() }),
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
      let responded = false;
      let async = false;
      const sendResponse = (r) => {
        if (responded) return;
        responded = true;
        send(owner, "respond", { rid: msg.rid, payload: r === undefined ? null : r }).catch(() => {});
      };
      for (const ret of onMessage.fire(msg.payload, msg.sender || {}, sendResponse)) {
        if (ret === true) async = true;
        else if (ret && typeof ret.then === "function") {
          async = true;
          ret.then(sendResponse, () => sendResponse(undefined));
        }
      }
      if (!responded && !async) send(owner, "respond", { rid: msg.rid, none: true }).catch(() => {});
    } else if (msg.event === "storage") {
      changed.fire(msg.changes, msg.area);
      const a = api.storage[msg.area];
      if (a) a.onChanged.fire(msg.changes);
    } else if (msg.event === "tabs" && api.tabs && api.tabs[msg.name]) {
      api.tabs[msg.name].fire(...msg.args);
    }
  });

  if (page) extensionApis(api, entry, call);
  // Content scripts tell the browser which tabs to deliver messages to.
  else send(owner, "hello", {}).catch(() => {});
  return api;
}

// What only extension pages (background, popup, options) have.
function extensionApis(api, entry, call) {
  const ev = event;
  const manifest = entry.manifest || {};
  api.tabs = {
    TAB_ID_NONE: -1,
    query: (query, cb) => call("tabs.query", { query: query || {} }, cb),
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
  const theWindow = { id: 1, focused: true, type: "normal", state: "normal", incognito: false };
  api.windows = {
    WINDOW_ID_CURRENT: -2,
    WINDOW_ID_NONE: -1,
    getCurrent: noop(api, theWindow),
    getLastFocused: noop(api, theWindow),
    getAll: noop(api, [theWindow]),
    get: noop(api, theWindow),
    create: (data, cb) => call("tabs.create", { props: { url: data && [].concat(data.url || [])[0] } }, cb),
    update: noop(api, theWindow),
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
  api.permissions = {
    contains: noop(api, true),
    request: noop(api, true),
    remove: noop(api, true),
    getAll: noop(api, { permissions: manifest.permissions || [], origins: manifest.host_permissions || [] }),
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
  const api = makeChrome(entry);
  window.chrome = Object.assign(window.chrome || {}, api);
  window.browser = window.chrome;
  if (context === "background") {
    // Service worker globals, for MV3 workers run as a page.
    window.importScripts = (...urls) => {
      for (const url of urls) {
        const xhr = new XMLHttpRequest();
        xhr.open("GET", url, false);
        xhr.send();
        (0, eval)(xhr.responseText + "\n//# sourceURL=" + url);
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
  if (context === "background") {
    // Once the extension's scripts ran: installed or started.
    window.addEventListener("load", () =>
      setTimeout(() => {
        if (entry.installReason) api.runtime.onInstalled.fire({ reason: entry.installReason });
        else api.runtime.onStartup.fire();
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
