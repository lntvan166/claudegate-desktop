# Releasing

## Cutting a release

Bump `version` in `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json` and `ui/package.json`, then push a `v<version>` tag. `.github/workflows/release.yml` builds and signs the installers and `latest.json`, and leaves a draft release to publish. Signing uses the `TAURI_SIGNING_PRIVATE_KEY` secret. Its only copies are that secret and the age-encrypted backup in bifrost (`~/.tauri/claudegate-desktop.key`). If the key is lost, installed apps can no longer verify an update.

The release workflow builds a `.deb` and an AppImage for Linux and a universal `.dmg` for macOS. The installers are not code-signed. The updater signature (below) is separate from OS code signing.

## Signing key

The updater checks each download against the public key in `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). The private key and its backups are described in the paragraph above.

## Bumping the bundled hook

The app ships the extension's `hooks/hook.py` from the tag pinned as `EXT_TAG` in `xcheck/fetch-extension.sh`, committed as `src-tauri/resources/hook.py` with `# CLAUDEGATE_HOOK_VERSION=<tag>` as its second line. After an extension release, run `scripts/bump-hook.sh v1.17.0`: it changes `EXT_TAG`, re-fetches `.xcheck-cache/claudegate` at that tag, regenerates `resources/hook.py` with the stamp and copies the tag's `hooks/audit.py` to `resources/audit.py`, and runs the xcheck, which fails unless the bundled hook is the tag's hook plus that one line and `audit.py` is the tag's byte for byte. Then run `cargo test --workspace` and `bash scripts/capture-e2e.sh`, and commit `xcheck/fetch-extension.sh`, `src-tauri/resources/hook.py` and `src-tauri/resources/audit.py`. Before a release is tagged, `scripts/bump-hook.sh --from-cache v1.18.0` bundles from whatever `.xcheck-cache/claudegate` has checked out (a local clone at a branch), stamped with the planned version, without touching `EXT_TAG`; re-run the plain form once the tag exists. If the xcheck reports a port that drifted (the settings patch, say), update it in `core/src/capture.rs` first.
