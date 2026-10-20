// Per-tool preferences kept on this computer: the settings last used for each
// operation, and named user presets. Values that must be confirmed every
// time (authorization boxes) or that point at a specific file are never kept.

import type { OpDef, Params } from "./defs";
import { safeGet, safeSet } from "./i18n";

const LAST = "converthub.last.";
const PRESETS = "converthub.userPresets.";

/** Fields that should not be remembered between jobs. */
export function persistable(op: OpDef, params: Params): Params {
  const out: Params = {};
  for (const f of op.fields) {
    if (f.type === "file" || f.key === op.confirmParam) continue;
    const v = params[f.key];
    if (v !== undefined) out[f.key] = v;
  }
  return out;
}

function readJson<T>(key: string, fallback: T): T {
  const raw = safeGet(key);
  if (!raw) return fallback;
  try {
    const v = JSON.parse(raw);
    return v && typeof v === "object" ? (v as T) : fallback;
  } catch {
    return fallback;
  }
}

export function loadLastParams(op: OpDef): Params {
  // Re-filter on load so stale keys from older versions are dropped.
  return persistable(op, readJson<Params>(LAST + op.id, {}));
}

export function saveLastParams(op: OpDef, params: Params) {
  safeSet(LAST + op.id, JSON.stringify(persistable(op, params)));
}

export function userPresets(op: OpDef): Record<string, Params> {
  return readJson<Record<string, Params>>(PRESETS + op.id, {});
}

export function saveUserPreset(op: OpDef, name: string, params: Params) {
  const all = userPresets(op);
  all[name] = persistable(op, params);
  safeSet(PRESETS + op.id, JSON.stringify(all));
}

export function deleteUserPreset(op: OpDef, name: string) {
  const all = userPresets(op);
  delete all[name];
  safeSet(PRESETS + op.id, JSON.stringify(all));
}
