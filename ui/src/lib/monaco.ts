// The Monaco build the diff view uses: the core editor API, the diff editor and a few editor
// features, and the editor worker. Colours come from Shiki (grammars.ts), not Monaco's Monarch
// grammars: each language's TextMate grammar loads on the first diff in it. Everything is bundled by
// Vite; nothing is fetched at runtime. Loaded lazily by DiffView, so it stays out of the first paint.
import * as monaco from "monaco-editor/editor/editor.api.js";
import "monaco-editor/features/diffEditor/register.js";
import "monaco-editor/features/codicon/register.js";
import "monaco-editor/features/find/register.js";
import "monaco-editor/features/folding/register.js";
import "monaco-editor/features/clipboard/register.js";
import "monaco-editor/features/contextmenu/register.js";
import "monaco-editor/features/bracketMatching/register.js";
// The blame decoration's hover card (a diff has no other hover providers).
import "monaco-editor/features/hover/register.js";
import EditorWorker from "monaco-editor/editor/editor.worker.js?worker";
import { EncodedTokenMetadata, INITIAL, type StateStack } from "@shikijs/vscode-textmate";
import { GRAMMARS, SHIKI_THEME, createDiffHighlighter } from "./grammars";
import { LANGUAGES } from "./language";
import { buildTheme, commentOverride, fromTextmate, themeName } from "./monacoTheme";

// The diff runs in the editor worker. No language service runs at all: a read-only diff needs
// colours, not completions or validation.
self.MonacoEnvironment = {
  getWorker() {
    return new EditorWorker();
  },
};

const highlighter = await createDiffHighlighter();

// Our two themes: the TextMate theme's token rules, our chrome and diff tints.
const textmate = {
  dark: fromTextmate(highlighter.getTheme(SHIKI_THEME.dark)),
  light: fromTextmate(highlighter.getTheme(SHIKI_THEME.light)),
};
const monacoThemes = {
  dark: buildTheme(true, textmate.dark),
  light: buildTheme(false, textmate.light),
};
/** Shiki still paints comments its own colour; read it as ours so the token finds its rule. */
const comments = { dark: commentOverride(true, textmate.dark), light: commentOverride(false, textmate.light) };
monaco.editor.defineTheme(themeName(true), monacoThemes.dark);
monaco.editor.defineTheme(themeName(false), monacoThemes.light);

// Shiki tokenizes to colours from its own colour map; Monaco wants token names it resolves against
// the theme. As @shikijs/monaco does, each colour+style is mapped back to the first theme rule that
// produces it, so Monaco paints the same colour. Rebuilt whenever the theme changes.
let colorMap: string[] = [];
const scopeFor = new Map<string, string>();

// vscode-textmate's FontStyle bits (a const enum, which isolated modules cannot read).
const FontStyle = { Italic: 1, Bold: 2, Underline: 4, Strikethrough: 8 } as const;

const norm = (c: string | undefined) => (c ? c.replace(/^#/, "").toLowerCase() : "");
function styleKey(color: string, bits: number): string {
  const s = [
    bits & FontStyle.Italic ? "italic" : "",
    bits > 0 && bits & FontStyle.Bold ? "bold" : "",
    bits > 0 && bits & FontStyle.Underline ? "underline" : "",
    bits > 0 && bits & FontStyle.Strikethrough ? "strikethrough" : "",
  ].filter(Boolean);
  return bits > 0 && s.length ? `${color}|${s.join(" ")}` : color;
}
function ruleStyleBits(fontStyle: string | undefined): number {
  let b = 0;
  for (const w of (fontStyle ?? "").split(/[\s,]+/)) {
    if (w === "italic") b |= FontStyle.Italic;
    else if (w === "bold") b |= FontStyle.Bold;
    else if (w === "underline") b |= FontStyle.Underline;
    else if (w === "strikethrough" || w === "line-through") b |= FontStyle.Strikethrough;
  }
  return b;
}

/** Switch the diff to the dark or light theme: Shiki's colour map first, then Monaco's theme. */
export function setDiffTheme(dark: boolean) {
  const ret = highlighter.setTheme(dark ? SHIKI_THEME.dark : SHIKI_THEME.light);
  const c = dark ? comments.dark : comments.light;
  colorMap = ret.colorMap.map(norm).map((x) => (c.from && x === c.from ? c.to : x));
  scopeFor.clear();
  for (const r of (dark ? monacoThemes.dark : monacoThemes.light).rules) {
    const c = norm(r.foreground);
    if (!c) continue;
    const key = styleKey(c, ruleStyleBits(r.fontStyle));
    if (!scopeFor.has(key)) scopeFor.set(key, r.token);
  }
  monaco.editor.setTheme(themeName(dark));
}

class State implements monaco.languages.IState {
  constructor(readonly stack: StateStack) {}
  clone() {
    return new State(this.stack);
  }
  equals(other: monaco.languages.IState) {
    return other instanceof State && other.stack === this.stack;
  }
}

const MAX_LINE = 20_000;
const TIME_LIMIT_MS = 500;

function provider(lang: string): monaco.languages.TokensProvider {
  const grammar = highlighter.getLanguage(lang);
  return {
    getInitialState: () => new State(INITIAL),
    tokenize(line, state) {
      const s = state as State;
      if (line.length >= MAX_LINE) return { endState: s, tokens: [{ startIndex: 0, scopes: "" }] };
      const r = grammar.tokenizeLine2(line, s.stack, TIME_LIMIT_MS);
      const tokens: monaco.languages.IToken[] = [];
      for (let i = 0; i < r.tokens.length; i += 2) {
        const meta = r.tokens[i + 1];
        const color = colorMap[EncodedTokenMetadata.getForeground(meta)] ?? "";
        const scope = color ? (scopeFor.get(styleKey(color, EncodedTokenMetadata.getFontStyle(meta))) ?? "") : "";
        tokens.push({ startIndex: r.tokens[i], scopes: scope });
      }
      return { endState: new State(r.ruleStack), tokens };
    },
  };
}

// Every language is registered up front; its grammar loads only when a model first uses it.
for (const id of LANGUAGES) {
  if (id === "plaintext") continue;
  monaco.languages.register({ id });
  monaco.languages.registerTokensProviderFactory(id, {
    async create() {
      await highlighter.loadLanguage((await GRAMMARS[id]()).default);
      return provider(id);
    },
  });
}

export { monaco };
