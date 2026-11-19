// uBlock Origin's scriptlets (`src/js/resources`, ES modules) as adblock-rust
// resources: the code the lists' `+js(...)` rules run, which the ad blocker
// downloads (published by .github/workflows/ublock-resources.yml).
//
// usage: node scripts/ublock-resources.mjs <uBlock checkout> <out.json>

import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const [root, out] = process.argv.slice(2);
if (!root || !out) {
  console.error('usage: node scripts/ublock-resources.mjs <uBlock checkout> <out.json>');
  process.exit(2);
}

const entry = pathToFileURL(path.join(root, 'src', 'js', 'resources', 'scriptlets.js'));
const { builtinScriptlets } = await import(entry.href);

// Helpers (`*.fn`) are pulled in by the scriptlets that list them; trusted
// scriptlets (`trusted-*`) need the permission uBlock's own lists are given.
const resources = builtinScriptlets.map((s) => ({
  name: s.name,
  aliases: s.aliases ?? [],
  kind: { mime: s.name.endsWith('.fn') ? 'fn/javascript' : 'application/javascript' },
  content: Buffer.from(s.fn.toString()).toString('base64'),
  dependencies: s.dependencies ?? [],
  ...(s.requiresTrust ? { permission: 1 } : {}),
}));

if (!resources.some((r) => r.name === 'set-constant.js')) {
  console.error('set-constant.js missing: uBlock changed its scriptlets');
  process.exit(1);
}
fs.writeFileSync(out, JSON.stringify(resources));
console.log(`${resources.length} resources`);
