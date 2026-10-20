// Small UI building blocks shared by every view.

import { open } from "@tauri-apps/plugin-dialog";
import { h } from "./dom";
import { has, safeSet, t, tm } from "./i18n";
import { render, state } from "./state";

export function toast(text: string, kind: "info" | "error" | "success" = "info") {
  const region = document.getElementById("toasts");
  if (!region) return;
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

export function showError(e: unknown) {
  toast(tm(e), "error");
}

export function optLabel(key: string, value: string) {
  const k = `options.${key}.${value}`;
  return has(k) ? t(k) : value.toUpperCase();
}

export function modal(title: string, body: Node) {
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

export async function chooseOutputDir(): Promise<string | null> {
  const dir = await open({ directory: true, defaultPath: state.outputDir || undefined, title: t("output.choose") });
  if (typeof dir === "string") {
    state.outputDir = dir;
    safeSet("converthub.outputDir", dir);
    return dir;
  }
  return null;
}

export function outputRow() {
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
