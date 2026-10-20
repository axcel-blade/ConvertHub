// The file-based tool views (video, audio, image, PDF, disc, archive).

import { ask, open } from "@tauri-apps/plugin-dialog";
import { api, type JobRequest } from "../api";
import { defaults, opsFor, toOperation, type Field, type OpDef, type Params, type Section } from "../defs";
import { basename, dirname, fmtBytes, fmtDuration, h } from "../dom";
import { t, tm } from "../i18n";
import { deleteUserPreset, saveLastParams, saveUserPreset, userPresets } from "../prefs";
import { currentOp, itemParams, navigate, render, state, tool, toolDefaults, type Item, type ToolState } from "../state";
import { chooseOutputDir, modal, optLabel, outputRow, showError, toast } from "../ui";

function opTabs(section: Section) {
  const ts = state.tools[section];
  const ops = opsFor(section);
  return h("div", { class: "ops", role: "tablist", "aria-label": t("tool.operations") },
    ...ops.map((o) => h("button", {
      role: "tab", id: `tab-${o.id}`, class: `op-tab${ts.opId === o.id ? " active" : ""}`, "aria-selected": String(ts.opId === o.id),
      onclick: () => { ts.opId = o.id; render(); },
    }, t(`ops.${o.id}.title`))));
}

export function toolView(section: Section): Node {
  const ts = state.tools[section];
  const op = currentOp();
  if (!op) return h("p", {}, "");
  const needsFfmpeg = ["video", "audio", "disc"].includes(section);
  const ffMissing = needsFfmpeg && state.sys && !state.sys.deps.find((d) => d.tool === "ffmpeg")?.found;
  return h("div", { class: "tool" },
    opTabs(section),
    h("p", { class: "op-desc" }, t(`ops.${op.id}.desc`)),
    ffMissing ? h("div", { class: "notice warn", role: "note" }, t("deps.ffmpegMissingBanner"), " ",
      h("button", { class: "btn link", onclick: () => navigate("system") }, t("deps.howToInstall"))) : null,
    op.noticeKey ? h("div", { class: "notice", role: "note" }, t(op.noticeKey)) : null,
    h("div", { class: "tool-body" }, fileList(ts, op), settingsPanel(ts, op)));
}

export async function addPaths(paths: string[]) {
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

export async function pickInputs() {
  const op = currentOp();
  if (!op || op.input === "none") return;
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
        op.combine ? h("span", { class: "order", "aria-label": t("tool.position", { n: idx + 1 }) }, String(idx + 1)) : h("input", {
          type: "checkbox", id: `sel-${idx}`, checked: it.selected, "aria-label": t("tool.selectFile", { name: basename(it.path) }),
          onchange: (e: Event) => { it.selected = (e.target as HTMLInputElement).checked; render(); },
        }),
        h("div", { class: "file-main", title: it.path },
          h("div", { class: "file-name" }, basename(it.path)),
          h("div", { class: "file-meta" }, folder ? it.path : [fmtBytes(it.size), it.info ? ` · ${it.info}` : ""])),
        op.combine ? h("button", { class: "icon-btn", id: `up-${idx}`, "aria-label": t("tool.moveUp"), disabled: idx === 0,
          onclick: () => { [ts.items[idx - 1], ts.items[idx]] = [ts.items[idx], ts.items[idx - 1]]; render(); } }, "↑") : null,
        op.combine ? h("button", { class: "icon-btn", id: `down-${idx}`, "aria-label": t("tool.moveDown"), disabled: idx === ts.items.length - 1,
          onclick: () => { [ts.items[idx + 1], ts.items[idx]] = [ts.items[idx], ts.items[idx + 1]]; render(); } }, "↓") : null,
        !folder ? h("button", { class: "icon-btn", "aria-label": t("tool.info", { name: basename(it.path) }), onclick: () => showInfo(it, op) }, "i") : null,
        h("button", { class: "icon-btn", "aria-label": t("tool.remove", { name: basename(it.path) }),
          onclick: () => { ts.items.splice(idx, 1); render(); } }, "✕"))))
    : h("button", { class: `dropzone${state.dragging ? " over" : ""}`, onclick: pickInputs, "aria-keyshortcuts": "Control+O" },
      h("strong", {}, folder ? t("tool.dropFolder") : t("tool.dropFiles")),
      h("span", {}, folder ? t(`ops.${op.id}.inputHint`) : t("tool.dropHint")));

  const toggleAll = () => {
    for (const i of ts.items) i.selected = !allSel;
    render();
  };
  return h("div", { class: "panel files" },
    h("div", { class: "panel-head" },
      h("h2", {}, folder ? t("tool.source") : t("tool.files")),
      h("div", { class: "spacer" }),
      !op.combine && !folder && ts.items.length ? h("button", { class: "btn small", id: "sel-all", onclick: toggleAll },
        allSel ? t("tool.selectNone") : t("tool.selectAll")) : null,
      ts.items.length ? h("button", { class: "btn small", onclick: () => { ts.items = []; render(); } }, t("tool.clear")) : null,
      h("button", { class: "btn small primary", onclick: pickInputs, "aria-keyshortcuts": "Control+O" },
        folder ? t("tool.chooseFolder") : t("tool.addFiles"))),
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

/** "My presets": apply, save or delete a named settings set for this tool. */
function presetRow(op: OpDef, editing: Params, apply: (rerender: boolean) => void) {
  if (!op.fields.some((f) => f.type !== "file" && f.key !== op.confirmParam)) return null;
  const mine = userPresets(op);
  const names = Object.keys(mine).sort((a, b) => a.localeCompare(b));
  const nameInput = h("input", { id: `up-name-${op.id}`, type: "text", placeholder: t("userPresets.namePlaceholder"),
    "aria-label": t("userPresets.name") }) as HTMLInputElement;
  const select = h("select", { id: `up-sel-${op.id}`, "aria-label": t("userPresets.choose"),
    onchange: (e: Event) => {
      const name = (e.target as HTMLSelectElement).value;
      if (!name || !mine[name]) return;
      Object.assign(editing, defaults(op), mine[name]);
      apply(true);
      toast(t("userPresets.applied", { name }), "success");
    } },
    h("option", { value: "" }, names.length ? t("userPresets.choose") : t("userPresets.none")),
    ...names.map((n) => h("option", { value: n }, n))) as HTMLSelectElement;
  return h("details", { class: "user-presets" },
    h("summary", {}, t("userPresets.title")),
    h("div", { class: "preset-row" },
      select,
      names.length ? h("button", { class: "btn small", onclick: () => {
        if (!select.value) return;
        deleteUserPreset(op, select.value);
        render();
      } }, t("userPresets.delete")) : null),
    h("div", { class: "preset-row" },
      nameInput,
      h("button", { class: "btn small", onclick: () => {
        const name = nameInput.value.trim();
        if (!name) return toast(t("userPresets.needName"), "error");
        saveUserPreset(op, name, editing);
        toast(t("userPresets.saved", { name }), "success");
        render();
      } }, t("userPresets.save"))));
}

function settingsPanel(ts: ToolState, op: OpDef) {
  const selected = ts.items.filter((i) => i.selected);
  // Combined jobs use one settings set; otherwise edit the first selected file
  // and apply changes to every selected file (per-file settings).
  const editing: Params = op.combine || selected.length === 0 ? toolDefaults(ts, op) : itemParams(ts, selected[0], op);
  const apply = (rerender: boolean) => {
    if (!op.combine) for (const it of selected) if (it !== selected[0]) it.params[op.id] = { ...editing };
    // New files for this tool start from the latest choices, now and next launch.
    if (editing !== ts.defaults[op.id]) ts.defaults[op.id] = { ...editing };
    saveLastParams(op, editing);
    if (rerender) render();
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
    presetRow(op, editing, apply),
    op.combine ? h("div", { class: "field" }, h("label", { for: "out-name" }, t("output.name")),
      h("input", { id: "out-name", type: "text", value: ts.outputName, placeholder: t("output.nameAuto"),
        oninput: (e: Event) => { ts.outputName = (e.target as HTMLInputElement).value; } })) : null,
    outputRow(),
    h("div", { class: "actions" },
      h("button", { class: "btn primary big", id: "enqueue", "aria-keyshortcuts": "Control+Enter",
        disabled: ts.items.length === 0 || (!op.combine && selected.length === 0),
        onclick: () => enqueue(ts, op) }, t("tool.addToQueue", { n: op.combine ? 1 : selected.length }))));
}

async function resolveOutputDir(input?: string): Promise<string | null> {
  if (state.sameAsSource && input) return dirname(input);
  if (state.outputDir) return state.outputDir;
  return chooseOutputDir();
}

/** Queue requests, asking before anything existing would be overwritten. */
export async function submit(requests: JobRequest[]): Promise<boolean> {
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

/** Add the current tool's files to the queue (used by the button and Ctrl+Enter). */
export function enqueueCurrent() {
  const op = currentOp();
  const ts = tool();
  if (op && ts && op.input !== "none") enqueue(ts, op).catch(showError);
}

async function enqueue(ts: ToolState, op: OpDef) {
  const items = op.combine ? ts.items : ts.items.filter((i) => i.selected);
  if (!items.length) return;
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
