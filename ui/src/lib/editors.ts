// Editor presets for "Open with". Must match ZED / VSCODE in src-tauri/src/config.rs.
export const ZED = "zed {path}:{line}:{col}";
export const VSCODE = "code -g {path}:{line}:{col}";
/** The presets before `{col}`; config.rs reads a stored one as its successor. */
const OLD_ZED = "zed {path}:{line}";
const OLD_VSCODE = "code -g {path}:{line}";

/** The app's settings (src-tauri/src/config.rs `Config`). */
export type EditorConfig = {
  editor: string;
  customEditor?: string | null;
  readOnly?: boolean;
  /** The roots on the rail, in the user's order; only they count in the header total and the tray. */
  workspace?: string[];
  /** Roots the app added (`projects[root].addedByApp`): marked "Added here" and removable. */
  appRoots?: string[];
};

const norm = (t: string) => t.trim().split(/\s+/).join(" ");

/** A short name for the editor a template runs: the preset's name, else the program's base name. */
export function editorLabel(template: string): string {
  const t = norm(template);
  if (t === ZED || t === OLD_ZED) return "Zed";
  if (t === VSCODE || t === OLD_VSCODE) return "VS Code";
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

/**
 * The acknowledgement after a file was handed to the editor: it opens in another window, often
 * behind this one, so a click on Open would otherwise look like nothing happened.
 */
export function openedText(path: string, line: number, template: string): string {
  const name = path.replace(/\\/g, "/").split("/").pop() || path;
  return `Opened ${name}${line > 1 ? `:${line}` : ""} in ${editorLabel(template)}`;
}
