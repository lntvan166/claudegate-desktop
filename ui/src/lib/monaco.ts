// The Monaco build the diff view uses: the core editor API, the diff editor and a few editor
// features, the languages in LANGUAGES (language.ts), and the editor worker. Everything is bundled by
// Vite; nothing is fetched at runtime. Loaded lazily by DiffView, so it stays out of the first paint.
import * as monaco from "monaco-editor/editor/editor.api.js";
import "monaco-editor/features/diffEditor/register.js";
import "monaco-editor/features/codicon/register.js";
import "monaco-editor/features/find/register.js";
import "monaco-editor/features/folding/register.js";
import "monaco-editor/features/clipboard/register.js";
import "monaco-editor/features/contextmenu/register.js";
import "monaco-editor/features/bracketMatching/register.js";
import "monaco-editor/languages/definitions/typescript/register.js";
import "monaco-editor/languages/definitions/javascript/register.js";
import "monaco-editor/languages/definitions/css/register.js";
import "monaco-editor/languages/definitions/scss/register.js";
import "monaco-editor/languages/definitions/less/register.js";
import "monaco-editor/languages/definitions/html/register.js";
import "monaco-editor/languages/definitions/xml/register.js";
import "monaco-editor/languages/definitions/markdown/register.js";
import "monaco-editor/languages/definitions/yaml/register.js";
import "monaco-editor/languages/definitions/go/register.js";
import "monaco-editor/languages/definitions/rust/register.js";
import "monaco-editor/languages/definitions/python/register.js";
import "monaco-editor/languages/definitions/shell/register.js";
import "monaco-editor/languages/definitions/sql/register.js";
import "monaco-editor/languages/definitions/dockerfile/register.js";
import "monaco-editor/languages/definitions/java/register.js";
import "monaco-editor/languages/definitions/kotlin/register.js";
import "monaco-editor/languages/definitions/swift/register.js";
import "monaco-editor/languages/definitions/cpp/register.js";
import "monaco-editor/languages/definitions/csharp/register.js";
import "monaco-editor/languages/definitions/ruby/register.js";
import "monaco-editor/languages/definitions/php/register.js";
import "monaco-editor/languages/definitions/ini/register.js";
import "monaco-editor/languages/definitions/graphql/register.js";
import "monaco-editor/languages/definitions/lua/register.js";
import "monaco-editor/languages/definitions/hcl/register.js";
import "monaco-editor/languages/definitions/protobuf/register.js";
import "monaco-editor/languages/definitions/powershell/register.js";
import EditorWorker from "monaco-editor/editor/editor.worker.js?worker";

// The diff runs in the editor worker. No language service runs at all: a read-only diff needs
// colours, not completions or validation.
self.MonacoEnvironment = {
  getWorker() {
    return new EditorWorker();
  },
};

// JSON: highlighting only, from the JavaScript grammar. The JSON language service would add about
// 1.8 MB (its mode and worker) for validation and hovers a read-only diff never shows.
monaco.languages.register({ id: "json", extensions: [".json", ".jsonc", ".json5"], aliases: ["JSON"] });
monaco.languages.onLanguage("json", () => {
  void import("monaco-editor/languages/definitions/javascript/javascript.js").then((js) => {
    monaco.languages.setMonarchTokensProvider("json", js.language);
    monaco.languages.setLanguageConfiguration("json", js.conf);
  });
});

export { monaco };
