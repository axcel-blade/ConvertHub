// Job queue table, live job updates, and progress outside the window
// (taskbar/dock progress and a system notification when a batch finishes).

import { getCurrentWindow, ProgressBarStatus, UserAttentionType } from "@tauri-apps/api/window";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, type Job } from "../api";
import { OPS } from "../defs";
import { basename, extOf, h } from "../dom";
import { t, tm } from "../i18n";
import { activeJobs, navigate, render, state } from "../state";
import { showError, toast } from "../ui";

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

export function jobInputLabel(job: Job) {
  const r = job.request;
  if (r.operation.op === "download") return String(r.operation.url ?? "");
  if (r.inputs.length > 1) return t("queue.nFiles", { first: basename(r.inputs[0]), n: r.inputs.length - 1 });
  return basename(r.inputs[0] ?? "");
}

export function toggleQueue() {
  const waiting = [...state.jobs.values()].some((j) => j.status === "queued");
  if (state.queueRunning) api.pauseQueue().catch(showError);
  else if (waiting) api.startQueue().catch(showError);
}

async function retryFailed() {
  const ids = await api.retryFailed().catch((e) => { showError(e); return [] as string[]; });
  for (const [id, j] of state.jobs) if (j.status === "failed") state.jobs.delete(id);
  if (ids.length) toast(t("queue.retriedAll", { n: ids.length }), "success");
  render();
}

export function queueTable(compact: boolean): Node {
  const jobs = [...state.jobs.values()].sort((a, b) => b.createdAt - a.createdAt);
  const shown = compact ? jobs.filter((j) => j.status === "queued" || j.status === "running" || Date.now() - (j.finishedAt ?? 0) < 120000).slice(0, 6) : jobs;
  const waiting = jobs.filter((j) => j.status === "queued").length;
  const failed = jobs.filter((j) => j.status === "failed").length;
  const head = h("div", { class: "panel-head" },
    h("h2", {}, compact ? t("queue.title") : t("queue.all")),
    state.queueRunning
      ? h("button", { class: "btn small", id: "q-toggle", "aria-keyshortcuts": "Control+Shift+S", onclick: toggleQueue }, t("queue.pause"))
      : h("button", { class: "btn small primary", id: "q-toggle", "aria-keyshortcuts": "Control+Shift+S", disabled: waiting === 0,
        onclick: toggleQueue }, t("queue.start", { n: waiting })),
    h("div", { class: "spacer" }),
    failed ? h("button", { class: "btn small", onclick: retryFailed }, t("queue.retryAll", { n: failed })) : null,
    compact ? h("button", { class: "btn small", onclick: () => navigate("queue") }, t("queue.viewAll", { n: jobs.length })) : null,
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
        h("caption", { class: "sr-only" }, compact ? t("queue.title") : t("queue.all")),
        h("thead", {}, h("tr", {}, ...["input", "operation", "output", "progress", "status", "engine", "location", "actions"].map((c) => h("th", { scope: "col" }, t(`queue.cols.${c}`))))),
        h("tbody", {}, ...shown.map(jobRow)))));
}

function jobRow(job: Job) {
  const pct = Math.round(job.progress * 100);
  const location = job.outputs[0] ?? job.request.outputDir;
  const msg = job.message ? tm(job.message) : "";
  const name = jobInputLabel(job);
  return h("tr", { class: `job ${job.status}`, id: `job-${job.id}` },
    h("td", { class: "ellipsis", title: job.request.inputs.join("\n") || String(job.request.operation.url ?? "") }, name),
    h("td", {}, t(`ops.${job.request.operation.op}.title`)),
    h("td", {}, outputFormat(job)),
    h("td", {},
      h("div", { class: "progress", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": String(pct),
        "aria-label": t("queue.progressOf", { name }) },
        h("div", { class: "bar", style: `width:${pct}%` })),
      h("small", { "aria-hidden": "true" }, `${pct}%`)),
    h("td", {}, h("span", { class: `status ${job.status}` },
      job.status === "queued" && !state.queueRunning ? t("status.waiting") : t(`status.${job.status}`)),
      msg ? h("div", { class: `job-msg ${job.status === "failed" ? "err" : ""}` }, msg) : null),
    h("td", { class: "small" }, job.engine ?? "—"),
    h("td", { class: "ellipsis small", title: location }, location),
    h("td", { class: "row-actions" },
      job.status === "queued" || job.status === "running"
        ? h("button", { class: "btn small", "aria-label": `${t("queue.cancel")}: ${name}`, onclick: () => api.cancelJob(job.id).catch(showError) }, t("queue.cancel")) : null,
      job.status === "failed" || job.status === "cancelled"
        ? h("button", { class: "btn small", "aria-label": `${t("queue.retry")}: ${name}`, onclick: async () => {
          await api.retryJob(job.id).catch(showError);
          state.jobs.delete(job.id);
          render();
        } }, t("queue.retry")) : null,
      job.status === "done"
        ? h("button", { class: "btn small", "aria-label": `${t("queue.openFolder")}: ${name}`, onclick: () => revealItemInDir(location).catch(showError) }, t("queue.openFolder")) : null,
      !["queued", "running"].includes(job.status)
        ? h("button", { class: "icon-btn", "aria-label": `${t("queue.remove")}: ${name}`, onclick: async () => {
          await api.removeJob(job.id);
          state.jobs.delete(job.id);
          render();
        } }, "✕") : null));
}

export function updateJob(job: Job) {
  const prev = state.jobs.get(job.id);
  state.jobs.set(job.id, job);
  const row = document.getElementById(`job-${job.id}`);
  const statusChanged = !prev || prev.status !== job.status;
  if (row && !statusChanged) {
    // Cheap in-place progress update.
    row.replaceWith(jobRow(job));
  } else {
    render();
  }
  if (statusChanged && job.status === "done") toast(t("queue.doneToast", { name: jobInputLabel(job) }), "success");
  if (statusChanged && job.status === "failed") toast(t("queue.failedToast", { name: jobInputLabel(job) }), "error");
  updateTaskbar();
}

let lastTaskbar = -2;

/** Mirror overall queue progress on the taskbar / dock icon. */
export function updateTaskbar() {
  const active = activeJobs();
  const win = getCurrentWindow();
  if (!state.queueRunning || !active.length) {
    if (lastTaskbar !== -1) win.setProgressBar({ status: ProgressBarStatus.None }).catch(() => {});
    lastTaskbar = -1;
    return;
  }
  // Average over the jobs of this batch that are still open plus finished ones
  // would need batch tracking; the active jobs' mean is a good, stable proxy.
  const pct = Math.round((active.reduce((s, j) => s + j.progress, 0) / active.length) * 100);
  if (pct === lastTaskbar) return;
  lastTaskbar = pct;
  win.setProgressBar({ status: ProgressBarStatus.Normal, progress: pct }).catch(() => {});
}

/** Tell the user a batch finished when they are looking at another window. */
export async function notifyQueueDone() {
  const failed = [...state.jobs.values()].filter((j) => j.status === "failed").length;
  const body = failed ? t("queue.notifyFailed", { n: failed }) : t("queue.allDone");
  if (failed) getCurrentWindow().requestUserAttention(UserAttentionType.Informational).catch(() => {});
  if (document.hasFocus()) return;
  try {
    let ok = await isPermissionGranted();
    if (!ok) ok = (await requestPermission()) === "granted";
    if (ok) sendNotification({ title: "ConvertHub", body });
  } catch {
    /* notifications unavailable: the in-app toast is enough */
  }
}
