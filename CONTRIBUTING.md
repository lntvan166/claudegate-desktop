# Contributing

Thanks for helping. Bug reports, small fixes and focused pull requests are all welcome. For a larger change, open an issue first so we can agree on the shape before you build it.

## Set up

You need Rust (stable), Node.js 22 with npm, python3, and the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```bash
npm ci
npm --prefix ui ci
./xcheck/fetch-extension.sh     # the pinned ClaudeGate extension, used by the cross-check and e2e scripts
```

Run the app against a sandbox, not your real data:

```bash
CLAUDEGATE_HOME=/tmp/cg-sandbox npm run tauri dev
```

Or work on the UI alone in a browser, with fixture data and no Tauri:

```bash
npm --prefix ui run dev
```

[docs/development.md](docs/development.md) has the details: headless runs, the fixture flags, the cross-check and the end-to-end scripts.

## Tests

Run all three before you open a pull request. CI runs them too.

```bash
npm --prefix ui test            # UI unit tests
npm --prefix ui run check       # svelte-check
cargo test --workspace          # Rust core and Tauri shell
```

If you touch code the extension also has (session merge, decisions, exclude matching, settings defaults), run the cross-check as well:

```bash
cd xcheck && npm ci && npx tsx run.ts
```

## Rules for tests and examples

- **Tests never touch the real `~/.claudegate` or `~/.claude`.** Point every test at a temp directory: set `HOME` and `CLAUDEGATE_HOME` (and `CLAUDEGATE_DESKTOP_CONFIG` for the app's config) to a `mktemp -d` folder, as `scripts/decide-e2e.sh` does. The only exceptions are the two `#[ignore]`d read-only checks in `core/tests/real_sessions.rs` and `core/tests/real_projects.rs`, which run only when you pass a path explicitly.
- **Use neutral fixture names.** This repo is public. Names in fixtures, tests, screenshots, docs and commit messages must be made up: `acme-api`, `shop-api`, `umbrella`, `svc-lib`, `rin@acme.test`. Never use a real company, product, customer, colleague or private repository name, and never a real home directory path (fixtures use a made-up user, `dev`).
- **Writes stay in their allow-list.** `core/tests/no_writes.rs` fails if a write API appears outside `src-tauri/src/config.rs`, `core/src/store.rs` and `core/src/workspace_write.rs`. A new kind of write belongs in one of those files and needs a row in [docs/what-the-app-writes.md](docs/what-the-app-writes.md).

## Pull requests

- Keep a pull request to one change, with tests for the behaviour it adds or fixes.
- Describe what a user will see differently.
- Update the docs when behaviour changes, and add a line under `Unreleased` in [CHANGELOG.md](CHANGELOG.md).
- Do not bump the version; releases are cut separately (see [docs/releasing.md](docs/releasing.md)).

## Reporting a bug

Use the bug report template. Include your OS, how you installed the app, the app version, the hook version (from the Capture chip's popover) and the steps to reproduce. `~/.claudegate/hook.log` often shows whether the hook ran. Check it for private paths before you paste it.
