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

Run `/release` in Claude Code (`.claude/skills/release/SKILL.md`), which goes through the whole process. By hand:

1. Write the release notes under `## [Unreleased]` in `CHANGELOG.md` ([Keep a Changelog](https://keepachangelog.com/en/1.1.0/)).
2. `node scripts/bump-version.mjs X.Y.Z` sets the version in `package.json`, `tauri.conf.json`, `Cargo.toml` and
   `Cargo.lock`, and dates the changelog section. Land it through a `Release vX.Y.Z` PR.
3. Tag the merged commit and push: `git tag -a vX.Y.Z -m "Kamal Desktop Manager vX.Y.Z" && git push origin vX.Y.Z`.
4. `.github/workflows/release.yml` builds macOS (arm64, x64) and Linux and publishes them, with the updater's
   `latest.json`, to the public [kdm-releases](https://github.com/rslhdyt/kdm-releases) repo. The release
   notes are the version's `CHANGELOG.md` section. Installed apps pick the update up on next launch.
5. Copy `CHANGELOG.md` to kdm-site's `src/CHANGELOG.md` and deploy it, so https://kdm.rslhdyt.dev/changelog shows the
   release.

Secrets on this repo: `TAURI_SIGNING_PRIVATE_KEY` / `_PASSWORD` (updater key, kept in `~/.tauri/kdm.key`; losing it
means existing installs can't verify new updates) and `RELEASES_TOKEN` (fine-grained PAT with Contents read/write on
kdm-releases). macOS signing and notarization turn on when the `APPLE_*` secrets listed in the workflow are set;
until then builds are ad-hoc signed.

Landing page: [kdm-lp](https://github.com/rslhdyt/kdm-lp), live at https://kdm.rslhdyt.dev.
