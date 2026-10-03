# Kamal Desktop Manager

Desktop monitor for Kamal apps (Tauri v2 + React). M0 spike.

Toolchain is pinned in `mise.toml` (Node 24, Rust stable).

```sh
pnpm install
pnpm tauri dev
```

Tests (from `src-tauri/`):

```sh
cargo test
# End-to-end against a real project (read-only): kamal config, kamal lock status,
# two host polls (metrics + containers) and a short log follow over one SSH connection
KDM_PROJECT=/path/to/app [KDM_DESTINATION=staging] cargo test live_spike -- --ignored --nocapture
```

## Releasing

1. Bump `version` in `src-tauri/tauri.conf.json` (and `package.json`).
2. Tag and push: `git tag v0.1.0 && git push origin v0.1.0`.
3. `.github/workflows/release.yml` builds macOS (arm64, x64) and Linux and publishes them, with the updater's
   `latest.json`, to the public [kdm-releases](https://github.com/rslhdyt/kdm-releases) repo. Installed apps
   pick the update up on next launch.

Secrets on this repo: `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` (updater key, kept in `~/.tauri/kdm.key`; losing it
means existing installs can't verify new updates) and `RELEASES_TOKEN` (fine-grained PAT with Contents read/write on
kdm-releases). macOS signing and notarization turn on when the `APPLE_*` secrets listed in the workflow are set;
until then builds are ad-hoc signed.

Landing page: [kdm-lp](https://github.com/rslhdyt/kdm-lp), live at https://kdm.rslhdyt.dev.
