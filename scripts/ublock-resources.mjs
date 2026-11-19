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

// What adblock-rust would drop without a word: refused here.
const problems = [];
const names = new Set();
for (const r of resources) {
  for (const name of [r.name, ...r.aliases]) {
    if (names.has(name)) problems.push(`${name}: named twice`);
    names.add(name);
  }
}
for (const s of builtinScriptlets) {
  // Not a named function: taken for a template, never called.
  if (!s.name.endsWith('.fn') && !/^function\s+[^\s(){}]+\s*\(/.test(s.fn.toString())) {
    problems.push(`${s.name}: not a named function`);
  }
  for (const dep of s.dependencies ?? []) {
    if (!names.has(dep)) problems.push(`${s.name}: depends on ${dep}, which is no resource`);
  }
}
if (!names.has('set-constant.js')) problems.push('set-constant.js missing');
if (problems.length > 0) {
  console.error(`uBlock changed its scriptlets:\n  ${problems.join('\n  ')}`);
  process.exit(1);
}
fs.writeFileSync(out, JSON.stringify(resources));
console.log(`${resources.length} resources`);
