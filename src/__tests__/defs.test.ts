import { describe, expect, it } from "vitest";
import { OPS, SECTIONS, backendOp, defaults, opsFor, toOperation } from "../defs";

const op = (id: string) => {
  const o = OPS.find((x) => x.id === id);
  if (!o) throw new Error(`no op ${id}`);
  return o;
};

describe("operation definitions", () => {
  it("have unique ids and valid sections", () => {
    const ids = OPS.map((o) => o.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const o of OPS) expect(SECTIONS).toContain(o.section);
  });

  it("every file-based section has at least one op", () => {
    for (const s of SECTIONS.filter((s) => s !== "download" && s !== "recorder")) {
      expect(opsFor(s).length).toBeGreaterThan(0);
    }
  });

  it("defaults only include fields with a default", () => {
    const d = defaults(op("audio_split"));
    expect(d).toEqual({ segmentSeconds: 600 });
  });
});

describe("toOperation", () => {
  it("maps UI ids to backend ops", () => {
    expect(backendOp("audio_tags")).toBe("media_tags");
    expect(toOperation(op("audio_tags"), {}).op).toBe("media_tags");
  });

  it("converts numbers and drops empty values", () => {
    const out = toOperation(op("audio_split"), { segmentSeconds: "30" });
    expect(out).toEqual({ op: "audio_split", segmentSeconds: 30 });
    expect(toOperation(op("audio_trim"), { start: "5", end: "" })).toEqual({ op: "audio_trim", start: "5" });
  });

  it("keeps format as a string even when numeric-looking", () => {
    const out = toOperation(op("audio_join"), { format: "mp3" });
    expect(out.format).toBe("mp3");
  });

  it("forces mp4 when a device preset is chosen", () => {
    const out = toOperation(op("video_convert"), { ...defaults(op("video_convert")), preset: "iphone", format: "mkv" });
    expect(out.format).toBe("mp4");
  });
});
