import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ask, message, open } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, type HwInfo, type Job, type JobRequest, type Preset, type SystemInfo } from "./api";
import { OPS, SECTIONS, defaults, opsFor, toOperation, type Field, type OpDef, type Params, type Section } from "./defs";
import { basename, clear, dirname, extOf, fmtBytes, fmtDuration, h } from "./dom";
import { has, initialLanguage, language, languages, safeGet, safeSet, setLanguage, t, tm } from "./i18n";

const CONTRIBUTE_URL = "https://www.pcfreetime.com/formatfactory/transhelp.php?language=en";

type View = Section | "queue" | "recent" | "formats" | "system" | "settings";

interface Item {
  path: string;
  size: number;
  selected: boolean;
  /** Per-file settings, keyed by operation id. */
  params: Record<string, Params>;
  info?: string;
}

interface ToolState {
  opId: string;
  items: Item[];
  /** Settings used for new files and for combined (multi-input) jobs. */
  defaults: Record<string, Params>;
  outputName: string;
}

const state = {
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
};

for (const s of SECTIONS) {
  const first = opsFor(s)[0];
  state.tools[s] = { opId: first?.id ?? "", items: [], defaults: {}, outputName: "" };
}

const app = document.getElementById("app")!;

// ------------------------------------------------------------------ helpers

function tool(): ToolState {
  return state.tools[state.view];
}

function currentOp(): OpDef | undefined {
  return OPS.find((o) => o.id === tool()?.opId);
}

function toolDefaults(ts: ToolState, op: OpDef): Params {
  return (ts.defaults[op.id] ??= defaults(op));
}

function itemParams(ts: ToolState, item: Item, op: OpDef): Params {
  return (item.params[op.id] ??= { ...toolDefaults(ts, op) });
}

function toast(text: string, kind: "info" | "error" | "success" = "info") {
  const region = document.getElementById("toasts")!;
  const el = h("div", { class: `toast ${kind}`, role: kind === "error" ? "alert" : "status" },
    h("span", { class: "toast-text" }, text),
    h("button", { class: "icon-btn toast-close", "aria-label": t("common.close"), onclick: () => el.remove() }, "×"));
  region.append(el);
  // Keep the toast up while the user is reading it (hover or keyboard focus).
  let timer = 0;
  const arm = () => { timer = window.setTimeout(() => el.remove(), kind === "error" ? 9000 : 4500); };
  const hold = () => clearTimeout(timer);
  el.addEventListener("mouseenter", hold);
  el.addEventListener("focusin", hold);
  el.addEventListener("mouseleave", arm);
  el.addEventListener("focusout", arm);
  arm();
}

function showError(e: unknown) {
  toast(tm(e), "error");
}

function optLabel(key: string, value: string) {
  const k = `options.${key}.${value}`;
  return has(k) ? t(k) : value.toUpperCase();
}

function modal(title: string, body: Node) {
  const dlg = h("dialog", { class: "modal", "aria-label": title },
    h("div", { class: "modal-inner" },
      h("h2", {}, title),
      h("div", { class: "modal-body" }, body),
      h("div", { class: "modal-actions" }, h("button", { class: "btn", onclick: () => dlg.close() }, t("common.close")))));
  dlg.addEventListener("close", () => dlg.remove());
  // Clicking the backdrop (outside the dialog box) closes it.
  dlg.addEventListener("click", (e) => { if (e.target === dlg) dlg.close(); });
  document.body.append(dlg);
  dlg.showModal();
}

async function chooseOutputDir(): Promise<string | null> {
  const dir = await open({ directory: true, defaultPath: state.outputDir || undefined, title: t("output.choose") });
  if (typeof dir === "string") {
    state.outputDir = dir;
    safeSet("converthub.outputDir", dir);
    return dir;
  }
  return null;
}

// ------------------------------------------------------------------ shell

function render() {
  clear(app);
  const navBtn = (v: View, icon: string) => {
    const active = state.view === v;
    const badge =
      v === "queue"
        ? [...state.jobs.values()].filter((j) => j.status === "queued" || j.status === "running").length
        : 0;
    return h("button", {
      class: `nav-item${active ? " active" : ""}`,
      "aria-current": active ? "page" : undefined,
      onclick: () => { state.view = v; render(); },
    },
      h("span", { class: "nav-icon", "aria-hidden": "true" }, icon),
      h("span", {}, t(`nav.${v}`)),
      badge ? h("span", { class: "badge", "aria-label": t("queue.activeCount", { n: badge }) }, String(badge)) : null);
  };
  const icons: Record<string, string> = {
    video: "▶", audio: "♪", image: "▣", pdf: "▤", disc: "◉", download: "⇩", recorder: "●", archive: "▦",
    queue: "☰", recent: "↺", formats: "?", system: "⚙", settings: "✎",
  };

  const lang = h("select", {
    id: "lang", "aria-label": t("settings.language"),
    onchange: (e: Event) => {
      const v = (e.target as HTMLSelectElement).value;
      setLanguage(v);
      safeSet("converthub.lang", v);
      render();
    },
  }, ...languages().map((l) => h("option", { value: l.code, selected: l.code === language() }, l.name)));

  app.append(
    h("div", { class: "shell" },
      h("nav", { class: "sidebar", "aria-label": t("nav.label") },
        h("div", { class: "brand" }, h("span", { class: "logo", "aria-hidden": "true" }), "ConvertHub"),
        h("div", { class: "nav-group" }, ...SECTIONS.map((s) => navBtn(s, icons[s]))),
        h("div", { class: "nav-sep" }),
        h("div", { class: "nav-group" }, ...(["queue", "recent", "formats", "system", "settings"] as View[]).map((v) => navBtn(v, icons[v])))),
      h("main", { class: "main" },
        h("header", { class: "topbar" },
          h("h1", {}, t(`nav.${state.view}`)),
          h("div", { class: "spacer" }),
          state.hw ? h("span", { class: "chip", title: t("system.gpuTitle") },
            state.hw.verified.length ? t("system.gpuReady", { name: state.hw.verified[0].vendor }) : t("system.cpuOnly")) : null,
          lang,
          h("button", { class: "btn link", onclick: () => openUrl(CONTRIBUTE_URL).catch(showError) }, t("settings.contribute"))),
        state.recording ? recBanner() : null,
        h("section", { class: "view", id: "view" }, renderView()),
        state.view !== "queue" ? h("section", { class: "dock", "aria-label": t("nav.queue") }, queueTable(true)) : null)),
    h("div", { id: "toasts", class: "toasts", "aria-live": "polite" }),
  );
  if (state.dragging) document.body.classList.add("dragging");
}

function renderView(): Node {
  switch (state.view) {
    case "queue": return queueTable(false);
    case "recent": return recentView();
    case "formats": return formatsView();
    case "system": return systemView();
    case "settings": return settingsView();
    case "download": return downloadView();
    case "recorder": return recorderView();
    default: return toolView(state.view);
  }
}

// ------------------------------------------------------------------ tool view

function opTabs(section: Section) {
  const ts = state.tools[section];
  const ops = opsFor(section);
  return h("div", { class: "ops", role: "tablist", "aria-label": t("tool.operations") },
    ...ops.map((o) => h("button", {
      role: "tab", class: `op-tab${ts.opId === o.id ? " active" : ""}`, "aria-selected": String(ts.opId === o.id),
      onclick: () => { ts.opId = o.id; render(); },
    }, t(`ops.${o.id}.title`))));
}

function toolView(section: Section): Node {
  const ts = state.tools[section];
  const op = currentOp();
  if (!op) return h("p", {}, "");
  const needsFfmpeg = ["video", "audio", "disc"].includes(section);
  const ffMissing = needsFfmpeg && state.sys && !state.sys.deps.find((d) => d.tool === "ffmpeg")?.found;
  return h("div", { class: "tool" },
    opTabs(section),
    h("p", { class: "op-desc" }, t(`ops.${op.id}.desc`)),
    ffMissing ? h("div", { class: "notice warn", role: "note" }, t("deps.ffmpegMissingBanner"), " ",
      h("button", { class: "btn link", onclick: () => { state.view = "system"; render(); } }, t("deps.howToInstall"))) : null,
    op.noticeKey ? h("div", { class: "notice", role: "note" }, t(op.noticeKey)) : null,
    h("div", { class: "tool-body" }, fileList(ts, op), settingsPanel(ts, op)));
}

async function addPaths(paths: string[]) {
  const op = currentOp();
  if (!op || op.input === "none") return;
  const ts = tool();
  if (op.input === "folder") {
    for (const p of paths) {
      if (!ts.items.some((i) => i.path === p)) ts.items.push({ path: p, size: 0, selected: true, params: {} });
    }
    render();
    return;
  }
  const checks = await api.validateInputs(paths, op.category ?? "media").catch((e) => { showError(e); return []; });
  let added = 0;
  for (const c of checks) {
    if (!c.ok) {
      toast(tm(c.error), "error");
      continue;
    }
    if (ts.items.some((i) => i.path === c.path)) continue;
    ts.items.push({ path: c.path, size: c.size, selected: true, params: {} });
    added++;
  }
  if (added) toast(t("tool.added", { n: added }), "success");
  render();
}

async function pickInputs() {
  const op = currentOp();
  if (!op) return;
  if (op.input === "folder") {
    const dir = await open({ directory: true, title: t("tool.chooseFolder") });
    if (typeof dir === "string") await addPaths([dir]);
    return;
  }
  const key = `${op.category === "media" ? "audio" : op.category}_in`;
  const exts = op.category === "media"
    ? [...(state.formats.audio_in ?? []), ...(state.formats.video_in ?? [])]
    : op.category === "pdf" ? ["pdf"] : state.formats[key] ?? [];
  const files = await open({
    multiple: true, title: t("tool.addFiles"),
    filters: exts.length ? [{ name: t(`nav.${op.section}`), extensions: exts }, { name: t("tool.allFiles"), extensions: ["*"] }] : undefined,
  });
  if (Array.isArray(files)) await addPaths(files);
  else if (typeof files === "string") await addPaths([files]);
}

function fileList(ts: ToolState, op: OpDef) {
  const folder = op.input === "folder";
  const allSel = ts.items.length > 0 && ts.items.every((i) => i.selected);
  const list = ts.items.length
    ? h("ul", { class: "file-list", "aria-label": t("tool.files") },
      ...ts.items.map((it, idx) => h("li", { class: `file${it.selected ? " sel" : ""}` },
        op.combine ? h("span", { class: "order" }, String(idx + 1)) : h("input", {
          type: "checkbox", checked: it.selected, "aria-label": t("tool.selectFile", { name: basename(it.path) }),
          onchange: (e: Event) => { it.selected = (e.target as HTMLInputElement).checked; render(); },
        }),
        h("div", { class: "file-main", title: it.path },
          h("div", { class: "file-name" }, basename(it.path)),
          h("div", { class: "file-meta" }, folder ? it.path : [fmtBytes(it.size), it.info ? ` · ${it.info}` : ""])),
        op.combine ? h("button", { class: "icon-btn", "aria-label": t("tool.moveUp"), disabled: idx === 0,
          onclick: () => { [ts.items[idx - 1], ts.items[idx]] = [ts.items[idx], ts.items[idx - 1]]; render(); } }, "↑") : null,
        op.combine ? h("button", { class: "icon-btn", "aria-label": t("tool.moveDown"), disabled: idx === ts.items.length - 1,
          onclick: () => { [ts.items[idx + 1], ts.items[idx]] = [ts.items[idx], ts.items[idx + 1]]; render(); } }, "↓") : null,
        !folder ? h("button", { class: "icon-btn", "aria-label": t("tool.info", { name: basename(it.path) }), onclick: () => showInfo(it, op) }, "i") : null,
        h("button", { class: "icon-btn", "aria-label": t("tool.remove", { name: basename(it.path) }),
          onclick: () => { ts.items.splice(idx, 1); render(); } }, "✕"))))
    : h("button", { class: `dropzone${state.dragging ? " over" : ""}`, onclick: pickInputs },
      h("strong", {}, folder ? t("tool.dropFolder") : t("tool.dropFiles")),
      h("span", {}, folder ? t(`ops.${op.id}.inputHint`) : t("tool.dropHint")));

  return h("div", { class: "panel files" },
    h("div", { class: "panel-head" },
      h("h2", {}, folder ? t("tool.source") : t("tool.files")),
      h("div", { class: "spacer" }),
      !op.combine && !folder && ts.items.length ? h("button", { class: "btn small",
        onclick: () => { ts.items.forEach((i) => (i.selected = !allSel)); render(); } }, allSel ? t("tool.selectNone") : t("tool.selectAll")) : null,
      ts.items.length ? h("button", { class: "btn small", onclick: () => { ts.items = []; render(); } }, t("tool.clear")) : null,
      h("button", { class: "btn small primary", onclick: pickInputs }, folder ? t("tool.chooseFolder") : t("tool.addFiles"))),
    list,
    ts.items.length && !folder ? h("p", { class: "muted small" }, t("tool.dragMore")) : null);
}

async function showInfo(it: Item, op: OpDef) {
  try {
    if (op.section === "image") {
      const info = await api.imageInfo(it.path);
      const tbl = h("table", { class: "kv" },
        h("tr", {}, h("th", {}, t("info.dimensions")), h("td", {}, `${info.width} × ${info.height}`)),
        h("tr", {}, h("th", {}, t("info.format")), h("td", {}, info.format)),
        ...info.tags.map(([k, v]) => h("tr", {}, h("th", {}, k), h("td", {}, v))));
      modal(basename(it.path), info.tags.length ? tbl : h("div", {}, tbl, h("p", { class: "muted" }, t("info.noTags"))));
    } else if (op.section === "pdf" || op.section === "archive") {
      modal(basename(it.path), h("p", {}, `${fmtBytes(it.size)} — ${it.path}`));
    } else {
      const p = await api.probeMedia(it.path);
      it.info = [p.duration ? fmtDuration(p.duration) : null, p.width ? `${p.width}×${p.height}` : null, p.videoCodec, p.audioCodec]
        .filter(Boolean).join(" · ");
      render();
    }
  } catch (e) {
    showError(e);
  }
}

function fieldInput(f: Field, params: Params, onChange: (rerender: boolean) => void, idPrefix: string): Node | null {
  if (f.showIf && !f.showIf(params)) return null;
  const id = `${idPrefix}-${f.key}`;
  const label = t(`fields.${f.key}`);
  const hint = f.hintKey ? h("small", { class: "hint", id: `${id}-hint` }, t(f.hintKey)) : null;
  const describedBy = f.hintKey ? `${id}-hint` : undefined;
  const v = params[f.key];

  if (f.type === "checkbox") {
    return h("div", { class: "field check" },
      h("input", { type: "checkbox", id, checked: Boolean(v), "aria-describedby": describedBy,
        onchange: (e: Event) => { params[f.key] = (e.target as HTMLInputElement).checked; onChange(true); } }),
      h("label", { for: id }, label), hint);
  }
  let input: HTMLElement;
  if (f.type === "select") {
    let options: { value: string; label: string }[] = [];
    if (f.options === "presets") {
      options = state.presets.map((p) => ({ value: p.id, label: t(`presets.${p.id}`) }));
    } else if (f.options === "accel") {
      // GPU choices appear only once a hardware encoder has been verified.
      // "Auto" is always safe: the backend falls back to the CPU.
      const gpu = (state.hw?.verified.length ?? 0) > 0;
      options = (gpu ? ["auto", "cpu", "gpu"] : ["auto", "cpu"]).map((x) => ({ value: x, label: optLabel("accel", x) }));
      if (!gpu && v === "gpu") params[f.key] = "auto";
    } else {
      options = (f.options ?? []).map((x) => ({ value: x, label: optLabel(f.key, x) }));
    }
    if (f.optional) options.unshift({ value: "", label: t(`options.${f.key}.none`) });
    input = h("select", { id, "aria-describedby": describedBy,
      onchange: (e: Event) => { params[f.key] = (e.target as HTMLSelectElement).value; onChange(true); } },
      ...options.map((o) => h("option", { value: o.value, selected: String(params[f.key] ?? "") === o.value }, o.label)));
  } else if (f.type === "file") {
    const path = String(v ?? "");
    input = h("div", { class: "file-field" },
      h("span", { class: "muted", title: path }, path ? basename(path) : t("tool.noFileChosen")),
      h("button", { class: "btn small", id, onclick: async () => {
        const fsel = await open({ multiple: false, title: label,
          filters: [{ name: t("nav.audio"), extensions: [...(state.formats.audio_in ?? []), ...(state.formats.video_in ?? [])] }] });
        if (typeof fsel === "string") { params[f.key] = fsel; onChange(true); }
      } }, t("tool.browse")));
  } else {
    input = h("input", {
      id, type: f.type === "number" ? "number" : "text", value: v ?? "", min: f.min, max: f.max, step: f.step,
      placeholder: f.type === "time" ? "hh:mm:ss" : f.type === "number" ? t("tool.auto") : "",
      inputmode: f.type === "number" ? "numeric" : undefined, "aria-describedby": describedBy,
      oninput: (e: Event) => { params[f.key] = (e.target as HTMLInputElement).value; onChange(false); },
    });
  }
  return h("div", { class: "field" }, h("label", { for: id }, label), input, hint);
}

function settingsPanel(ts: ToolState, op: OpDef) {
  const selected = ts.items.filter((i) => i.selected);
  // Combined jobs use one settings set; otherwise edit the first selected file
  // and apply changes to every selected file (per-file settings).
  const editing: Params = op.combine || selected.length === 0 ? toolDefaults(ts, op) : itemParams(ts, selected[0], op);
  const apply = (rerender: boolean) => {
    if (!op.combine) for (const it of selected) if (it !== selected[0]) it.params[op.id] = { ...editing };
    if (rerender) {
      const focused = document.activeElement?.id;
      render();
      if (focused) document.getElementById(focused)?.focus();
    }
  };
  const scope = op.combine
    ? t("tool.scopeCombined", { n: ts.items.length })
    : selected.length
      ? t("tool.scopeSelected", { n: selected.length })
      : t("tool.scopeNew");

  const fields = op.fields.map((f) => fieldInput(f, editing, apply, `f-${op.id}`)).filter(Boolean) as Node[];
  return h("div", { class: "panel settings" },
    h("div", { class: "panel-head" }, h("h2", {}, t("tool.settings")), h("div", { class: "spacer" }), h("span", { class: "muted small" }, scope)),
    fields.length ? h("div", { class: "fields" }, ...fields) : h("p", { class: "muted" }, t("tool.noSettings")),
    op.combine ? h("div", { class: "field" }, h("label", { for: "out-name" }, t("output.name")),
      h("input", { id: "out-name", type: "text", value: ts.outputName, placeholder: t("output.nameAuto"),
        oninput: (e: Event) => (ts.outputName = (e.target as HTMLInputElement).value) })) : null,
    outputRow(),
    h("div", { class: "actions" },
      h("button", { class: "btn primary big", disabled: ts.items.length === 0 || (!op.combine && selected.length === 0),
        onclick: () => enqueue(ts, op) }, t("tool.addToQueue", { n: op.combine ? 1 : selected.length }))));
}

function outputRow() {
  return h("div", { class: "output-row" },
    h("div", { class: "field check" },
      h("input", { type: "checkbox", id: "same-src", checked: state.sameAsSource, onchange: (e: Event) => {
        state.sameAsSource = (e.target as HTMLInputElement).checked;
        safeSet("converthub.sameAsSource", state.sameAsSource ? "1" : "0");
        render();
      } }),
      h("label", { for: "same-src" }, t("output.sameAsSource"))),
    !state.sameAsSource ? h("div", { class: "field" },
      h("label", { for: "out-dir" }, t("output.folder")),
      h("div", { class: "file-field" },
        h("span", { class: state.outputDir ? "" : "muted", title: state.outputDir }, state.outputDir || t("output.notSet")),
        h("button", { id: "out-dir", class: "btn small", onclick: async () => { if (await chooseOutputDir()) render(); } }, t("tool.browse")))) : null);
}

async function resolveOutputDir(input?: string): Promise<string | null> {
  if (state.sameAsSource && input) return dirname(input);
  if (state.outputDir) return state.outputDir;
  return chooseOutputDir();
}

async function submit(requests: JobRequest[]): Promise<boolean> {
  try {
    const conflicts = await api.checkConflicts(requests);
    if (conflicts.length) {
      const list = conflicts.slice(0, 8).map(basename).join("\n") + (conflicts.length > 8 ? `\n… (+${conflicts.length - 8})` : "");
      const ok = await ask(t("dialogs.overwrite", { list }), { title: t("dialogs.overwriteTitle"), kind: "warning",
        okLabel: t("dialogs.replace"), cancelLabel: t("common.cancel") });
      if (!ok) return false;
      const set = new Set(conflicts);
      // Only the conflicting jobs are allowed to overwrite.
      const all = await Promise.all(requests.map(async (r) => ({ r, c: await api.checkConflicts([r]) })));
      for (const { r, c } of all) r.overwrite = c.some((x) => set.has(x));
    }
    await api.submitJobs(requests);
    toast(t(state.queueRunning ? "tool.queuedRunning" : "tool.queued", { n: requests.length }), "success");
    return true;
  } catch (e) {
    showError(e);
    return false;
  }
}

async function enqueue(ts: ToolState, op: OpDef) {
  const items = op.combine ? ts.items : ts.items.filter((i) => i.selected);
  const paramsFor = (it?: Item) => (op.combine || !it ? toolDefaults(ts, op) : itemParams(ts, it, op));
  for (const it of op.combine ? [undefined] : items) {
    const p = paramsFor(it);
    if (op.confirmParam && !p[op.confirmParam]) {
      toast(t(`confirm.${op.id}`), "error");
      return;
    }
    if (op.id === "video_mux" && !p.audio) {
      toast(t("errors.muxNeedsAudio"), "error");
      return;
    }
  }
  if (op.combine && items.length < 2) {
    toast(t("errors.needTwo"), "error");
    return;
  }
  const requests: JobRequest[] = [];
  if (op.combine) {
    const dir = await resolveOutputDir(items[0]?.path);
    if (!dir) return;
    requests.push({ inputs: items.map((i) => i.path), outputDir: dir, outputName: ts.outputName.trim() || undefined,
      overwrite: false, operation: toOperation(op, paramsFor()) });
  } else {
    for (const it of items) {
      const dir = await resolveOutputDir(it.path);
      if (!dir) return;
      requests.push({ inputs: [it.path], outputDir: dir, overwrite: false, operation: toOperation(op, paramsFor(it)) });
    }
  }
  if (await submit(requests)) {
    ts.items = op.combine ? [] : ts.items.filter((i) => !i.selected);
    render();
  }
}

// ------------------------------------------------------------------ queue

function opIdOf(job: Job) {
  const o = job.request.operation.op;
  return o;
}

function outputFormat(job: Job): string {
  const op = job.request.operation;
  const def = OPS.find((o) => o.id === op.op);
  if (op.preset) return t(`presets.${op.preset}`);
  if (def?.formatParam && op[def.formatParam]) return String(op[def.formatParam]).toUpperCase();
  if (def?.outputLabel === "same") return extOf(job.request.inputs[0] ?? "").toUpperCase() || "—";
  if (def?.outputLabel === "folder") return t("queue.folder");
  if (def?.outputLabel) return def.outputLabel.toUpperCase();
  if (op.op === "download") return t("queue.original");
  return "—";
}

function jobInputLabel(job: Job) {
  const r = job.request;
  if (r.operation.op === "download") return String(r.operation.url ?? "");
  if (r.inputs.length > 1) return t("queue.nFiles", { first: basename(r.inputs[0]), n: r.inputs.length - 1 });
  return basename(r.inputs[0] ?? "");
}

function queueTable(compact: boolean): Node {
  const jobs = [...state.jobs.values()].sort((a, b) => b.createdAt - a.createdAt);
  const shown = compact ? jobs.filter((j) => j.status === "queued" || j.status === "running" || Date.now() - (j.finishedAt ?? 0) < 120000).slice(0, 6) : jobs;
  const waiting = jobs.filter((j) => j.status === "queued").length;
  const head = h("div", { class: "panel-head" },
    h("h2", {}, compact ? t("queue.title") : t("queue.all")),
    state.queueRunning
      ? h("button", { class: "btn small", onclick: () => api.pauseQueue().catch(showError) }, t("queue.pause"))
      : h("button", { class: "btn small primary", disabled: waiting === 0,
        onclick: () => api.startQueue().catch(showError) }, t("queue.start", { n: waiting })),
    h("div", { class: "spacer" }),
    compact ? h("button", { class: "btn small", onclick: () => { state.view = "queue"; render(); } }, t("queue.viewAll", { n: jobs.length })) : null,
    jobs.some((j) => !["queued", "running"].includes(j.status))
      ? h("button", { class: "btn small", onclick: async () => {
        await api.clearFinished();
        for (const [id, j] of state.jobs) if (!["queued", "running"].includes(j.status)) state.jobs.delete(id);
        render();
      } }, t("queue.clearFinished")) : null);
  if (!shown.length) {
    return h("div", { class: compact ? "queue compact" : "queue" }, head,
      h("p", { class: "empty" }, compact ? t("queue.emptyCompact") : t("queue.empty")));
  }
  return h("div", { class: compact ? "queue compact" : "queue" }, head,
    h("div", { class: "table-wrap" },
      h("table", { class: "jobs" },
        h("thead", {}, h("tr", {}, ...["input", "operation", "output", "progress", "status", "engine", "location", "actions"].map((c) => h("th", { scope: "col" }, t(`queue.cols.${c}`))))),
        h("tbody", {}, ...shown.map(jobRow)))));
}

function jobRow(job: Job) {
  const pct = Math.round(job.progress * 100);
  const location = job.outputs[0] ?? job.request.outputDir;
  const msg = job.message ? tm(job.message) : "";
  return h("tr", { class: `job ${job.status}`, id: `job-${job.id}` },
    h("td", { class: "ellipsis", title: job.request.inputs.join("\n") || String(job.request.operation.url ?? "") }, jobInputLabel(job)),
    h("td", {}, t(`ops.${opIdOf(job)}.title`)),
    h("td", {}, outputFormat(job)),
    h("td", {},
      h("div", { class: "progress", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(pct),
        "aria-label": t("queue.progressOf", { name: jobInputLabel(job) }) },
        h("div", { class: "bar", style: `width:${pct}%` })),
      h("small", {}, `${pct}%`)),
    h("td", {}, h("span", { class: `status ${job.status}` },
      job.status === "queued" && !state.queueRunning ? t("status.waiting") : t(`status.${job.status}`)),
      msg ? h("div", { class: `job-msg ${job.status === "failed" ? "err" : ""}` }, msg) : null),
    h("td", { class: "small" }, job.engine ?? "—"),
    h("td", { class: "ellipsis small", title: location }, location),
    h("td", { class: "row-actions" },
      job.status === "queued" || job.status === "running"
        ? h("button", { class: "btn small", onclick: () => api.cancelJob(job.id).catch(showError) }, t("queue.cancel")) : null,
      job.status === "failed" || job.status === "cancelled"
        ? h("button", { class: "btn small", onclick: async () => {
          await api.retryJob(job.id).catch(showError);
          state.jobs.delete(job.id);
          render();
        } }, t("queue.retry")) : null,
      job.status === "done"
        ? h("button", { class: "btn small", onclick: () => revealItemInDir(location).catch(showError) }, t("queue.openFolder")) : null,
      !["queued", "running"].includes(job.status)
        ? h("button", { class: "icon-btn", "aria-label": t("queue.remove"), onclick: async () => {
          await api.removeJob(job.id);
          state.jobs.delete(job.id);
          render();
        } }, "✕") : null));
}

function updateJob(job: Job) {
  const prev = state.jobs.get(job.id);
  state.jobs.set(job.id, job);
  const row = document.getElementById(`job-${job.id}`);
  const statusChanged = !prev || prev.status !== job.status;
  if (row && !statusChanged) {
    // Cheap in-place progress update.
    row.replaceWith(jobRow(job));
  } else {
    const focused = document.activeElement?.id;
    render();
    if (focused) document.getElementById(focused)?.focus();
  }
  if (statusChanged && job.status === "done") toast(t("queue.doneToast", { name: jobInputLabel(job) }), "success");
  if (statusChanged && job.status === "failed") toast(t("queue.failedToast", { name: jobInputLabel(job) }), "error");
}

// ------------------------------------------------------------------ other views

function recentView(): Node {
  const wrap = h("div", { class: "panel" }, h("p", { class: "muted" }, t("common.loading")));
  api.recentJobs().then((jobs) => {
    clear(wrap);
    wrap.append(h("div", { class: "panel-head" }, h("h2", {}, t("recent.title")), h("div", { class: "spacer" }),
      jobs.length ? h("button", { class: "btn small", onclick: async () => { await api.clearRecent(); render(); } }, t("recent.clear")) : null));
    if (!jobs.length) {
      wrap.append(h("p", { class: "empty" }, t("recent.empty")));
      return;
    }
    wrap.append(h("ul", { class: "recent" }, ...jobs.map((j) => h("li", {},
      h("span", { class: `status ${j.status}` }, t(`status.${j.status}`)),
      h("strong", {}, t(`ops.${opIdOf(j)}.title`)), " — ", jobInputLabel(j),
      h("span", { class: "muted small" }, ` · ${new Date(j.finishedAt ?? j.createdAt).toLocaleString(language())}`),
      j.status === "done" && j.outputs[0] ? h("button", { class: "btn small", onclick: () => revealItemInDir(j.outputs[0]).catch(showError) }, t("queue.openFolder")) : null,
      j.message ? h("div", { class: "muted small" }, tm(j.message)) : null))));
  }).catch(showError);
  return wrap;
}

function formatsView(): Node {
  const f = state.formats;
  const row = (labelKey: string, inn?: string[], out?: string[]) =>
    h("tr", {}, h("th", { scope: "row" }, t(labelKey)),
      h("td", {}, (inn ?? []).map((x) => x.toUpperCase()).join(", ")),
      h("td", {}, (out ?? []).map((x) => x.toUpperCase()).join(", ")));
  return h("div", { class: "panel prose" },
    h("p", {}, t("formats.intro")),
    h("table", { class: "kv formats" },
      h("thead", {}, h("tr", {}, h("th", {}, t("formats.category")), h("th", {}, t("formats.inputs")), h("th", {}, t("formats.outputs")))),
      h("tbody", {},
        row("nav.video", f.video_in, f.video_out),
        row("nav.audio", f.audio_in, f.audio_out),
        row("nav.image", f.image_in, f.image_out),
        row("nav.pdf", ["pdf"], f.pdf_out),
        row("nav.archive", f.archive_in, [t("formats.extractedFolder")]),
        row("nav.disc", [t("formats.discInputs")], [t("formats.discOutputs")]))),
    h("h3", {}, t("formats.notesTitle")),
    h("ul", {}, ...["heic", "pdf", "gpu", "lgpl", "disc", "download"].map((k) => h("li", {}, t(`formats.notes.${k}`)))));
}

function systemView(): Node {
  const sys = state.sys;
  const hw = state.hw;
  const deps = sys?.deps ?? [];
  return h("div", { class: "stack" },
    h("div", { class: "panel" },
      h("div", { class: "panel-head" }, h("h2", {}, t("system.deps")), h("div", { class: "spacer" }),
        h("button", { class: "btn small", onclick: async () => { state.sys = await api.systemInfo(); render(); } }, t("system.recheck"))),
      sys ? h("p", { class: "muted small" }, `ConvertHub ${sys.version} · ${sys.os} ${sys.arch}`) : h("p", {}, t("common.loading")),
      h("div", { class: "table-wrap" }, h("table", { class: "jobs" },
        h("thead", {}, h("tr", {}, ...["tool", "status", "usedFor", "setup"].map((c) => h("th", {}, t(`system.cols.${c}`))))),
        h("tbody", {}, ...deps.map((d) => h("tr", {},
          h("td", {}, h("strong", {}, d.tool), d.version ? h("div", { class: "muted small" }, d.version) : null),
          h("td", {}, h("span", { class: `status ${d.found ? "done" : "failed"}` }, d.found ? t("system.found") : t("system.missing")),
            d.path ? h("div", { class: "muted small ellipsis", title: d.path }, d.path) : null),
          h("td", { class: "small" }, t(`deps.${d.tool}.purpose`)),
          h("td", { class: "small" }, d.found ? "—" : t(`deps.${d.tool}.hint.${sys?.os ?? "linux"}`)))))))),
    h("div", { class: "panel" },
      h("div", { class: "panel-head" }, h("h2", {}, t("system.gpu")), h("div", { class: "spacer" }),
        h("button", { class: "btn small", onclick: async () => {
          toast(t("system.detecting"));
          state.hw = await api.hardwareInfo(true).catch((e) => { showError(e); return state.hw; });
          render();
        } }, t("system.redetect"))),
      !hw ? h("p", {}, t("system.detecting")) : !hw.ffmpegFound ? h("p", {}, t("system.gpuNoFfmpeg")) : h("div", {},
        h("p", {}, hw.verified.length ? t("system.gpuVerified", { list: hw.verified.map((v) => `${v.name} (${v.vendor})`).join(", ") }) : t("system.gpuNone")),
        h("p", { class: "muted small" }, t("system.hwaccels", { list: hw.hwaccels.join(", ") || "—" })),
        h("p", { class: "muted small" }, t("system.compiled", { list: hw.compiled.join(", ") || "—" })),
        h("p", { class: "muted small" }, t("system.gpuExplain")))),
    h("div", { class: "panel prose" }, h("h2", {}, t("privacy.title")), h("p", {}, t("privacy.body"))));
}

function settingsView(): Node {
  return h("div", { class: "panel stack" },
    h("div", { class: "field" }, h("label", { for: "set-lang" }, t("settings.language")),
      h("select", { id: "set-lang", onchange: (e: Event) => {
        const v = (e.target as HTMLSelectElement).value;
        setLanguage(v);
        safeSet("converthub.lang", v);
        render();
      } }, ...languages().map((l) => h("option", { value: l.code, selected: l.code === language() }, l.name)))),
    h("p", { class: "muted" }, t("settings.translateHelp"), " ",
      h("button", { class: "btn link", onclick: () => openUrl(CONTRIBUTE_URL).catch(showError) }, t("settings.contribute"))),
    outputRow(),
    h("p", { class: "muted small" }, t("settings.storageNote")));
}

function downloadView(): Node {
  const d = state.download;
  const wrap = h("div", { class: "tool" },
    h("p", { class: "op-desc" }, t("ops.download.desc")),
    h("div", { class: "notice warn", role: "note" }, t("download.networkNotice")),
    h("div", { class: "tool-body" },
      h("div", { class: "panel" },
        h("div", { class: "field" }, h("label", { for: "dl-urls" }, t("download.urls")),
          h("textarea", { id: "dl-urls", rows: "5", placeholder: "https://…", spellcheck: "false",
            oninput: (e: Event) => (d.urls = (e.target as HTMLTextAreaElement).value) }, d.urls)),
        h("div", { class: "field check" },
          h("input", { type: "checkbox", id: "dl-ack", checked: d.acknowledged, onchange: (e: Event) => (d.acknowledged = (e.target as HTMLInputElement).checked) }),
          h("label", { for: "dl-ack" }, t("download.ack"))),
        outputRow(),
        h("div", { class: "actions" }, h("button", { class: "btn primary big", onclick: async () => {
          const urls = d.urls.split(/\s+/).map((u) => u.trim()).filter(Boolean);
          if (!urls.length) return toast(t("download.noUrls"), "error");
          if (!d.acknowledged) return toast(t("errors.downloadAck"), "error");
          const dir = state.outputDir || (await chooseOutputDir());
          if (!dir) return;
          const reqs = urls.map((url) => ({ inputs: [], outputDir: dir, overwrite: false, operation: { op: "download", url, acknowledged: true } }));
          if (await submit(reqs)) { d.urls = ""; render(); }
        } }, t("download.start")))),
      h("div", { class: "panel" }, h("h2", {}, t("download.sources")), h("ul", { class: "sources" }, h("li", {}, t("common.loading"))))));
  api.downloadSources().then((src) => {
    const ul = wrap.querySelector(".sources")!;
    clear(ul);
    for (const s of src) {
      ul.append(h("li", {},
        h("span", { class: `status ${s.enabled ? "done" : "cancelled"}` }, s.enabled ? t("download.enabled") : t("download.disabled")),
        h("strong", {}, s.name), h("div", { class: "muted small" }, t(s.noteKey))));
    }
  }).catch(showError);
  return wrap;
}

function recBanner() {
  return h("div", { class: "rec-banner", role: "status" },
    h("span", { class: "rec-dot", "aria-hidden": "true" }),
    h("strong", {}, t("recorder.recording")), " ",
    h("span", { id: "rec-time" }, "00:00"),
    h("div", { class: "spacer" }),
    h("button", { class: "btn danger", onclick: stopRecording }, t("recorder.stop")));
}

async function stopRecording() {
  try {
    const path = await api.stopRecording();
    toast(t("recorder.saved", { path }), "success");
    await message(t("recorder.saved", { path }), { title: t("nav.recorder") });
    revealItemInDir(path).catch(() => {});
  } catch (e) {
    showError(e);
  }
  state.recording = false;
  clearInterval(state.recTimer);
  getCurrentWindow().setTitle("ConvertHub").catch(() => {});
  render();
}

function recorderView(): Node {
  const r = state.rec;
  const wrap = h("div", { class: "tool" }, h("p", { class: "op-desc" }, t("ops.recorder.desc")),
    h("div", { class: "notice", role: "note" }, t("recorder.permissionNotice")),
    h("div", { class: "panel" }, h("p", {}, t("common.loading"))));
  api.captureTargets().then((tg) => {
    const panel = wrap.querySelector(".panel")!;
    clear(panel);
    if (!tg.supported) {
      panel.append(h("p", { class: "notice warn" }, t(tg.reasonKey ?? "recorder.reasons.unknown")));
      return;
    }
    const targets = r.kind === "window" ? tg.windows.map((w) => ({ id: w.id, label: w.title }))
      : tg.displays.map((d) => ({ id: d.id, label: `${d.name} (${d.width}×${d.height})` }));
    if (!targets.some((x) => x.id === r.id)) r.id = targets[0]?.id ?? "";
    panel.append(
      h("div", { class: "fields" },
        h("div", { class: "field" }, h("label", { for: "rec-kind" }, t("recorder.source")),
          h("select", { id: "rec-kind", disabled: state.recording, onchange: (e: Event) => { r.kind = (e.target as HTMLSelectElement).value; r.id = ""; render(); } },
            h("option", { value: "display", selected: r.kind === "display" }, t("recorder.display")),
            tg.windowCapture ? h("option", { value: "window", selected: r.kind === "window" }, t("recorder.window")) : null)),
        h("div", { class: "field" }, h("label", { for: "rec-target" }, r.kind === "window" ? t("recorder.window") : t("recorder.display")),
          targets.length
            ? h("select", { id: "rec-target", disabled: state.recording, onchange: (e: Event) => (r.id = (e.target as HTMLSelectElement).value) },
              ...targets.map((x) => h("option", { value: x.id, selected: x.id === r.id }, x.label)))
            : h("p", { class: "muted" }, t("recorder.noTargets"))),
        h("div", { class: "field" }, h("label", { for: "rec-fps" }, t("recorder.fps")),
          h("select", { id: "rec-fps", disabled: state.recording, onchange: (e: Event) => (r.fps = Number((e.target as HTMLSelectElement).value)) },
            ...[15, 24, 30, 60].map((f) => h("option", { value: String(f), selected: r.fps === f }, String(f))))),
        h("div", { class: "field" }, h("label", { for: "rec-format" }, t("fields.format")),
          h("select", { id: "rec-format", disabled: state.recording, onchange: (e: Event) => (r.format = (e.target as HTMLSelectElement).value) },
            ...["mp4", "mkv"].map((f) => h("option", { value: f, selected: r.format === f }, f.toUpperCase())))),
        h("div", { class: "field" }, h("label", { for: "rec-name" }, t("output.name")),
          h("input", { id: "rec-name", type: "text", value: r.name, disabled: state.recording, oninput: (e: Event) => (r.name = (e.target as HTMLInputElement).value) }))),
      h("div", { class: "field" }, h("label", { for: "out-dir" }, t("output.folder")),
        h("div", { class: "file-field" }, h("span", { class: state.outputDir ? "" : "muted" }, state.outputDir || t("output.notSet")),
          h("button", { id: "out-dir", class: "btn small", disabled: state.recording, onclick: async () => { if (await chooseOutputDir()) render(); } }, t("tool.browse")))),
      h("div", { class: "actions" }, state.recording
        ? h("button", { class: "btn danger big", onclick: stopRecording }, t("recorder.stop"))
        : h("button", { class: "btn primary big", disabled: !r.id, onclick: startRecording }, t("recorder.start"))));
  }).catch(showError);
  return wrap;
}

async function startRecording() {
  const r = state.rec;
  const dir = state.outputDir || (await chooseOutputDir());
  if (!dir) return;
  const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
  const fileName = `${r.name.trim() || "recording"}-${stamp}`;
  try {
    await api.startRecording({ kind: r.kind, id: r.id, fps: r.fps, format: r.format, outputDir: dir, fileName, overwrite: false });
    state.recording = true;
    getCurrentWindow().setTitle(`● ${t("recorder.recording")} — ConvertHub`).catch(() => {});
    const started = Date.now();
    state.recTimer = window.setInterval(() => {
      const el = document.getElementById("rec-time");
      if (el) el.textContent = fmtDuration((Date.now() - started) / 1000);
    }, 500);
    render();
  } catch (e) {
    showError(e);
  }
}

// ------------------------------------------------------------------ boot

async function boot() {
  setLanguage(initialLanguage());
  render();
  const [presets, formats, jobs, out] = await Promise.all([
    api.presets().catch(() => []),
    api.formatGuide().catch(() => ({})),
    api.listJobs().catch(() => []),
    api.defaultOutputDir().catch(() => null),
  ]);
  state.presets = presets;
  state.formats = formats;
  for (const j of jobs) state.jobs.set(j.id, j);
  state.queueRunning = (await api.queueState().catch(() => ({ running: false }))).running;
  // Use the default when nothing is saved, and move users still on the old
  // default (the Videos folder itself) to the ConvertHub subfolder.
  if (out && (!state.outputDir || state.outputDir === dirname(out))) {
    state.outputDir = out;
    safeSet("converthub.outputDir", out);
  }
  const rec = await api.recordingState().catch(() => null);
  state.recording = !!rec?.recording;
  render();

  await listen<Job>("job-updated", (e) => updateJob(e.payload));
  await listen<{ running: boolean }>("queue-state", (e) => {
    const was = state.queueRunning;
    state.queueRunning = e.payload.running;
    if (was && !e.payload.running && ![...state.jobs.values()].some((j) => j.status === "queued" || j.status === "running")) {
      toast(t("queue.allDone"), "success");
    }
    render();
  });
  await getCurrentWebview().onDragDropEvent((e) => {
    const p = e.payload;
    if (p.type === "enter" || p.type === "over") {
      if (!state.dragging) { state.dragging = true; document.body.classList.add("dragging"); }
    } else {
      state.dragging = false;
      document.body.classList.remove("dragging");
      if (p.type === "drop" && p.paths.length) {
        const op = currentOp();
        if (!op) toast(t("tool.dropNotHere"), "error");
        else addPaths(p.paths).catch(showError);
      }
    }
  });

  api.systemInfo().then((s) => { state.sys = s; render(); }).catch(showError);
  api.hardwareInfo(false).then((hw) => { state.hw = hw; render(); }).catch(() => {});
}

boot().catch(showError);
