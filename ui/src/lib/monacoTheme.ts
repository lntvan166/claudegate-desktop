// Monaco themes for the diff. Token colours come from Shiki's TextMate themes (Dracula in dark,
// GitHub Light in light), scope for scope as VS Code applies them; the editor chrome and the diff
// tints are ours and win over the TextMate theme's: Dracula's in dark, Alucard's ground in light.
// Plain data and one pure builder; the DiffView only picks the mode.

import { textmateThemeToMonacoTheme } from "@shikijs/monaco";

export interface ThemeRule {
  token: string;
  /** Monaco wants `rrggbb`, no `#`. */
  foreground?: string;
  background?: string;
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

/** A TextMate theme's rules and colours, as @shikijs/monaco converts them for Monaco. */
export interface TextmateTheme {
  rules: ThemeRule[];
  colors: Record<string, string>;
}

/** @shikijs/monaco's conversion, typed here: its declarations point at `monaco-editor-core`. */
export function fromTextmate(theme: unknown): TextmateTheme {
  return textmateThemeToMonacoTheme(theme as Parameters<typeof textmateThemeToMonacoTheme>[0]) as unknown as TextmateTheme;
}

/**
 * Comments in a shade that reads at 4.5:1 on the diff's ground. Dracula's #6272a4 is 3.0:1 on
 * #282a36; GitHub Light's #6a737d is 4.5:1 on white but only just on Alucard's #f8f8f2, and less
 * on a tinted diff line. Only the comment colour changes; every other token keeps its own.
 */
const COMMENT = { dark: "8a98c9", light: "5c6570" } as const;

/** The TextMate theme's comment colour (`from`, as Shiki paints it) and the one we paint (`to`). */
export function commentOverride(dark: boolean, shiki: TextmateTheme): { from: string; to: string } {
  const from = (shiki.rules.find((r) => r.token === "comment")?.foreground ?? "").toLowerCase();
  return { from, to: dark ? COMMENT.dark : COMMENT.light };
}

/** The Monaco theme for one mode: `shiki` (a TextMate theme converted by @shikijs/monaco) for the
 *  tokens, with our chrome and diff colours laid over its own and comments lightened. */
export function buildTheme(dark: boolean, shiki: TextmateTheme): Theme {
  const p = dark ? DRACULA : ALUCARD;
  const c = commentOverride(dark, shiki);
  const rules = shiki.rules.map((r) => (c.from && r.foreground?.toLowerCase() === c.from ? { ...r, foreground: c.to } : r));
  // Not inherited: every token colour is the TextMate theme's, none leaks in from Monaco's vs/vs-dark.
  return { base: dark ? "vs-dark" : "vs", inherit: false, rules, colors: { ...shiki.colors, ...colors(p) } };
}

export const themeName = (dark: boolean) => (dark ? "claudegate-dracula" : "claudegate-light");
