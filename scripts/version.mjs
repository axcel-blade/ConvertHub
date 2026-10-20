// Single source of truth for the app version is package.json.
// tauri.conf.json reads it directly ("version": "../package.json"); Cargo
// needs its own copy, which this script keeps in sync.
//
//   node scripts/version.mjs          check that every file agrees (CI)
//   node scripts/version.mjs 0.2.0    set the version everywhere

import { readFileSync, writeFileSync } from "node:fs";

const root = new URL("../", import.meta.url);
const read = (p) => readFileSync(new URL(p, root), "utf8");
const write = (p, s) => writeFileSync(new URL(p, root), s);

const CARGO_TOML = "src-tauri/Cargo.toml";
const CARGO_LOCK = "src-tauri/Cargo.lock";
const tomlRe = /^(\[package\][\s\S]*?^version\s*=\s*")([^"]+)(")/m;
const lockRe = /(\[\[package\]\]\nname = "converthub"\nversion = ")([^"]+)(")/;

const target = process.argv[2];
if (target && !/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(target)) {
  console.error(`Not a semantic version: ${target}`);
  process.exit(1);
}

const pkg = JSON.parse(read("package.json"));
const tauri = JSON.parse(read("src-tauri/tauri.conf.json"));
const toml = read(CARGO_TOML);
const lock = read(CARGO_LOCK);

if (target) {
  pkg.version = target;
  write("package.json", `${JSON.stringify(pkg, null, 2)}\n`);
  write(CARGO_TOML, toml.replace(tomlRe, `$1${target}$3`));
  write(CARGO_LOCK, lock.replace(lockRe, `$1${target}$3`));
  console.log(`Version set to ${target}. Also update CHANGELOG.md and package-lock.json (npm install).`);
  process.exit(0);
}

const found = {
  "package.json": pkg.version,
  [CARGO_TOML]: toml.match(tomlRe)?.[2],
  [CARGO_LOCK]: lock.match(lockRe)?.[2],
};
const errors = Object.entries(found).filter(([, v]) => v !== pkg.version);
if (tauri.version !== "../package.json") errors.push(["src-tauri/tauri.conf.json", `${tauri.version} (should be "../package.json")`]);
if (errors.length) {
  console.error(`Version mismatch (package.json is ${pkg.version}):`);
  for (const [f, v] of errors) console.error(`  ${f}: ${v ?? "not found"}`);
  console.error("Run: node scripts/version.mjs <version>");
  process.exit(1);
}
console.log(`Version ${pkg.version} is consistent.`);
