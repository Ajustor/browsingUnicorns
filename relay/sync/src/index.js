// Sync relay for browsingUnicorns. It stores one encrypted blob per device of
// a sync group and hands them back to the group's devices. The blobs are
// sealed on the devices (XChaCha20-Poly1305, key never sent): the relay
// cannot read them.
//
//   PUT    /v1/groups/:group/devices/:device   body: the blob (octets)
//   GET    /v1/groups/:group                   → {"devices":[{"id","data","updated"}]}
//   DELETE /v1/groups/:group/devices/:device
//
// Every request carries `Authorization: Bearer <token>`. The token is derived
// from the group's key on the devices; the first write to a group records its
// SHA-256, later requests must match it.
//
// Bindings (see wrangler.toml and README.md):
//   SYNC          KV namespace
//   RATE_LIMITER  optional rate limiting binding

import { LIMITS, TTL_SECONDS, sha256, toBase64 } from "./lib.js";

// Only the handler is exported: the Workers runtime takes every named export
// of the main module for an entry point.

const GROUP = /^[0-9a-f]{32}$/;
const DEVICE = /^[A-Za-z0-9_-]{1,64}$/;
const TOKEN = /^[0-9a-f]{64}$/;

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    const parts = url.pathname.split("/").filter(Boolean);
    // ["v1", "groups", group] or ["v1", "groups", group, "devices", device]
    if (parts[0] !== "v1" || parts[1] !== "groups" || !(parts.length === 3 || (parts.length === 5 && parts[3] === "devices"))) {
      return json({ error: "Not found." }, 404);
    }
    const group = parts[2];
    const device = parts[4];
    if (!GROUP.test(group) || (device !== undefined && !DEVICE.test(device))) {
      return json({ error: "Bad group or device id." }, 400);
    }

    if (env.RATE_LIMITER) {
      const key = request.headers.get("CF-Connecting-IP") || "unknown";
      const { success } = await env.RATE_LIMITER.limit({ key });
      if (!success) return json({ error: "Too many requests, try again in a minute." }, 429);
    }

    const auth = request.headers.get("Authorization") || "";
    const token = auth.startsWith("Bearer ") ? auth.slice(7).trim() : "";
    if (!TOKEN.test(token)) return json({ error: "Missing or malformed token." }, 401);
    const tokenHash = await sha256(token);
    const { value: known, metadata: authMeta } = await env.SYNC.getWithMetadata(authKey(group));

    if (device === undefined) {
      if (request.method !== "GET") return json({ error: "Use GET." }, 405);
      if (known === null) return json({ devices: [] }, 200);
      if (known !== tokenHash) return json({ error: "Forbidden." }, 403);
      return json({ devices: await listBlobs(env.SYNC, group) }, 200);
    }

    if (known !== null && known !== tokenHash) return json({ error: "Forbidden." }, 403);

    if (request.method === "DELETE") {
      await env.SYNC.delete(blobKey(group, device));
      return json({ ok: true }, 200);
    }
    if (request.method !== "PUT") return json({ error: "Use PUT or DELETE." }, 405);

    const length = Number(request.headers.get("Content-Length") || 0);
    if (length > LIMITS.blobBytes) return json({ error: "Record too large." }, 413);
    // Counted as it comes: a body without Content-Length is not read whole
    // before being found too large.
    const blob = await readBody(request, LIMITS.blobBytes);
    if (blob === null) return json({ error: "Record too large." }, 413);
    if (blob.length === 0) return json({ error: "Empty record." }, 400);

    // Writes and lists are the scarce operations (free tier): a device
    // already known is checked with a read, the group listed only for a
    // new one.
    const own = await env.SYNC.get(blobKey(group, device), { type: "stream" });
    if (own === null) {
      const existing = await env.SYNC.list({ prefix: blobPrefix(group) });
      if (existing.keys.length >= LIMITS.devicesPerGroup) {
        return json({ error: `At most ${LIMITS.devicesPerGroup} devices per group.` }, 409);
      }
    } else {
      await own.cancel?.();
    }

    const updated = Math.floor(Date.now() / 1000);
    await env.SYNC.put(blobKey(group, device), toBase64(blob), {
      expirationTtl: TTL_SECONDS,
      metadata: { updated },
    });
    // Recorded on the first write; rewritten only once half its life is
    // gone, not on every write.
    const expires = authMeta?.expires ?? 0;
    if (known === null || expires - updated < TTL_SECONDS / 2) {
      await env.SYNC.put(authKey(group), tokenHash, {
        expirationTtl: TTL_SECONDS,
        metadata: { expires: updated + TTL_SECONDS },
      });
    }
    return json({ ok: true, updated }, 200);
  },
};

const authKey = (group) => `auth:${group}`;
const blobPrefix = (group) => `blob:${group}:`;
const blobKey = (group, device) => `${blobPrefix(group)}${device}`;

/// The request's body, or `null` once it goes over `limit` bytes.
async function readBody(request, limit) {
  if (request.body === null) return new Uint8Array(0);
  const reader = request.body.getReader();
  const chunks = [];
  let size = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    size += value.byteLength;
    if (size > limit) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const body = new Uint8Array(size);
  let at = 0;
  for (const chunk of chunks) {
    body.set(chunk, at);
    at += chunk.byteLength;
  }
  return body;
}

async function listBlobs(kv, group) {
  const prefix = blobPrefix(group);
  const { keys } = await kv.list({ prefix });
  const devices = await Promise.all(
    keys.map(async (k) => {
      const data = await kv.get(k.name);
      if (data === null) return null;
      return { id: k.name.slice(prefix.length), data, updated: k.metadata?.updated ?? 0 };
    }),
  );
  return devices.filter(Boolean);
}

function json(data, status) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}
