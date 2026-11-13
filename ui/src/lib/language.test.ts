import { describe, expect, it } from "vitest";
import { LANGUAGES, languageFor } from "./language";

describe("languageFor", () => {
  it("maps the brief's examples", () => {
    expect(languageFor("x.go")).toBe("go");
    expect(languageFor("Dockerfile")).toBe("dockerfile");
    expect(languageFor("x.unknown")).toBe("plaintext");
  });

  it("uses the base name of a full path, any case", () => {
    expect(languageFor("/w/src/App.TSX")).toBe("typescript");
    expect(languageFor("C:\\w\\main.RS")).toBe("rust");
    expect(languageFor("/w/docker/Dockerfile.dev")).toBe("dockerfile");
    expect(languageFor("/w/api.dockerfile")).toBe("dockerfile");
  });

  it("covers the common set", () => {
    const cases: [string, string][] = [
      ["a.ts", "typescript"],
      ["a.mjs", "javascript"],
      ["package.json", "json"],
      ["tsconfig.jsonc", "json"],
      ["a.svelte", "html"],
      ["a.yml", "yaml"],
      ["Cargo.toml", "ini"],
      ["run.sh", "shell"],
      [".zshrc", "shell"],
      ["q.sql", "sql"],
      ["main.py", "python"],
      ["README.md", "markdown"],
      ["x.h", "cpp"],
      ["Gemfile", "ruby"],
      ["main.tf", "hcl"],
      [".env", "ini"],
      ["Makefile", "plaintext"],
      ["noext", "plaintext"],
    ];
    for (const [f, want] of cases) expect(languageFor(f), f).toBe(want);
  });

  it("only returns ids the editor registers", () => {
    const names = ["a.ts", "a.js", "a.json", "a.css", "a.scss", "a.less", "a.html", "a.xml", "a.md", "a.yaml", "a.go", "a.rs", "a.py", "a.sh", "a.sql", "Dockerfile", "a.java", "a.kt", "a.swift", "a.cpp", "a.cs", "a.rb", "a.php", "a.ini", "a.graphql", "a.lua", "a.tf", "a.proto", "a.ps1", "a.zzz"];
    for (const n of names) expect(LANGUAGES, n).toContain(languageFor(n));
  });
});
