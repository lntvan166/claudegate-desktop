## What changes

<!-- What a user will see differently, in a sentence or two. -->

## Why

<!-- The problem this solves. Link the issue if there is one. -->

## Checklist

- [ ] `npm --prefix ui test` passes
- [ ] `npm --prefix ui run check` passes
- [ ] `cargo test --workspace` passes
- [ ] Tests use a temp `HOME` / `CLAUDEGATE_HOME`, never the real `~/.claudegate` or `~/.claude`
- [ ] Fixture names, screenshots and docs use neutral names only (`acme-api`, `shop-api`, `svc-lib`)
- [ ] Docs updated if behaviour changed, and a line added under `Unreleased` in `CHANGELOG.md`
