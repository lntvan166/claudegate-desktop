import { describe, expect, it } from "vitest";
import { chipLabel, summary, type UpdateView } from "./update";

const u = (over: Partial<UpdateView> = {}): UpdateView => ({
  version: "0.3.0",
  current: "0.2.0",
  notes: null,
  canInstall: true,
  command: null,
  ...over,
});

describe("update chip", () => {
  it("names the new version", () => {
    expect(chipLabel(u())).toBe("Update 0.3.0");
  });
  it("says installing restarts when the app can replace itself", () => {
    expect(summary(u())).toBe("ClaudeGate 0.3.0 is available (you have 0.2.0). Installing restarts the app.");
  });
  it("points a .deb install at the command", () => {
    expect(summary(u({ canInstall: false, command: "curl …" }))).toContain("came from a .deb package");
  });
});
