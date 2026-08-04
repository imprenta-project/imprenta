#!/usr/bin/env node
/**
 * Are there changesets waiting to be turned into a version?
 *
 * The release workflow has two phases and this is what tells them apart. Yes:
 * open the "version packages" pull request. No: that pull request has just
 * been merged, so cut the release.
 *
 * Counting `.changeset/*.md` is the obvious way to do it and it is wrong here.
 * **In pre-release mode `changeset version` does not delete the changeset
 * files** — it records their names in `pre.json` so that leaving pre-release
 * can write one combined changelog from all of them. The count would sit at
 * one forever and the release would never be cut. Measured, not assumed: run
 * `changeset version` on this repository in `alpha` mode and the `.md` is
 * still there afterwards.
 *
 * So a changeset is pending when it is on disk and **not** already listed in
 * `pre.json`. Outside pre-release there is no `pre.json` and every file on
 * disk is pending, which is the same rule with an empty list.
 *
 * Prints `true` or `false` on stdout, and writes `pending=<value>` to
 * `$GITHUB_OUTPUT` when there is one.
 */
import { appendFileSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const dir = fileURLToPath(new URL('../.changeset', import.meta.url));

const consumed = (() => {
  try {
    return new Set(JSON.parse(readFileSync(`${dir}/pre.json`, 'utf8')).changesets ?? []);
  } catch {
    // No pre.json: not in pre-release, nothing has been consumed.
    return new Set();
  }
})();

const pending = readdirSync(dir)
  .filter((name) => name.endsWith('.md') && name !== 'README.md')
  .map((name) => name.slice(0, -'.md'.length))
  .filter((name) => !consumed.has(name));

console.log(pending.length > 0);

// biome-ignore-start lint/suspicious/noUndeclaredEnvVars: the rule wants this
// declared in turbo.json, and it does not belong there — nothing runs this
// script through turbo. It is a step in `release.yml`, and GITHUB_OUTPUT is
// how a step hands a value to the ones after it.
if (process.env.GITHUB_OUTPUT) {
  appendFileSync(process.env.GITHUB_OUTPUT, `pending=${pending.length > 0}\n`);
}
// biome-ignore-end lint/suspicious/noUndeclaredEnvVars: as above

// On stderr so it never lands in the value a workflow reads.
console.error(
  pending.length > 0
    ? `${pending.length} changeset(s) pending: ${pending.join(', ')}`
    : 'no changesets pending — the version pull request has landed',
);
