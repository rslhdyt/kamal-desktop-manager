---
name: release
description: Cut a Kamal Desktop Manager release end to end — pick the semver bump, write CHANGELOG.md, open the release PR, tag after merge, watch the build publish the GitHub release, verify the updater feed and downloads, then update the release notes on kdm-site. Use when the user types /release or asks to release, ship, cut or tag a new version of kdm.
argument-hint: "[patch|minor|major|x.y.z]"
disable-model-invocation: true
---

# Release Kamal Desktop Manager

Ships a new version of the app (this repo) and updates the release notes on the landing page (`../kdm-site`, repo
`rslhdyt/kdm-lp`). Run it from the kdm repo root.

How the pieces fit:

- The version lives in four files: `package.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and the `kdm`
  entry in `src-tauri/Cargo.lock`. `scripts/bump-version.mjs` sets all four and dates the changelog. Don't edit them by
  hand.
- `CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Its section for a version becomes
  the GitHub release body and the notes in the updater's `latest.json` (see the "Release notes" step in
  `.github/workflows/release.yml`).
- Pushing a `v*` tag runs `release.yml`, which fails unless the tag matches `tauri.conf.json`, then builds macOS
  (arm64, x64) and Linux and publishes them as a release of this repo. Installed apps update from
  `releases/latest/download/latest.json`, so **a published release goes out to every user**.
- kdm-site redirects `/download/*` to the newest release's assets on its own. Its `/changelog` page and the version
  in the download section come from `src/CHANGELOG.md`, a copy of this repo's changelog.

## Ground rules

- Every outward step needs the user's go-ahead, asked for at that step, with the exact command shown: pushing a
  branch, opening a PR, pushing a tag, merging, and deploying the site. Approval for one doesn't cover the next.
- Never move, delete or reuse a tag once its release is published, and never edit a published version's changelog
  section. Fix forward with a patch release.
- Stop and report at the first failed check. Don't work around it.
- Keep a short checklist of the phases below and tick each off as you go, so the user can see where the release is.

## 1. Preflight

Run these and stop on any failure:

```sh
git switch main && git pull --ff-only
git status --porcelain                  # must be empty
gh auth status
gh secret list                          # needs TAURI_SIGNING_PRIVATE_KEY, TAURI_SIGNING_PRIVATE_KEY_PASSWORD
git -C ../kdm-site status --porcelain   # must be empty
gh pr list --state open                 # anything here that should go in first?
```

Find the last release: `git describe --tags --abbrev=0 --match 'v*'`. No tag means this is the first release.

## 2. Pick the version

Base the bump on what changed since the last tag (`git log <last-tag>..HEAD --oneline`, plus merged PRs:
`gh pr list --state merged --base main --search "merged:>=<date of last tag>" --json number,title,body,labels`).
Under semver while the version is `0.x`:

| Change | Bump |
| --- | --- |
| Breaking change: removed feature, changed settings/storage format, raised minimum macOS | minor (major once ≥ 1.0) |
| New user-visible feature | minor |
| Fixes, copy, performance, dependency updates only | patch |

If the argument was `patch`, `minor`, `major` or an explicit `x.y.z`, use it, but say so if the changes suggest a
different bump. If there's no previous tag and the version in `tauri.conf.json` has never been tagged, the first
release can ship that version as is.

Use AskUserQuestion to confirm the version, with your suggestion as the first option. Pre-releases (`x.y.z-beta.1`) are
allowed, but `release.yml` publishes them as the latest release and the updater serves them to everyone. Warn about
this before using one.

## 3. Write the release notes

Edit the `## [Unreleased]` section of `CHANGELOG.md`. If it already has entries, check them against the commits and
add what's missing. Rules:

- Group under `### Added`, `### Changed`, `### Deprecated`, `### Removed`, `### Fixed`, `### Security`, in that order,
  and leave out empty groups.
- Write for people who use the app, not for contributors. Say what changed for them ("Logs tab keeps your search when
  you switch containers"), not how it was done. Leave out internal-only work: refactors, CI, tests, docs.
- One bullet per change, ending with a period, matching the style of earlier entries. Credit outside
  contributors as `(#123, thanks @handle)`.
- Put breaking changes first in their group and say what the user has to do.
- Security fixes go under `### Security`. Name the impact, not an exploit.

Show the user the section and wait for their approval or changes. Don't continue without it.

## 4. Verify the build

```sh
pnpm install --frozen-lockfile
pnpm build                           # tsc + vite
(cd src-tauri && cargo test)
```

For a release with UI changes, offer to run `mise run build-local` and open the `.app` for a quick smoke test before
going on.

## 5. Release PR

```sh
git switch -c release/vX.Y.Z
node scripts/bump-version.mjs X.Y.Z
git diff --stat                      # exactly: CHANGELOG.md, package.json, src-tauri/{tauri.conf.json,Cargo.toml,Cargo.lock}
git commit -am "Release vX.Y.Z"
```

Once the user agrees, push and open the PR. The PR body is the version's changelog section:

```sh
git push -u origin release/vX.Y.Z
gh pr create --base main --title "Release vX.Y.Z" --body-file <changelog section saved to a scratch file>
```

Give the user the PR link. The user merges it, or you merge with `gh pr merge --squash --delete-branch` if they ask
you to. Wait until it's merged, and check with `gh pr view --json state,mergeCommit`.

## 6. Tag

```sh
git switch main && git pull --ff-only
node -p "require('./src-tauri/tauri.conf.json').version"   # must print X.Y.Z
git tag -a vX.Y.Z -m "Kamal Desktop Manager vX.Y.Z"
```

Push the tag only after the user confirms. This is the step that publishes the release to every user:

```sh
git push origin vX.Y.Z
```

## 7. Watch the build

```sh
gh run list --workflow release.yml --limit 1      # find the run for vX.Y.Z
gh run watch <run-id> --exit-status               # ~15–25 min; run it in the background and report progress
```

If a job fails, get the log with `gh run view <run-id> --log-failed` and diagnose. What to do next depends on whether
anything was published (`gh release view vX.Y.Z`):

- **Nothing published:** fix it on a branch through a PR, then move the tag to the fixed commit, after the user
  confirms: `git push --delete origin vX.Y.Z && git tag -d vX.Y.Z`, then repeat step 6.
- **A partial release was published** (one platform failed): re-run the failed job with `gh run rerun <run-id> --failed`.
  If it fails again, ask the user whether to delete the partial release and its tag or to leave it and
  ship a patch.

## 8. Verify the release

```sh
gh release view vX.Y.Z --json assets,body --jq '.assets[].name, .body'
curl -sL https://github.com/rslhdyt/kamal-desktop-manager/releases/latest/download/latest.json
curl -sI https://kdm.rslhdyt.dev/download/mac-arm | grep -i '^location'
curl -sI https://kdm.rslhdyt.dev/download/mac-intel | grep -i '^location'
```

Check all of these:

- The assets include `_aarch64.dmg`, `_x64.dmg`, both `.app.tar.gz` with a `.sig` each, the Linux `.AppImage`/`.deb`,
  and `latest.json`.
- The body is the changelog section.
- `latest.json` shows `"version": "X.Y.Z"` and has `darwin-aarch64` and `darwin-x86_64` entries with signatures.
- Both download redirects point at the X.Y.Z dmg. The site caches the GitHub API for 5 minutes, so retry before you
  report a failure.

Mention that the user can confirm the update end to end by opening an older installed copy and looking for the
update banner.

## 9. Update kdm-site

```sh
cd ../kdm-site
git switch main && git pull --ff-only
git switch -c release-notes/vX.Y.Z
cp ../kdm/CHANGELOG.md src/CHANGELOG.md
pnpm build                                        # prerenders dist/changelog.html
grep -c 'X.Y.Z' dist/changelog.html dist/index.html   # both must be > 0
```

Then check whether the landing copy needs to change. `src/Page.tsx` describes each feature (`FEATURES`), and
`src/demo/` holds hand-ported copies of the app's panels. If the release changed a panel or a described behavior, list
what's out of date and ask whether to update it in this PR or later. Don't port demos without being asked.

Commit `Release notes for vX.Y.Z`. After the user confirms, push and open a PR in kdm-site. When it's merged, deploy
from up-to-date `main`, again only after the user confirms:

```sh
git switch main && git pull --ff-only && pnpm build && npx wrangler deploy
curl -s https://kdm.rslhdyt.dev/changelog | grep -c 'X.Y.Z'
```

## 10. Wrap up

Report:

- the version
- links to the GitHub release, the kdm and kdm-site PRs, and https://kdm.rslhdyt.dev/changelog
- anything skipped or left open, such as site copy that still needs updating, an unchecked smoke test, or a missing
  notarization warning from the build

Remind the user that `## [Unreleased]` is empty again, so the next changes start there.
