import { describe, expect, it } from "vitest";
import { LANGUAGES, languageFor } from "./language";

describe("languageFor", () => {
  it("maps the brief's examples", () => {
    expect(languageFor("x.go")).toBe("go");
    expect(languageFor("Dockerfile")).toBe("dockerfile");
    expect(languageFor("x.unknown")).toBe("plaintext");
  });

  it("uses the base name of a full path, any case", () => {
    expect(languageFor("/w/src/App.TSX")).toBe("tsx");
    expect(languageFor("C:\\w\\main.RS")).toBe("rust");
    expect(languageFor("/w/docker/Dockerfile.dev")).toBe("dockerfile");
    expect(languageFor("/w/api.dockerfile")).toBe("dockerfile");
  });

  it("maps extensions to Shiki's grammars", () => {
    const cases: [string, string][] = [
      ["a.ts", "typescript"],
      ["a.mts", "typescript"],
      ["a.tsx", "tsx"],
      ["a.js", "javascript"],
      ["a.mjs", "javascript"],
      ["a.jsx", "jsx"],
      ["package.json", "json"],
      ["tsconfig.jsonc", "jsonc"],
      ["a.go", "go"],
      ["main.py", "python"],
      ["lib.rs", "rust"],
      ["A.java", "java"],
      ["b.kt", "kotlin"],
      ["build.gradle.kts", "kotlin"],
      ["x.c", "c"],
      ["x.h", "c"],
      ["x.cpp", "cpp"],
      ["x.hpp", "cpp"],
      ["x.cs", "csharp"],
      ["x.swift", "swift"],
      ["x.php", "php"],
      ["x.rb", "ruby"],
      ["x.lua", "lua"],
      ["q.sql", "sql"],
      ["a.yml", "yaml"],
      ["Cargo.toml", "toml"],
      ["README.md", "markdown"],
      ["index.html", "html"],
      ["a.css", "css"],
      ["a.scss", "scss"],
      ["a.svelte", "svelte"],
      ["a.vue", "vue"],
      ["run.sh", "shellscript"],
      ["a.bash", "shellscript"],
      ["a.ini", "ini"],
      ["a.proto", "proto"],
      ["s.graphql", "graphql"],
      ["s.gql", "graphql"],
      ["main.tf", "terraform"],
      ["prod.tfvars", "terraform"],
      ["x.hcl", "hcl"],
      ["login.feature", "gherkin"],
      ["a.svg", "xml"],
      ["a.ps1", "powershell"],
      ["noext", "plaintext"],
      ["a.zzz", "plaintext"],
    ];
    for (const [f, want] of cases) expect(languageFor(f), f).toBe(want);
  });

  it("knows special file names", () => {
    const cases: [string, string][] = [
      ["Dockerfile", "dockerfile"],
      ["Containerfile", "dockerfile"],
      ["Makefile", "make"],
      ["GNUmakefile", "make"],
      ["rules.mk", "make"],
      [".env", "dotenv"],
      [".env.local", "dotenv"],
      [".env.production", "dotenv"],
      ["go.mod", "go"],
      ["go.sum", "plaintext"],
      ["Gemfile", "ruby"],
      [".zshrc", "shellscript"],
      [".bashrc", "shellscript"],
      [".gitconfig", "ini"],
      [".editorconfig", "ini"],
    ];
    for (const [f, want] of cases) expect(languageFor(f), f).toBe(want);
  });

  it("only returns ids it lists", () => {
    const names = ["a.ts", "a.tsx", "a.feature", "Makefile", ".env.x", "a.tf", "go.mod", "a.zzz", "a.robot"];
    for (const n of names) expect(LANGUAGES, n).toContain(languageFor(n));
  });
});
