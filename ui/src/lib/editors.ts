// Editor presets for "Open with". Must match ZED / VSCODE in src-tauri/src/config.rs.
export const ZED = "zed {path}:{line}";
export const VSCODE = "code -g {path}:{line}";

export type EditorConfig = { editor: string; customEditor?: string | null };

const norm = (t: string) => t.trim().split(/\s+/).join(" ");

/** A short name for the editor a template runs: the preset's name, else the program's base name. */
export function editorLabel(template: string): string {
  const t = norm(template);
  if (t === ZED) return "Zed";
  if (t === VSCODE) return "VS Code";
  const program = t.split(" ")[0] ?? "";
  const base = program.split(/[\\/]/).pop() ?? "";
  return base.replace(/\.(exe|cmd|bat)$/i, "") || "editor";
}

export type EditorOption = { label: string; template: string; current: boolean };

/** The Open-with menu: Zed, VS Code, and Custom… when the config names one. */
export function menuOptions(config: EditorConfig): EditorOption[] {
  const cur = norm(config.editor);
  const opts: { label: string; template: string }[] = [
    { label: "Zed", template: ZED },
    { label: "VS Code", template: VSCODE },
  ];
  const custom = config.customEditor?.trim();
  if (custom) opts.push({ label: "Custom…", template: custom });
  return opts.map((o) => ({ ...o, current: norm(o.template) === cur }));
}
