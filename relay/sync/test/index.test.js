import { test } from "node:test";
import assert from "node:assert/strict";
import worker from "../src/index.js";
import { LIMITS, sha256 } from "../src/lib.js";

/// Workers KV in memory (get, put with metadata, delete, list by prefix).
class FakeKV {
  constructor() {
    this.items = new Map();
  }
  async get(key) {
    this.reads = (this.reads ?? 0) + 1;
    return this.items.has(key) ? this.items.get(key).value : null;
  }
  async getWithMetadata(key) {
    this.reads = (this.reads ?? 0) + 1;
    const item = this.items.get(key);
    return item ? { value: item.value, metadata: item.metadata ?? null } : { value: null, metadata: null };
  }
  async put(key, value, options = {}) {
    this.writes = (this.writes ?? 0) + 1;
    this.items.set(key, { value, metadata: options.metadata, ttl: options.expirationTtl });
  }
  async delete(key) {
    this.items.delete(key);
  }
  async list({ prefix }) {
    this.lists = (this.lists ?? 0) + 1;
    const keys = [...this.items.entries()]
      .filter(([k]) => k.startsWith(prefix))
      .map(([name, v]) => ({ name, metadata: v.metadata }));
    return { keys, list_complete: true };
  }
}

const GROUP = "0123456789abcdef0123456789abcdef";
const TOKEN = "a".repeat(64);
const OTHER = "b".repeat(64);

function setup() {
  const env = { SYNC: new FakeKV() };
  const call = (method, path, { token = TOKEN, body, duplex } = {}) =>
    worker.fetch(
      new Request(`https://relay.test${path}`, {
        method,
        body,
        duplex,
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      }),
      env,
    );
  return { env, call };
}

const put = (call, device, bytes, opts) =>
  call("PUT", `/v1/groups/${GROUP}/devices/${device}`, { ...opts, body: new Uint8Array(bytes) });

test("devices store blobs and read them all back", async () => {
  const { call } = setup();
  assert.equal((await put(call, "desktop", [1, 2, 3])).status, 200);
  assert.equal((await put(call, "phone", [4, 5])).status, 200);
  const res = await call("GET", `/v1/groups/${GROUP}`);
  assert.equal(res.status, 200);
  const { devices } = await res.json();
  const byId = Object.fromEntries(devices.map((d) => [d.id, d]));
  assert.deepEqual([...Buffer.from(byId.desktop.data, "base64")], [1, 2, 3]);
  assert.deepEqual([...Buffer.from(byId.phone.data, "base64")], [4, 5]);
  assert.ok(byId.desktop.updated > 0);
});

test("an unknown group is empty", async () => {
  const { call } = setup();
  const res = await call("GET", `/v1/groups/${GROUP}`);
  assert.deepEqual(await res.json(), { devices: [] });
});

test("the first writer's token guards the group", async () => {
  const { env, call } = setup();
  await put(call, "desktop", [1]);
  assert.equal(await env.SYNC.get(`auth:${GROUP}`), await sha256(TOKEN), "only the hash is kept");
  assert.equal((await put(call, "intruder", [9], { token: OTHER })).status, 403);
  assert.equal((await call("GET", `/v1/groups/${GROUP}`, { token: OTHER })).status, 403);
  assert.equal((await call("DELETE", `/v1/groups/${GROUP}/devices/desktop`, { token: OTHER })).status, 403);
  assert.equal((await call("GET", `/v1/groups/${GROUP}`, { token: null })).status, 401);
  assert.equal((await call("GET", `/v1/groups/${GROUP}`, { token: "short" })).status, 401);
});

test("a device leaves the group", async () => {
  const { call } = setup();
  await put(call, "desktop", [1]);
  await put(call, "phone", [2]);
  assert.equal((await call("DELETE", `/v1/groups/${GROUP}/devices/phone`)).status, 200);
  const { devices } = await (await call("GET", `/v1/groups/${GROUP}`)).json();
  assert.deepEqual(devices.map((d) => d.id), ["desktop"]);
});

test("limits on size, emptiness and device count", async () => {
  const { call } = setup();
  assert.equal((await put(call, "big", new Uint8Array(LIMITS.blobBytes + 1))).status, 413);
  assert.equal((await put(call, "empty", [])).status, 400);
  for (let i = 0; i < LIMITS.devicesPerGroup; i++) {
    assert.equal((await put(call, `d${i}`, [i])).status, 200);
  }
  assert.equal((await put(call, "one-too-many", [1])).status, 409);
  assert.equal((await put(call, "d0", [7])).status, 200, "an existing device can still update");
});

test("records expire unless refreshed", async () => {
  const { env, call } = setup();
  await put(call, "desktop", [1]);
  assert.ok(env.SYNC.items.get(`blob:${GROUP}:desktop`).ttl > 0);
  assert.ok(env.SYNC.items.get(`auth:${GROUP}`).ttl > 0);
});

test("a known device's write lists nothing and leaves the token alone", async () => {
  const { env, call } = setup();
  await put(call, "desktop", [1]);
  const [lists, writes] = [env.SYNC.lists, env.SYNC.writes];
  assert.equal(writes, 2, "the blob and the token's hash, the first time");
  assert.equal((await put(call, "desktop", [2])).status, 200);
  assert.equal(env.SYNC.lists, lists, "no list");
  assert.equal(env.SYNC.writes, writes + 1, "the blob only");
});

test("the token's hash is rewritten once half its life is gone", async () => {
  const { env, call } = setup();
  await put(call, "desktop", [1]);
  const auth = env.SYNC.items.get(`auth:${GROUP}`);
  // Written by an older relay: no expiry known.
  auth.metadata = undefined;
  await put(call, "desktop", [2]);
  const now = Math.floor(Date.now() / 1000);
  assert.ok(env.SYNC.items.get(`auth:${GROUP}`).metadata.expires > now);
  env.SYNC.items.get(`auth:${GROUP}`).metadata.expires = now + 1000;
  const writes = env.SYNC.writes;
  await put(call, "desktop", [3]);
  assert.equal(env.SYNC.writes, writes + 2, "refreshed");
});

test("a body without length is refused as soon as it is too large", async () => {
  const { call } = setup();
  let pulled = 0;
  const chunk = new Uint8Array(64 * 1024);
  const body = new ReadableStream({
    pull(controller) {
      pulled += chunk.length;
      controller.enqueue(chunk);
    },
  });
  const res = await call("PUT", `/v1/groups/${GROUP}/devices/desktop`, { body, duplex: "half" });
  assert.equal(res.status, 413);
  assert.ok(pulled < LIMITS.blobBytes * 2, "not read to the end");
  const small = new ReadableStream({
    start(controller) {
      controller.enqueue(new Uint8Array([1, 2]));
      controller.close();
    },
  });
  assert.equal((await call("PUT", `/v1/groups/${GROUP}/devices/desktop`, { body: small, duplex: "half" })).status, 200);
});

test("bad paths, ids and methods are refused", async () => {
  const { call } = setup();
  assert.equal((await call("GET", "/")).status, 404);
  assert.equal((await call("GET", "/v1/groups/NOTHEX")).status, 400);
  assert.equal((await call("PUT", `/v1/groups/${GROUP}/devices/bad%2Fid`, { body: "x" })).status, 400);
  assert.equal((await call("POST", `/v1/groups/${GROUP}`)).status, 405);
  assert.equal((await call("POST", `/v1/groups/${GROUP}/devices/x`, { body: "x" })).status, 405);
});

test("rate limited clients are refused", async () => {
  const env = { SYNC: new FakeKV(), RATE_LIMITER: { limit: async () => ({ success: false }) } };
  const req = new Request(`https://relay.test/v1/groups/${GROUP}`, { headers: { Authorization: `Bearer ${TOKEN}` } });
  assert.equal((await worker.fetch(req, env)).status, 429);
});
