// Sets the app version everywhere it lives and turns CHANGELOG.md's [Unreleased] section into the release's section.
// Usage: node scripts/bump-version.mjs 0.2.0 [--date 2026-10-03]
// Every file is checked before any is written, so a failure leaves the tree untouched.
import { readFile, writeFile } from "node:fs/promises";

const REPO = "https://github.com/rslhdyt/kamal-desktop-manager";

const [version, flag, dateArg] = process.argv.slice(2);
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version ?? "")) fail("usage: node scripts/bump-version.mjs <x.y.z> [--date YYYY-MM-DD]");
const date = flag === "--date" ? dateArg : new Date().toISOString().slice(0, 10);

const writes = new Map();

/** Replaces exactly one match, so a format change fails loudly instead of skipping a file. */
async function replaceOnce(path, pattern, replacement) {
  const text = await readFile(path, "utf8");
  const matches = text.match(new RegExp(pattern.source, pattern.flags + "g")) ?? [];
  if (matches.length !== 1) fail(`${path}: expected 1 match for ${pattern}, found ${matches.length}`);
  writes.set(path, text.replace(pattern, replacement));
}

await replaceOnce("package.json", /^  "version": ".*",$/m, `  "version": "${version}",`);
await replaceOnce("src-tauri/tauri.conf.json", /^  "version": ".*",$/m, `  "version": "${version}",`);
await replaceOnce("src-tauri/Cargo.toml", /^version = ".*"$/m, `version = "${version}"`);
await replaceOnce("src-tauri/Cargo.lock", /^name = "kdm"\nversion = ".*"$/m, `name = "kdm"\nversion = "${version}"`);

const changelog = await readFile("CHANGELOG.md", "utf8");
if (changelog.includes(`## [${version}]`)) fail(`CHANGELOG.md already has a ${version} section`);
const unreleased = changelog.match(/^## \[Unreleased\]\n([\s\S]*?)(?=^## \[|^\[)/m);
if (!unreleased || !unreleased[1].trim()) fail("CHANGELOG.md: [Unreleased] is empty; write the release notes first");
const previous = changelog.match(/^## \[(\d[^\]]*)\]/m)?.[1];

const links = [
  `[Unreleased]: ${REPO}/compare/v${version}...HEAD`,
  previous ? `[${version}]: ${REPO}/compare/v${previous}...v${version}` : `[${version}]: ${REPO}/tree/v${version}`,
].join("\n");
writes.set(
  "CHANGELOG.md",
  changelog.replace("## [Unreleased]\n", `## [Unreleased]\n\n## [${version}] - ${date}\n`).replace(/^\[Unreleased\]: .*$/m, links),
);

for (const [path, text] of writes) await writeFile(path, text);
console.log(`bumped to ${version} (${date})${previous ? `, previous ${previous}` : ", first release"}`);

function fail(message) {
  console.error(message);
  process.exit(1);
}
