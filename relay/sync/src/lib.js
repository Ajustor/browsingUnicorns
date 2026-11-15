// Constants and helpers of the relay, apart from src/index.js: the Workers
// runtime only accepts handlers as named exports of the main module.

export const LIMITS = { blobBytes: 1024 * 1024, devicesPerGroup: 10 };
/// Groups untouched for this long are forgotten (KV expiration, seconds).
export const TTL_SECONDS = 180 * 24 * 60 * 60;

export async function sha256(text) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export function toBase64(bytes) {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}