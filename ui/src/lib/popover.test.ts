import { describe, expect, it } from "vitest";
import { popoverLeft } from "./popover";

describe("popoverLeft", () => {
  it("right-aligns with the chip when there is room", () => {
    // chip ends at 1200, wrapper starts at 1100: a 340px popover starts 860 → -240 from the wrapper.
    expect(popoverLeft({ right: 1200 }, 1100, 340, 1440)).toBe(-240);
  });
  it("stays 16px inside the left edge", () => {
    // chip ends at 300 in an 800px window: 300 - 380 would be -80.
    expect(popoverLeft({ right: 300 }, 220, 380, 800)).toBe(16 - 220);
  });
  it("stays 16px inside the right edge", () => {
    expect(popoverLeft({ right: 795 }, 700, 340, 800)).toBe(800 - 16 - 340 - 700);
  });
  it("narrows to the window on a phone", () => {
    // 390px window: the popover is 358px wide, so it starts at the margin.
    expect(popoverLeft({ right: 250 }, 120, 380, 390)).toBe(16 - 120);
  });
});
