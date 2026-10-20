// Global keyboard shortcuts and their help dialog.

import { SECTIONS } from "./defs";
import { h } from "./dom";
import { t } from "./i18n";
import { navigate, type View } from "./state";
import { modal } from "./ui";
import { toggleQueue } from "./views/queue";
import { enqueueCurrent, pickInputs } from "./views/tool";

const OTHER: View[] = ["queue", "recent", "formats", "system", "settings"];

interface Shortcut {
  keys: string;
  labelKey: string;
  vars?: Record<string, string>;
}

export const SHORTCUTS: Shortcut[] = [
  { keys: "Ctrl+O", labelKey: "shortcuts.addFiles" },
  { keys: "Ctrl+Enter", labelKey: "shortcuts.enqueue" },
  { keys: "Ctrl+Shift+S", labelKey: "shortcuts.toggleQueue" },
  { keys: "Alt+1 … Alt+8", labelKey: "shortcuts.sections" },
  { keys: "Alt+Q", labelKey: "shortcuts.goto", vars: { view: "queue" } },
  { keys: "Alt+R", labelKey: "shortcuts.goto", vars: { view: "recent" } },
  { keys: "Alt+S", labelKey: "shortcuts.goto", vars: { view: "settings" } },
  { keys: "F1", labelKey: "shortcuts.help" },
];

export function showShortcuts() {
  modal(t("shortcuts.title"), h("table", { class: "kv" },
    h("tbody", {}, ...SHORTCUTS.map((s) => h("tr", {},
      h("th", { scope: "row" }, h("kbd", {}, s.keys)),
      h("td", {}, t(s.labelKey, s.vars ? { view: t(`nav.${s.vars.view}`) } : {})))))));
}

/** Pure key-to-action mapping, exported for tests. */
export function actionFor(e: Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">): string | null {
  const mod = e.ctrlKey || e.metaKey;
  if (e.key === "F1") return "help";
  if (mod && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "o") return "add";
  if (mod && e.key === "Enter") return "enqueue";
  if (mod && e.shiftKey && e.key.toLowerCase() === "s") return "toggle";
  if (e.altKey && !mod) {
    const digit = /^Digit([1-9])$/.exec(e.code)?.[1];
    if (digit && Number(digit) <= SECTIONS.length) return `view:${SECTIONS[Number(digit) - 1]}`;
    const letter = { KeyQ: "queue", KeyR: "recent", KeyS: "settings" }[e.code];
    if (letter && OTHER.includes(letter as View)) return `view:${letter}`;
  }
  return null;
}

export function installShortcuts() {
  document.addEventListener("keydown", (e) => {
    // Leave typing alone, except for the explicit Ctrl/F1 combinations.
    const action = actionFor(e);
    if (!action) return;
    if (document.querySelector("dialog[open]") && action !== "help") return;
    e.preventDefault();
    if (action === "help") showShortcuts();
    else if (action === "add") pickInputs();
    else if (action === "enqueue") enqueueCurrent();
    else if (action === "toggle") toggleQueue();
    else if (action.startsWith("view:")) navigate(action.slice(5) as View);
  });
}
