// Shiki: the TextMate grammars and themes VS Code uses, on the JavaScript regex engine (no WASM,
// so the CSP needs no `wasm-unsafe-eval`). Each grammar is its own lazy chunk, loaded on the first
// diff in that language; the two themes are small and come with the editor chunk.
import { createHighlighterCore, type HighlighterCore, type LanguageRegistration } from "@shikijs/core";
import { createJavaScriptRegexEngine } from "@shikijs/engine-javascript";
import dracula from "@shikijs/themes/dracula";
import githubLight from "@shikijs/themes/github-light";
import type { Language } from "./language";

/** Shiki's bundled themes the diff uses. No Dracula-family light theme ships, so light is GitHub's. */
export const SHIKI_THEME = { dark: "dracula", light: "github-light" } as const;

type Loader = () => Promise<{ default: LanguageRegistration[] }>;

/** One dynamic import per grammar, written out so the bundler splits each into its own chunk. */
export const GRAMMARS: Record<Exclude<Language, "plaintext">, Loader> = {
  typescript: () => import("@shikijs/langs/typescript"),
  tsx: () => import("@shikijs/langs/tsx"),
  javascript: () => import("@shikijs/langs/javascript"),
  jsx: () => import("@shikijs/langs/jsx"),
  json: () => import("@shikijs/langs/json"),
  jsonc: () => import("@shikijs/langs/jsonc"),
  go: () => import("@shikijs/langs/go"),
  python: () => import("@shikijs/langs/python"),
  rust: () => import("@shikijs/langs/rust"),
  java: () => import("@shikijs/langs/java"),
  kotlin: () => import("@shikijs/langs/kotlin"),
  c: () => import("@shikijs/langs/c"),
  cpp: () => import("@shikijs/langs/cpp"),
  csharp: () => import("@shikijs/langs/csharp"),
  swift: () => import("@shikijs/langs/swift"),
  php: () => import("@shikijs/langs/php"),
  ruby: () => import("@shikijs/langs/ruby"),
  lua: () => import("@shikijs/langs/lua"),
  sql: () => import("@shikijs/langs/sql"),
  yaml: () => import("@shikijs/langs/yaml"),
  toml: () => import("@shikijs/langs/toml"),
  markdown: () => import("@shikijs/langs/markdown"),
  html: () => import("@shikijs/langs/html"),
  xml: () => import("@shikijs/langs/xml"),
  css: () => import("@shikijs/langs/css"),
  scss: () => import("@shikijs/langs/scss"),
  less: () => import("@shikijs/langs/less"),
  svelte: () => import("@shikijs/langs/svelte"),
  vue: () => import("@shikijs/langs/vue"),
  shellscript: () => import("@shikijs/langs/shellscript"),
  powershell: () => import("@shikijs/langs/powershell"),
  dockerfile: () => import("@shikijs/langs/dockerfile"),
  make: () => import("@shikijs/langs/make"),
  ini: () => import("@shikijs/langs/ini"),
  dotenv: () => import("@shikijs/langs/dotenv"),
  proto: () => import("@shikijs/langs/proto"),
  graphql: () => import("@shikijs/langs/graphql"),
  hcl: () => import("@shikijs/langs/hcl"),
  terraform: () => import("@shikijs/langs/terraform"),
  gherkin: () => import("@shikijs/langs/gherkin"),
};

/** A highlighter with both themes and no grammars; `loadLanguage` adds them as diffs need them. */
export function createDiffHighlighter(): Promise<HighlighterCore> {
  return createHighlighterCore({
    themes: [dracula, githubLight],
    langs: [],
    // Forgiving: a pattern the JS engine cannot translate matches nothing instead of throwing.
    engine: createJavaScriptRegexEngine({ forgiving: true }),
  });
}
