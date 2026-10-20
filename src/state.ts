// Shared UI state and the render hook. Views import `render` from here
// instead of from main.ts, which keeps the module graph acyclic.

import type { HwInfo, Job, Preset, SystemInfo } from "./api";
import { OPS, SECTIONS, defaults, opsFor, type OpDef, type Params, type Section } from "./defs";
import { safeGet } from "./i18n";
import { loadLastParams } from "./prefs";

export const CONTRIBUTE_URL = "https://www.pcfreetime.com/formatfactory/transhelp.php?language=en";

export type View = Section | "queue" | "recent" | "formats" | "system" | "settings";

export interface Item {
  path: string;
  size: number;
  selected: boolean;
  /** Per-file settings, keyed by operation id. */
  params: Record<string, Params>;
  info?: string;
}

export interface ToolState {
  opId: string;
  items: Item[];
  /** Settings used for new files and for combined (multi-input) jobs. */
  defaults: Record<string, Params>;
  outputName: string;
}

export const state = {
  view: "video" as View,
  outputDir: safeGet("converthub.outputDir") ?? "",
  sameAsSource: safeGet("converthub.sameAsSource") === "1",
  hw: null as HwInfo | null,
  sys: null as SystemInfo | null,
  presets: [] as Preset[],
  formats: {} as Record<string, string[]>,
  jobs: new Map<string, Job>(),
  tools: {} as Record<string, ToolState>,
  dragging: false,
  recording: false,
  queueRunning: false,
  recTimer: 0 as number,
  download: { urls: "", acknowledged: false },
  rec: { kind: "display", id: "", fps: 30, format: "mp4", name: "recording" },
  workers: { current: 1, max: 1 },
  updaterConfigured: false,
};

for (const s of SECTIONS) {
  const first = opsFor(s)[0];
  state.tools[s] = { opId: first?.id ?? "", items: [], defaults: {}, outputName: "" };
}

let renderFn: () => void = () => {};

export function setRenderer(fn: () => void) {
  renderFn = fn;
}

/** Re-render the whole app, keeping keyboard focus where it was. */
export function render() {
  const focused = document.activeElement?.id;
  renderFn();
  if (focused) document.getElementById(focused)?.focus();
}

export function navigate(v: View) {
  state.view = v;
  render();
  document.getElementById("view")?.focus({ preventScroll: true });
}

export function tool(): ToolState {
  return state.tools[state.view];
}

export function currentOp(): OpDef | undefined {
  return OPS.find((o) => o.id === tool()?.opId);
}

export function toolDefaults(ts: ToolState, op: OpDef): Params {
  // New sessions start from the settings the user last used for this tool.
  ts.defaults[op.id] ??= { ...defaults(op), ...loadLastParams(op) };
  return ts.defaults[op.id];
}

export function itemParams(ts: ToolState, item: Item, op: OpDef): Params {
  item.params[op.id] ??= { ...toolDefaults(ts, op) };
  return item.params[op.id];
}

export function activeJobs(): Job[] {
  return [...state.jobs.values()].filter((j) => j.status === "queued" || j.status === "running");
}
