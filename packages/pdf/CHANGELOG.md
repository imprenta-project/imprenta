# @imprentajs/pdf

## 0.1.0-alpha.1

### Patch Changes

- [`3906dda`](https://github.com/imprenta-project/imprenta/commit/3906ddacbb16379f720642962021b5bd4d19b55f) Thanks [@AbianS](https://github.com/AbianS)! - Every package now says it is public, and carries a readme.

  `0.1.0-alpha.0` went out restricted: `access: "public"` in the changesets
  config was not enough on its own, and a scoped package defaults to private, so
  the five were installable by nobody but their owner. Each declares
  `publishConfig.access` now, which is the setting npm actually reads.

  They also had no readme of their own, so their npm pages were blank — the one
  place somebody decides whether to install a thing.

## 0.1.0-alpha.0

### Minor Changes

- [`9812c67`](https://github.com/imprenta-project/imprenta/commit/9812c67aaf3719fb748872b34ee0e72e71129310) Thanks [@AbianS](https://github.com/AbianS)! - The first published version, and it is an alpha on purpose.

  A document engine in Rust, authored in React. `@imprentajs/pdf` measures and
  paginates a page and places every glyph on it; `@imprentajs/xlsx` writes a
  workbook where a number stays a number, so `SUM` returns what it should;
  `@imprentajs/react` declares either of them from components, with a separate set
  of elements per format because a page and a sheet are not the same model;
  `@imprentajs/fonts` fetches and caches Google faces without needing a CLI or a
  config file; and `@imprentajs/cli` gives you `init`, a live preview that shows the
  real PDF rather than a rendering of it, a `build` that compiles documents the
  same way the preview does, and nine rules that say whether a document will
  survive being printed.

  Installed with `@next`, because the shape of the API is not settled and a
  release that nobody can accidentally depend on is the point of this one.
