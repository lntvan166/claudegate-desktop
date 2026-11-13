import { describe, expect, it } from "vitest";
import { relTime } from "./time";

describe("relTime", () => {
  const now = Date.parse("2026-11-13T12:00:00Z");
  it("buckets recent times", () => {
    expect(relTime("2026-11-13T11:59:50Z", now)).toBe("just now");
    expect(relTime("2026-11-13T11:55:00Z", now)).toBe("5m ago");
    expect(relTime("2026-11-13T09:00:00Z", now)).toBe("3h ago");
    expect(relTime("2026-11-11T12:00:00Z", now)).toBe("2d ago");
  });
  it("returns empty for garbage", () => {
    expect(relTime("not a date", now)).toBe("");
  });
});
