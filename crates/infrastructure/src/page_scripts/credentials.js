// The browser's password manager in pages: tells the browser what a sign-in
// form sent (it offers to save it), and fills such forms in. Top frame only.
// Prepended with `const BRIDGE = "ipc" | "android"; const TOKEN = "…";`
// (capture) or run with the sign-in to fill (see credentials.rs).

const USER_FIELD =
  'input[autocomplete~="username"], input[type="email"], input[name*="user" i], ' +
  'input[name*="login" i], input[name*="email" i], input[id*="user" i], input[id*="login" i], ' +
  'input[id*="email" i]';
const TEXT_TYPES = ["", "text", "email", "tel"];

function isVisible(el) {
  return !!el && !el.disabled && el.type !== "hidden" && el.getClientRects().length > 0;
}

function isText(el) {
  return TEXT_TYPES.includes((el.getAttribute("type") || "").toLowerCase());
}

function passwordsIn(scope) {
  return Array.from(scope.querySelectorAll('input[type="password"]'));
}

// The user name field of `password`: the last text field before it, else
// one that looks like it.
function userFieldOf(scope, password, filled) {
  const inputs = Array.from(scope.querySelectorAll("input"));
  const at = password ? inputs.indexOf(password) : inputs.length;
  for (let i = at - 1; i >= 0; i--) {
    const el = inputs[i];
    if (isText(el) && (filled ? el.value : isVisible(el))) return el;
  }
  const named = Array.from(scope.querySelectorAll(USER_FIELD)).find((el) =>
    filled ? el.value : isVisible(el),
  );
  return named || null;
}

function isNewPassword(el) {
  return /new-password/.test(el.getAttribute("autocomplete") || "");
}

// Top frame only: a frame (an ad's) never gets to TOKEN, so it cannot offer
// to save a password for the site around it. What sending uses is taken
// now, before the page's scripts run, so that the page never sees TOKEN go
// by either.
function captureSignIns() {
  if (window.top !== window || window.__buSignIns) return;
  Object.defineProperty(window, "__buSignIns", { value: true });
  const stringify = JSON.stringify;
  const createObject = Object.create;
  let post = null;
  if (BRIDGE === "android") {
    const bridge = window.BUBridge;
    if (bridge && bridge.call) post = bridge.call.bind(bridge);
  } else {
    // The web view's own channel (wry's `ipc.postMessage` looks it up at
    // each call, after the page could replace it).
    const webview = window.chrome && window.chrome.webview;
    const handlers = window.webkit && window.webkit.messageHandlers;
    const webkit = handlers && handlers.ipc;
    const ipc = window.ipc;
    if (webview && webview.postMessage) post = webview.postMessage.bind(webview);
    else if (webkit && webkit.postMessage) post = webkit.postMessage.bind(webkit);
    else if (ipc && ipc.postMessage) post = ipc.postMessage.bind(ipc);
  }
  if (!post) return;
  let last = "";
  const send = (username, password) => {
    const key = username + "\n" + password;
    if (key === last) return;
    last = key;
    // No prototype: no `toJSON` nor setter of the page's sees it.
    const msg = createObject(null);
    msg.t = TOKEN;
    msg.op = "credentials.submit";
    msg.username = username;
    msg.password = password;
    try {
      post(stringify(msg));
    } catch (e) {}
  };
  const grab = (scope) => {
    const filled = passwordsIn(scope).filter((p) => p.value);
    if (filled.length === 0) {
      // The first step of a two-step sign-in: the user name alone.
      if (passwordsIn(scope).length > 0) return;
      const user = Array.from(scope.querySelectorAll(USER_FIELD)).find(
        (el) => el.value && isText(el),
      );
      if (user) send(user.value, "");
      return;
    }
    const password = filled.find(isNewPassword) || filled[filled.length - 1];
    const user = userFieldOf(scope, filled[0], true);
    send(user ? user.value : "", password.value);
  };
  const scopeOf = (el) => (el && (el.form || (el.closest && el.closest("form")))) || document;
  document.addEventListener(
    "submit",
    (e) => grab(e.target instanceof HTMLFormElement ? e.target : document),
    true,
  );
  document.addEventListener(
    "click",
    (e) => {
      const target = e.target && e.target.closest ? e.target : null;
      const button =
        target &&
        target.closest('button, input[type="submit"], input[type="button"], [role="button"]');
      if (button) grab(scopeOf(button));
    },
    true,
  );
  document.addEventListener(
    "keydown",
    (e) => {
      if (e.key === "Enter" && e.target instanceof HTMLInputElement) grab(scopeOf(e.target));
    },
    true,
  );
}

function fillSignIn(origin, username, password) {
  if (window.top !== window || location.origin !== origin) return;
  const valueSetter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  const set = (el, value) => {
    // Through the prototype's setter, so that frameworks (React) see it.
    valueSetter.call(el, value);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const fill = () => {
    let filled = false;
    for (const field of passwordsIn(document)) {
      if (!isVisible(field) || field.value || isNewPassword(field)) continue;
      const scope = field.form || document;
      // A sign-up or password change form: not filled.
      if (passwordsIn(scope).length > 1) continue;
      set(field, password);
      const user = username ? userFieldOf(scope, field, false) : null;
      if (user && !user.value) set(user, username);
      filled = true;
    }
    if (!filled && username && passwordsIn(document).length === 0) {
      const user = Array.from(document.querySelectorAll(USER_FIELD)).find(
        (el) => isVisible(el) && isText(el) && !el.value,
      );
      if (user) set(user, username);
    }
    return filled;
  };
  if (fill()) return;
  // Forms drawn later (single page apps): watched for a while.
  const observer = new MutationObserver(() => {
    if (fill()) observer.disconnect();
  });
  observer.observe(document.documentElement, { childList: true, subtree: true });
  setTimeout(() => observer.disconnect(), 15000);
}
