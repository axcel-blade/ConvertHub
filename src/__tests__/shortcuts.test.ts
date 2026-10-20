import { describe, expect, it } from "vitest";
import { actionFor } from "../shortcuts";

const key = (k: Partial<KeyboardEvent>) =>
  ({ key: "", code: "", ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...k }) as KeyboardEvent;

describe("keyboard shortcuts", () => {
  it("maps the documented combinations", () => {
    expect(actionFor(key({ key: "o", ctrlKey: true }))).toBe("add");
    expect(actionFor(key({ key: "o", metaKey: true }))).toBe("add");
    expect(actionFor(key({ key: "Enter", ctrlKey: true }))).toBe("enqueue");
    expect(actionFor(key({ key: "S", ctrlKey: true, shiftKey: true }))).toBe("toggle");
    expect(actionFor(key({ key: "F1" }))).toBe("help");
    expect(actionFor(key({ code: "Digit1", altKey: true }))).toBe("view:video");
    expect(actionFor(key({ code: "Digit8", altKey: true }))).toBe("view:archive");
    expect(actionFor(key({ code: "KeyQ", altKey: true }))).toBe("view:queue");
  });

  it("ignores plain typing", () => {
    expect(actionFor(key({ key: "o" }))).toBeNull();
    expect(actionFor(key({ key: "Enter" }))).toBeNull();
    expect(actionFor(key({ code: "Digit9", altKey: true }))).toBeNull();
  });
});
