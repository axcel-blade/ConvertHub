import { describe, expect, it } from "vitest";
import { has, t, tm } from "../i18n";

describe("i18n", () => {
  it("interpolates variables and leaves unknown ones", () => {
    expect(t("queue.start", { n: 3 })).toBe("Start (3)");
    expect(t("queue.start")).toBe("Start ({n})");
  });

  it("falls back to the key for missing strings", () => {
    expect(t("does.not.exist")).toBe("does.not.exist");
    expect(has("does.not.exist")).toBe(false);
    expect(has("nav.video")).toBe(true);
  });

  it("renders backend messages and unknown errors", () => {
    expect(tm({ key: "queue.doneToast", vars: { name: "a.mp4" } })).toBe("Finished: a.mp4");
    expect(tm("boom")).toContain("boom");
  });
});
