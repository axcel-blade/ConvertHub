// Views that do not take files: online download and screen recorder.

import { getCurrentWindow } from "@tauri-apps/api/window";
import { message } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import { clear, fmtDuration, h } from "../dom";
import { t } from "../i18n";
import { render, state } from "../state";
import { chooseOutputDir, outputRow, showError, toast } from "../ui";
import { submit } from "./tool";

export function downloadView(): Node {
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

export function recBanner() {
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

export function recorderView(): Node {
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

