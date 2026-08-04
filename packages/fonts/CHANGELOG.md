# @imprentajs/fonts

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
