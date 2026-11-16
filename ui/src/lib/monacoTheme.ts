// Monaco themes for the diff: Dracula in dark mode and Alucard (Dracula's light variant) in light,
// with Dracula's VS Code diff tints, so a diff reads the way it does in the maintainer's editor.
// Plain data and one pure builder; the DiffView only picks the mode.

export interface ThemeRule {
  token: string;
  /** Monaco wants `rrggbb`, no `#`. */
  foreground?: string;
  fontStyle?: string;
}

export interface Theme {
  base: "vs" | "vs-dark";
  inherit: boolean;
  rules: ThemeRule[];
  colors: Record<string, string>;
}

/** The hue roles one theme fills. */
interface Palette {
  background: string;
  backgroundDarker: string;
  foreground: string;
  comment: string;
  cyan: string;
  green: string;
  orange: string;
  pink: string;
  purple: string;
  red: string;
  yellow: string;
  selection: string;
  currentLine: string;
  lineNumber: string;
  lineNumberActive: string;
  scrollbar: string;
  guide: string;
  diff: {
    insertedText: string;
    removedText: string;
    insertedLine: string;
    removedLine: string;
    insertedGutter: string;
    removedGutter: string;
    insertedRuler: string;
    removedRuler: string;
    diagonalFill: string;
  };
}

export const DRACULA: Palette = {
  background: "#282a36",
  backgroundDarker: "#21222c",
  foreground: "#f8f8f2",
  comment: "#6272a4",
  cyan: "#8be9fd",
  green: "#50fa7b",
  orange: "#ffb86c",
  pink: "#ff79c6",
  purple: "#bd93f9",
  red: "#ff5555",
  yellow: "#f1fa8c",
  selection: "#44475a",
  currentLine: "#44475a",
  lineNumber: "#6272a4",
  lineNumberActive: "#f8f8f2",
  scrollbar: "#6272a4",
  guide: "#ffffff1a",
  diff: {
    insertedText: "#50fa7b20",
    removedText: "#ff555550",
    insertedLine: "#50fa7b14",
    removedLine: "#ff555530",
    insertedGutter: "#50fa7b30",
    removedGutter: "#ff555548",
    insertedRuler: "#50fa7b80",
    removedRuler: "#ff555580",
    diagonalFill: "#44475a80",
  },
};

export const ALUCARD: Palette = {
  background: "#f8f8f2",
  backgroundDarker: "#efeee6",
  foreground: "#1f1f1f",
  comment: "#6c664b",
  cyan: "#036a96",
  green: "#14710a",
  orange: "#a34d14",
  pink: "#a3144d",
  purple: "#644ac9",
  red: "#cb3a2a",
  yellow: "#846e15",
  selection: "#cfcfde",
  currentLine: "#e6e5dc",
  lineNumber: "#9e9a87",
  lineNumberActive: "#1f1f1f",
  scrollbar: "#6c664b",
  guide: "#0000001a",
  diff: {
    insertedText: "#9ccc2c40",
    removedText: "#ff000033",
    insertedLine: "#9bb95533",
    removedLine: "#ff00001a",
    insertedGutter: "#9bb95548",
    removedGutter: "#ff00002e",
    insertedRuler: "#9bb955cc",
    removedRuler: "#ff000099",
    diagonalFill: "#22222233",
  },
};

const hex = (c: string) => c.slice(1, 7);

/** Token roles as the Dracula VS Code theme assigns them, keyed by Monarch token prefixes. */
function rules(p: Palette): ThemeRule[] {
  const r = (token: string, color: string, fontStyle?: string): ThemeRule =>
    fontStyle ? { token, foreground: hex(color), fontStyle } : { token, foreground: hex(color) };
  return [
    r("", p.foreground),
    r("comment", p.comment),
    r("comment.doc", p.comment),
    r("keyword", p.pink),
    r("keyword.flow", p.pink),
    r("storage", p.pink),
    r("operator", p.pink),
    r("string", p.yellow),
    r("string.escape", p.pink),
    r("string.key", p.cyan),
    r("key", p.cyan),
    r("regexp", p.red),
    r("number", p.purple),
    r("constant", p.purple),
    r("variable.predefined", p.purple, "italic"),
    r("predefined", p.cyan),
    r("type", p.cyan, "italic"),
    r("type.identifier", p.cyan, "italic"),
    r("class", p.cyan, "italic"),
    r("namespace", p.cyan, "italic"),
    r("function", p.green),
    r("entity.name.function", p.green),
    r("parameter", p.orange, "italic"),
    r("variable.parameter", p.orange, "italic"),
    r("annotation", p.green),
    r("tag", p.pink),
    r("metatag", p.pink),
    r("attribute.name", p.green),
    r("attribute.value", p.yellow),
    r("delimiter", p.foreground),
    r("identifier", p.foreground),
    r("variable", p.foreground),
    r("invalid", p.red),
    r("emphasis", p.yellow, "italic"),
    r("strong", p.orange, "bold"),
    r("markup.heading", p.purple, "bold"),
  ];
}

function colors(p: Palette): Record<string, string> {
  const brackets = [p.foreground, p.pink, p.cyan, p.green, p.purple, p.orange];
  return {
    "editor.background": p.background,
    "editor.foreground": p.foreground,
    "editorGutter.background": p.background,
    "editorLineNumber.foreground": p.lineNumber,
    "editorLineNumber.activeForeground": p.lineNumberActive,
    "editor.selectionBackground": p.selection,
    "editor.inactiveSelectionBackground": `${p.selection}80`,
    "editor.lineHighlightBackground": "#00000000",
    "editor.lineHighlightBorder": p.currentLine,
    "editorCursor.foreground": p.foreground,
    "editorIndentGuide.background1": p.guide,
    "editorWhitespace.foreground": p.guide,
    "editorWidget.background": p.backgroundDarker,
    "editorHoverWidget.background": p.backgroundDarker,
    "editorBracketMatch.border": p.comment,
    "scrollbarSlider.background": `${p.scrollbar}40`,
    "scrollbarSlider.hoverBackground": `${p.scrollbar}70`,
    "scrollbarSlider.activeBackground": `${p.scrollbar}90`,
    "editorOverviewRuler.border": p.backgroundDarker,
    "editorOverviewRuler.addedForeground": p.diff.insertedRuler,
    "editorOverviewRuler.deletedForeground": p.diff.removedRuler,
    "editorOverviewRuler.modifiedForeground": `${p.cyan}80`,
    "diffEditor.insertedTextBackground": p.diff.insertedText,
    "diffEditor.removedTextBackground": p.diff.removedText,
    "diffEditor.insertedLineBackground": p.diff.insertedLine,
    "diffEditor.removedLineBackground": p.diff.removedLine,
    "diffEditorGutter.insertedLineBackground": p.diff.insertedGutter,
    "diffEditorGutter.removedLineBackground": p.diff.removedGutter,
    "diffEditorOverview.insertedForeground": p.diff.insertedRuler,
    "diffEditorOverview.removedForeground": p.diff.removedRuler,
    "diffEditor.diagonalFill": p.diff.diagonalFill,
    "diffEditor.unchangedRegionBackground": p.backgroundDarker,
    "diffEditor.unchangedRegionForeground": p.comment,
    "diffEditor.unchangedCodeBackground": `${p.selection}40`,
    ...Object.fromEntries(brackets.map((c, i) => [`editorBracketHighlight.foreground${i + 1}`, c])),
  };
}

/** The Monaco theme for one mode: Dracula when `dark`, Alucard otherwise. */
export function buildTheme(dark: boolean): Theme {
  const p = dark ? DRACULA : ALUCARD;
  return { base: dark ? "vs-dark" : "vs", inherit: true, rules: rules(p), colors: colors(p) };
}

export const themeName = (dark: boolean) => (dark ? "claudegate-dracula" : "claudegate-alucard");
