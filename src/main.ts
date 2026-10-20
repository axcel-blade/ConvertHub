// App shell and startup. Each view lives in src/views/; shared state in
// state.ts; shared widgets in ui.ts.

import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { ask } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type Job } from "./api";
import { SECTIONS } from "./defs";
import { clear, dirname, h } from "./dom";
import { initialLanguage, language, languages, safeGet, safeSet, setLanguage, t } from "./i18n";
import { installShortcuts } from "./shortcuts";
import { CONTRIBUTE_URL, activeJobs, currentOp, navigate, render, setRenderer, state, type View } from "./state";
import { showError, toast } from "./ui";
import { downloadView, recBanner, recorderView } from "./views/capture";
import { formatsView, recentView, settingsView, systemView } from "./views/pages";
import { notifyQueueDone, queueTable, updateJob, updateTaskbar } from "./views/queue";
import { addPaths, toolView } from "./views/tool";

const app = document.getElementById("app")!;

const ICONS: Record<string, string> = {
  video: "▶", audio: "♪", image: "▣", pdf: "▤", disc: "◉", download: "⇩", recorder: "●", archive: "▦",
  queue: "☰", recent: "↺", formats: "?", system: "⚙", settings: "✎",
};
const NAV_KEYS: Partial<Record<View, string>> = { queue: "Alt+Q", recent: "Alt+R", settings: "Alt+S" };
SECTIONS.forEach((s, i) => { NAV_KEYS[s] = `Alt+${i + 1}`; });

function navBtn(v: View) {
  const active = state.view === v;
  const badge = v === "queue" ? activeJobs().length : 0;
  return h("button", {
    id: `nav-${v}`,
    class: `nav-item${active ? " active" : ""}`,
    "aria-current": active ? "page" : undefined,
    "aria-keyshortcuts": NAV_KEYS[v],
    title: NAV_KEYS[v] ? `${t(`nav.${v}`)} (${NAV_KEYS[v]})` : undefined,
    onclick: () => navigate(v),
  },
    h("span", { class: "nav-icon", "aria-hidden": "true" }, ICONS[v]),
    h("span", {}, t(`nav.${v}`)),
    badge ? h("span", { class: "badge", "aria-label": t("queue.activeCount", { n: badge }) }, String(badge)) : null);
}

function shell() {
  clear(app);
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
    h("a", { class: "skip-link", href: "#view" }, t("a11y.skipToContent")),
    h("div", { class: "shell" },
      h("nav", { class: "sidebar", "aria-label": t("nav.label") },
        h("div", { class: "brand" }, h("span", { class: "logo", "aria-hidden": "true" }), "ConvertHub"),
        h("div", { class: "nav-group" }, ...SECTIONS.map(navBtn)),
        h("div", { class: "nav-sep" }),
        h("div", { class: "nav-group" }, ...(["queue", "recent", "formats", "system", "settings"] as View[]).map(navBtn))),
      h("main", { class: "main" },
        h("header", { class: "topbar" },
          h("h1", {}, t(`nav.${state.view}`)),
          h("div", { class: "spacer" }),
          state.hw ? h("span", { class: "chip", title: t("system.gpuTitle") },
            state.hw.verified.length ? t("system.gpuReady", { name: state.hw.verified[0].vendor }) : t("system.cpuOnly")) : null,
          lang,
          h("button", { class: "btn link", onclick: () => openUrl(CONTRIBUTE_URL).catch(showError) }, t("settings.contribute"))),
        state.recording ? recBanner() : null,
        h("section", { class: "view", id: "view", tabindex: "-1", "aria-label": t(`nav.${state.view}`) }, renderView()),
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

/** Offer to resume jobs left unfinished when the app last closed. */
async function offerRestore() {
  const n = await api.restorableJobs().catch(() => 0);
  if (!n) return;
  const ok = await ask(t("restore.body", { n }), { title: t("restore.title"), okLabel: t("restore.resume"), cancelLabel: t("restore.discard") });
  const restored = await api.restoreJobs(ok).catch((e) => { showError(e); return 0; });
  if (restored) toast(t("restore.done", { n: restored }), "success");
}

async function boot() {
  setRenderer(shell);
  setLanguage(initialLanguage());
  render();
  installShortcuts();
  const savedWorkers = Number(safeGet("converthub.workers"));
  const [presets, formats, jobs, out, workers, updater] = await Promise.all([
    api.presets().catch(() => []),
    api.formatGuide().catch(() => ({})),
    api.listJobs().catch(() => []),
    api.defaultOutputDir().catch(() => null),
    (savedWorkers ? api.setWorkers(savedWorkers).then(() => api.getWorkers()) : api.getWorkers()).catch(() => [1, 1] as [number, number]),
    api.updaterConfigured().catch(() => false),
  ]);
  state.presets = presets;
  state.formats = formats;
  state.workers = { current: workers[0], max: workers[1] };
  state.updaterConfigured = updater;
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
    if (was && !e.payload.running && !activeJobs().length) {
      toast(t("queue.allDone"), "success");
      notifyQueueDone();
    }
    updateTaskbar();
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
  await offerRestore();
}

boot().catch(showError);
