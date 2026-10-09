// Minimal localization layer. Every user-facing string lives in
// src/locales/<code>.json; adding a language means adding one JSON file
// (see docs/TRANSLATING.md). English is the fallback for missing keys.

type Dict = { [k: string]: string | Dict };

const files = import.meta.glob<{ default: Dict }>("./locales/*.json", { eager: true });
const locales: Record<string, Dict> = {};
for (const [path, mod] of Object.entries(files)) {
  const code = path.match(/([\w-]+)\.json$/)![1];
  locales[code] = mod.default;
}

const FALLBACK = "en";
let current = FALLBACK;

function lookup(dict: Dict | undefined, key: string): string | undefined {
  let node: string | Dict | undefined = dict;
  for (const part of key.split(".")) {
    if (node === undefined || typeof node === "string") return undefined;
    node = node[part];
  }
  return typeof node === "string" ? node : undefined;
}

export function t(key: string, vars: Record<string, string | number> = {}): string {
  const raw = lookup(locales[current], key) ?? lookup(locales[FALLBACK], key) ?? key;
  return raw.replace(/\{(\w+)\}/g, (m, name) => (name in vars ? String(vars[name]) : m));
}

/** True when a key exists (used for optional labels such as option names). */
export function has(key: string): boolean {
  return lookup(locales[current], key) !== undefined || lookup(locales[FALLBACK], key) !== undefined;
}

export interface UiMsg {
  key: string;
  vars?: Record<string, string>;
}

/** Render a backend message ({key, vars}) or an unexpected error value. */
export function tm(msg: UiMsg | unknown): string {
  if (msg && typeof msg === "object" && "key" in msg) {
    const m = msg as UiMsg;
    return t(m.key, m.vars ?? {});
  }
  return t("errors.generic", { detail: String(msg) });
}

export function languages(): { code: string; name: string }[] {
  return Object.keys(locales)
    .map((code) => ({ code, name: lookup(locales[code], "_meta.name") ?? code }))
    .sort((a, b) => a.name.localeCompare(b.name));
}

export function setLanguage(code: string) {
  current = locales[code] ? code : FALLBACK;
  document.documentElement.lang = current;
  document.documentElement.dir = lookup(locales[current], "_meta.dir") === "rtl" ? "rtl" : "ltr";
}

export function language() {
  return current;
}

export function initialLanguage(): string {
  const saved = safeGet("converthub.lang");
  if (saved && locales[saved]) return saved;
  const nav = navigator.language?.toLowerCase() ?? "";
  return Object.keys(locales).find((c) => nav === c.toLowerCase()) ??
    Object.keys(locales).find((c) => nav.startsWith(c.toLowerCase().split("-")[0])) ??
    FALLBACK;
}

export function safeGet(k: string): string | null {
  try {
    return localStorage.getItem(k);
  } catch {
    return null;
  }
}

export function safeSet(k: string, v: string) {
  try {
    localStorage.setItem(k, v);
  } catch {
    /* storage unavailable: settings just won't persist */
  }
}
