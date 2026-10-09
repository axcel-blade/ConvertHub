// Verifies that every translation key referenced by the frontend and the
// Rust backend exists in src/locales/en.json, and that other locales do not
// contain keys English lacks. Run: npm run check:i18n
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const en = JSON.parse(readFileSync(join(root, "src/locales/en.json"), "utf8"));

const flat = (o, p = "") =>
  Object.entries(o).flatMap(([k, v]) => (typeof v === "object" ? flat(v, `${p}${k}.`) : [`${p}${k}`]));
const keys = new Set(flat(en));

const walk = (d) => readdirSync(d).flatMap((f) => {
  const p = join(d, f);
  return statSync(p).isDirectory() ? walk(p) : [p];
});

const used = new Set();
const patterns = [
  /\bt\(\s*"([\w.-]+)"/g,             // t("key")
  /ui!\(\s*"([\w.-]+)"/g,             // ui!("key")
  /UiMsg::new\(\s*"([\w.-]+)"/g,      // UiMsg::new("key")
  /(?:note_key|reason_key|noticeKey|hintKey): *(?:Some\()?"([\w.-]+)"/g,
  /"((?:recorder\.reasons|download\.notes)\.[\w]+)"/g,
];
for (const f of [...walk(join(root, "src")), ...walk(join(root, "src-tauri/src"))]) {
  if (!/\.(ts|rs)$/.test(f)) continue;
  const text = readFileSync(f, "utf8");
  for (const re of patterns) for (const m of text.matchAll(re)) used.add(m[1]);
}

// Keys built dynamically from definitions.
const defs = readFileSync(join(root, "src/defs.ts"), "utf8");
for (const m of defs.matchAll(/id: "(\w+)"/g)) { used.add(`ops.${m[1]}.title`); used.add(`ops.${m[1]}.desc`); }
for (const m of defs.matchAll(/key: "(\w+)"/g)) used.add(`fields.${m[1]}`);
for (const m of defs.matchAll(/confirmParam: "\w+"/g)) void m;
for (const id of ["video_delogo", "dvd_rip", "bluray_rip"]) used.add(`confirm.${id}`);
for (const id of ["dvd_rip", "bluray_rip", "cd_rip"]) used.add(`ops.${id}.inputHint`);
const presets = readFileSync(join(root, "src-tauri/src/presets.rs"), "utf8");
for (const m of presets.matchAll(/id: "(\w+)"/g)) used.add(`presets.${m[1]}`);
const deps = readFileSync(join(root, "src-tauri/src/deps.rs"), "utf8");
const depIds = [...deps.matchAll(/Tool::\w+ => "([\w-]+)",\n/g)].map((m) => m[1]);
for (const d of new Set(depIds)) {
  used.add(`deps.${d}.purpose`);
  for (const os of ["windows", "macos", "linux"]) used.add(`deps.${d}.hint.${os}`);
}
for (const v of ["video", "audio", "image", "pdf", "disc", "download", "recorder", "archive", "queue", "recent", "formats", "system", "settings"]) used.add(`nav.${v}`);
for (const s of ["queued", "running", "done", "failed", "cancelled"]) used.add(`status.${s}`);

let missing = [...used].filter((k) => !keys.has(k)).sort();
let extra = [];
for (const f of readdirSync(join(root, "src/locales"))) {
  if (f === "en.json") continue;
  const other = flat(JSON.parse(readFileSync(join(root, "src/locales", f), "utf8")));
  extra.push(...other.filter((k) => !keys.has(k)).map((k) => `${f}: ${k}`));
}
if (missing.length) console.error("Missing in en.json:\n  " + missing.join("\n  "));
if (extra.length) console.error("Unknown keys in other locales:\n  " + extra.join("\n  "));
console.log(`${used.size} keys referenced, ${keys.size} defined.`);
process.exit(missing.length || extra.length ? 1 : 0);
