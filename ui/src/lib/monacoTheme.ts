// Monaco themes built from theme.css tokens, so the diff sits on the same sheet as the tree.

/** `#rgb`, `#rrggbb`, `rgb()` or `rgba()` as Monaco's `#rrggbb[aa]`; `alpha` overrides any alpha. */
export function toHex(color: string, alpha?: number): string | undefined {
  const c = color.trim().toLowerCase();
  let r: number, g: number, b: number, a: number | undefined;
  let m: RegExpMatchArray | null;
  if ((m = c.match(/^#([0-9a-f]{3})$/))) {
    [r, g, b] = [...m[1]].map((h) => parseInt(h + h, 16));
  } else if ((m = c.match(/^#([0-9a-f]{6})([0-9a-f]{2})?$/))) {
    r = parseInt(m[1].slice(0, 2), 16);
    g = parseInt(m[1].slice(2, 4), 16);
    b = parseInt(m[1].slice(4, 6), 16);
    if (m[2]) a = parseInt(m[2], 16) / 255;
  } else if ((m = c.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+)\s*)?\)$/))) {
    [r, g, b] = [m[1], m[2], m[3]].map(Number);
    if (m[4] !== undefined) a = Number(m[4]);
  } else {
    return undefined;
  }
  if (alpha !== undefined) a = alpha;
  const h = (n: number) => Math.round(Math.min(255, Math.max(0, n))).toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}${a === undefined ? "" : h(a * 255)}`;
}

/** Workbench colours for the diff editor from the app's CSS custom properties. */
export function themeColors(read: (name: string) => string, dark: boolean): Record<string, string> {
  const t = (name: string, alpha?: number) => toHex(read(name), alpha);
  const line = dark ? 0.09 : 0.07;
  const word = dark ? 0.16 : 0.14;
  const entries: [string, string | undefined][] = [
    ["editor.background", t("--surface")],
    ["editor.foreground", t("--text")],
    ["editorGutter.background", t("--surface")],
    ["editorLineNumber.foreground", t("--faint", 0.7)],
    ["editorLineNumber.activeForeground", t("--muted")],
    ["editor.lineHighlightBackground", "#00000000"],
    ["editor.lineHighlightBorder", "#00000000"],
    ["editor.selectionBackground", t("--active")],
    ["editor.inactiveSelectionBackground", t("--hover")],
    ["editorIndentGuide.background1", t("--guide")],
    ["editorWidget.background", t("--surface")],
    ["editorWidget.border", t("--border")],
    ["editorHoverWidget.background", t("--surface")],
    ["editorHoverWidget.border", t("--border")],
    ["input.background", t("--raised")],
    ["input.border", t("--border")],
    ["focusBorder", t("--focus-ring")],
    ["diffEditor.insertedLineBackground", t("--positive", line)],
    ["diffEditor.insertedTextBackground", t("--positive", word)],
    ["diffEditor.removedLineBackground", t("--negative", line)],
    ["diffEditor.removedTextBackground", t("--negative", word)],
    ["diffEditorGutter.insertedLineBackground", t("--positive", line)],
    ["diffEditorGutter.removedLineBackground", t("--negative", line)],
    ["diffEditorOverview.insertedForeground", t("--positive", 0.6)],
    ["diffEditorOverview.removedForeground", t("--negative", 0.6)],
    ["diffEditor.border", t("--rule")],
    ["diffEditor.diagonalFill", t("--rule")],
    ["diffEditor.unchangedRegionBackground", t("--raised")],
    ["diffEditor.unchangedRegionForeground", t("--muted")],
    ["diffEditor.unchangedCodeBackground", t("--raised")],
    ["editorOverviewRuler.border", "#00000000"],
    ["scrollbar.shadow", "#00000000"],
    ["scrollbarSlider.background", t("--scrollbar")],
    ["scrollbarSlider.hoverBackground", t("--scrollbar")],
    ["scrollbarSlider.activeBackground", t("--scrollbar")],
  ];
  // Brackets in the text colour: colourised pairs would compete with the diff tints.
  for (let i = 1; i <= 6; i++) entries.push([`editorBracketHighlight.foreground${i}`, t("--text")]);
  return Object.fromEntries(entries.filter((e): e is [string, string] => !!e[1]));
}
