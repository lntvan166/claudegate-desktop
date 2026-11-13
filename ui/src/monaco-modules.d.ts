// Monaco ships no declarations for its per-language grammar modules.
declare module "monaco-editor/languages/definitions/javascript/javascript.js" {
  import type { languages } from "monaco-editor/editor/editor.api.js";
  export const conf: languages.LanguageConfiguration;
  export const language: languages.IMonarchLanguage;
}
