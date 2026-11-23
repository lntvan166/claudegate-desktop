import { describe, expect, it } from "vitest";
import { editorLabel, menuOptions, openedText, VSCODE, ZED } from "./editors";

describe("editors", () => {
  it("uses the brief's preset templates", () => {
    expect(ZED).toBe("zed {path}:{line}:{col}");
    expect(VSCODE).toBe("code -g {path}:{line}:{col}");
  });

  it("names presets, and anything else by its program", () => {
    expect(editorLabel(ZED)).toBe("Zed");
    expect(editorLabel(VSCODE)).toBe("VS Code");
    expect(editorLabel("  zed  {path}:{line}:{col} ")).toBe("Zed");
    // The presets before {col} still read as their editor.
    expect(editorLabel("zed {path}:{line}")).toBe("Zed");
    expect(editorLabel("code -g {path}:{line}")).toBe("VS Code");
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

describe("openedText", () => {
  it("names the file, the line and the editor", () => {
    expect(openedText("/r/src/api/orders.ts", 11, ZED)).toBe("Opened orders.ts:11 in Zed");
    expect(openedText("C:\\r\\main.go", 4, VSCODE)).toBe("Opened main.go:4 in VS Code");
  });
  it("leaves out line 1", () => {
    expect(openedText("/r/a.md", 1, "nvim +{line} {path}")).toBe("Opened a.md in nvim");
  });
});
