#!/usr/bin/env python3
"""Sign `latest.json` for the in-app updater (Ed25519).

Usage: sign-manifest.py <latest.json> <public-key-file>

The private key is read from the UPDATE_SIGNING_KEY environment variable (a
PEM Ed25519 private key, a GitHub secret). The script adds a `signature`
field (hex) to the manifest, signing the canonical payload that
crates/updater/src/signature.rs rebuilds: keep both in sync. It first checks
that the key matches <public-key-file> (hex, the key built into the app), so
a wrong secret fails the release instead of publishing a manifest every
client refuses.

Signing uses `openssl pkeyutl -rawin` (OpenSSL 3, on the GitHub runners): no
Python package to install.

Make a key pair (once):
    openssl genpkey -algorithm ed25519 -out update_signing_key.pem
    openssl pkey -in update_signing_key.pem -pubout -outform DER | tail -c 32 | xxd -p -c 32
The PEM goes into the UPDATE_SIGNING_KEY secret, the hex into
crates/updater/update-signing-key.pub.
"""
import json
import os
import subprocess
import sys
import tempfile

HEADER = b"update-manifest-v1\n"


def field(value):
    data = value.encode("utf-8")
    return str(len(data)).encode() + b":" + data + b"\n"


def payload(manifest):
    assets = manifest.get("assets", [])
    out = HEADER
    out += field(manifest["version"])
    out += field(manifest["page_url"])
    out += field(manifest.get("notes", ""))
    out += field(str(len(assets)))
    for a in assets:
        out += field(a["name"]) + field(a["url"]) + field(a["sha256"])
    return out


def openssl(*args, data=None):
    return subprocess.run(["openssl", *args], input=data, capture_output=True, check=True).stdout


def main():
    manifest_path, public_key_path = sys.argv[1:3]
    key_pem = os.environ.get("UPDATE_SIGNING_KEY", "").strip()
    if not key_pem:
        sys.exit("::error::UPDATE_SIGNING_KEY is not set: the updater refuses an unsigned latest.json")
    with open(public_key_path, encoding="utf-8") as f:
        expected = f.read().strip().lower()
    with open(manifest_path, encoding="utf-8") as f:
        manifest = json.load(f)
    manifest.pop("signature", None)

    with tempfile.TemporaryDirectory() as tmp:
        key = os.path.join(tmp, "key.pem")
        fd = os.open(key, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(fd, "w") as f:
            f.write(key_pem + "\n")
        public = openssl("pkey", "-in", key, "-pubout", "-outform", "DER")[-32:].hex()
        if public != expected:
            sys.exit(
                f"::error::UPDATE_SIGNING_KEY's public key {public} is not the one in "
                f"{public_key_path} ({expected})"
            )
        data = os.path.join(tmp, "payload")
        with open(data, "wb") as f:
            f.write(payload(manifest))
        signature = openssl("pkeyutl", "-sign", "-rawin", "-inkey", key, "-in", data)

    if len(signature) != 64:
        sys.exit(f"::error::unexpected Ed25519 signature length {len(signature)}")
    manifest["signature"] = signature.hex()
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, ensure_ascii=False, indent=2)
        f.write("\n")


if __name__ == "__main__":
    main()
