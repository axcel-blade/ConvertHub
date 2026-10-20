import { beforeEach, describe, expect, it } from "vitest";
import { OPS } from "../defs";
import { deleteUserPreset, loadLastParams, persistable, saveLastParams, saveUserPreset, userPresets } from "../prefs";

const store = new Map<string, string>();
globalThis.localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => void store.set(k, v),
  removeItem: (k: string) => void store.delete(k),
  clear: () => store.clear(),
  key: () => null,
  length: 0,
} as Storage;

const delogo = OPS.find((o) => o.id === "video_delogo")!;
const mux = OPS.find((o) => o.id === "video_mux")!;
const join = OPS.find((o) => o.id === "audio_join")!;

beforeEach(() => store.clear());

describe("prefs", () => {
  it("never keeps authorization boxes or file paths", () => {
    expect(persistable(delogo, { x: 1, authorized: true })).toEqual({ x: 1 });
    expect(persistable(mux, { audio: "C:/a.mp3" })).toEqual({});
  });

  it("remembers last-used settings per operation", () => {
    saveLastParams(join, { format: "flac", unknown: 1 });
    expect(loadLastParams(join)).toEqual({ format: "flac" });
  });

  it("ignores corrupt storage", () => {
    store.set("converthub.last.audio_join", "{not json");
    expect(loadLastParams(join)).toEqual({});
  });

  it("saves and deletes named presets", () => {
    saveUserPreset(join, "Archive", { format: "flac" });
    saveUserPreset(join, "Phone", { format: "m4a" });
    expect(Object.keys(userPresets(join)).sort()).toEqual(["Archive", "Phone"]);
    deleteUserPreset(join, "Archive");
    expect(userPresets(join)).toEqual({ Phone: { format: "m4a" } });
  });
});
