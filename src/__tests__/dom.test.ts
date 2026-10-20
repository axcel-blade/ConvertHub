import { describe, expect, it } from "vitest";
import { basename, dirname, extOf, fmtBytes, fmtDuration } from "../dom";

describe("path helpers", () => {
  it("handles both separators", () => {
    expect(basename(String.raw`C:\videos\a.mp4`)).toBe("a.mp4");
    expect(basename("/home/u/b.mkv")).toBe("b.mkv");
    expect(dirname(String.raw`C:\videos\a.mp4`)).toBe(String.raw`C:\videos`);
    expect(dirname("/home/u/b.mkv")).toBe("/home/u");
  });

  it("extracts lowercase extensions, ignoring dotfiles", () => {
    expect(extOf("Clip.MP4")).toBe("mp4");
    expect(extOf("archive.tar.gz")).toBe("gz");
    expect(extOf(".hidden")).toBe("");
    expect(extOf("noext")).toBe("");
  });
});

describe("formatting", () => {
  it("formats bytes", () => {
    expect(fmtBytes(0)).toBe("0 B");
    expect(fmtBytes(1536)).toBe("1.5 KB");
    expect(fmtBytes(5 * 1024 ** 3)).toBe("5.0 GB");
  });

  it("formats durations", () => {
    expect(fmtDuration(0)).toBe("00:00");
    expect(fmtDuration(65.4)).toBe("01:05");
    expect(fmtDuration(3725)).toBe("1:02:05");
    expect(fmtDuration(-3)).toBe("00:00");
  });
});
