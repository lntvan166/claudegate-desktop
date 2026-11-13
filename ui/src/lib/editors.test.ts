import { describe, expect, it } from "vitest";
import { editorLabel, menuOptions, VSCODE, ZED } from "./editors";

describe("editors", () => {
  it("uses the brief's preset templates", () => {
    expect(ZED).toBe("zed {path}:{line}");
    expect(VSCODE).toBe("code -g {path}:{line}");
  });

  it("names presets, and anything else by its program", () => {
    expect(editorLabel(ZED)).toBe("Zed");
    expect(editorLabel(VSCODE)).toBe("VS Code");
    expect(editorLabel("  zed  {path}:{line} ")).toBe("Zed");
    expect(editorLabel("/opt/sublime/subl {path}:{line}")).toBe("subl");
    expect(editorLabel("C:\\Tools\\hx.exe {path}")).toBe("hx");
    expect(editorLabel("")).toBe("editor");
  });

  it("lists Zed and VS Code, and Custom only when one is configured", () => {
    expect(menuOptions({ editor: ZED }).map((o) => o.label)).toEqual(["Zed", "VS Code"]);
    const opts = menuOptions({ editor: "nvim +{line} {path}", customEditor: "nvim +{line} {path}" });
    expect(opts.map((o) => o.label)).toEqual(["Zed", "VS Code", "Custom…"]);
    expect(opts.map((o) => o.current)).toEqual([false, false, true]);
    expect(menuOptions({ editor: VSCODE }).find((o) => o.current)?.label).toBe("VS Code");
  });
});
