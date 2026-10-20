// Informational and preference pages: recent jobs, format guide, system,
// settings (language, output, parallel jobs, updates).

import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { api } from "../api";
import { clear, h } from "../dom";
import { language, languages, safeSet, setLanguage, t, tm } from "../i18n";
import { CONTRIBUTE_URL, render, state } from "../state";
import { outputRow, showError, toast } from "../ui";
import { jobInputLabel } from "./queue";
import { showShortcuts } from "../shortcuts";
import { submit } from "./tool";

export function recentView(): Node {
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
      h("strong", {}, t(`ops.${j.request.operation.op}.title`)), " — ", jobInputLabel(j),
      h("span", { class: "muted small" }, ` · ${new Date(j.finishedAt ?? j.createdAt).toLocaleString(language())}`),
      j.status === "done" && j.outputs[0] ? h("button", { class: "btn small", onclick: () => revealItemInDir(j.outputs[0]).catch(showError) }, t("queue.openFolder")) : null,
      h("button", { class: "btn small", "aria-label": `${t("recent.runAgain")}: ${jobInputLabel(j)}`,
        onclick: () => submit([{ ...j.request, overwrite: false }]) }, t("recent.runAgain")),
      j.message ? h("div", { class: "muted small" }, tm(j.message)) : null))));
  }).catch(showError);
  return wrap;
}

export function formatsView(): Node {
  const f = state.formats;
  const row = (labelKey: string, inn?: string[], out?: string[]) =>
    h("tr", {}, h("th", { scope: "row" }, t(labelKey)),
      h("td", {}, (inn ?? []).map((x) => x.toUpperCase()).join(", ")),
      h("td", {}, (out ?? []).map((x) => x.toUpperCase()).join(", ")));
  return h("div", { class: "panel prose" },
    h("p", {}, t("formats.intro")),
    h("table", { class: "kv formats" },
      h("thead", {}, h("tr", {}, h("th", { scope: "col" }, t("formats.category")), h("th", { scope: "col" }, t("formats.inputs")), h("th", { scope: "col" }, t("formats.outputs")))),
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

export function systemView(): Node {
  const sys = state.sys;
  const hw = state.hw;
  const deps = sys?.deps ?? [];
  return h("div", { class: "stack" },
    h("div", { class: "panel" },
      h("div", { class: "panel-head" }, h("h2", {}, t("system.deps")), h("div", { class: "spacer" }),
        h("button", { class: "btn small", onclick: async () => { state.sys = await api.systemInfo(); render(); } }, t("system.recheck"))),
      sys ? h("p", { class: "muted small" }, `ConvertHub ${sys.version} · ${sys.os} ${sys.arch}`) : h("p", {}, t("common.loading")),
      h("div", { class: "table-wrap" }, h("table", { class: "jobs" },
        h("thead", {}, h("tr", {}, ...["tool", "status", "usedFor", "setup"].map((c) => h("th", { scope: "col" }, t(`system.cols.${c}`))))),
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

async function checkForUpdates() {
  try {
    toast(t("updates.checking"));
    const update = await check();
    if (!update) return toast(t("updates.none"), "success");
    const ok = await ask(t("updates.available", { version: update.version, notes: update.body ?? "" }),
      { title: t("updates.title"), okLabel: t("updates.install"), cancelLabel: t("common.cancel") });
    if (!ok) return;
    toast(t("updates.downloading"));
    await update.downloadAndInstall();
    await relaunch();
  } catch (e) {
    toast(t("updates.failed", { detail: tm(e) }), "error");
  }
}

export function settingsView(): Node {
  const w = state.workers;
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
    h("div", { class: "field" }, h("label", { for: "set-workers" }, t("settings.workers")),
      h("select", { id: "set-workers", "aria-describedby": "set-workers-hint", onchange: async (e: Event) => {
        const n = Number((e.target as HTMLSelectElement).value);
        w.current = await api.setWorkers(n).catch((err) => { showError(err); return w.current; });
        safeSet("converthub.workers", String(w.current));
        render();
      } }, ...Array.from({ length: w.max }, (_, i) => i + 1).map((n) => h("option", { value: String(n), selected: n === w.current }, String(n)))),
      h("small", { class: "hint", id: "set-workers-hint" }, t("settings.workersHint"))),
    h("div", { class: "field" }, h("span", { class: "label" }, t("updates.title")),
      state.updaterConfigured
        ? h("button", { class: "btn small", onclick: checkForUpdates }, t("updates.check"))
        : h("p", { class: "muted small" }, t("updates.notConfigured"))),
    h("div", { class: "field" }, h("span", { class: "label" }, t("shortcuts.title")),
      h("button", { class: "btn small", "aria-keyshortcuts": "F1", onclick: showShortcuts }, t("shortcuts.show"))),
    h("p", { class: "muted small" }, t("settings.storageNote")));
}
