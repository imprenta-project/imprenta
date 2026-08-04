---
"@imprentajs/pdf": patch
"@imprentajs/xlsx": patch
"@imprentajs/react": patch
"@imprentajs/fonts": patch
"@imprentajs/cli": patch
---

Every package now says it is public, and carries a readme.

`0.1.0-alpha.0` went out restricted: `access: "public"` in the changesets
config was not enough on its own, and a scoped package defaults to private, so
the five were installable by nobody but their owner. Each declares
`publishConfig.access` now, which is the setting npm actually reads.

They also had no readme of their own, so their npm pages were blank — the one
place somebody decides whether to install a thing.
