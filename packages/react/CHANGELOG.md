# @imprentajs/react

## 0.1.0-alpha.4

### Minor Changes

- [`34e948f`](https://github.com/imprenta-project/imprenta/commit/34e948f4f046b4bce6335e6328266e85d06ae18d) Thanks [@AbianS](https://github.com/AbianS)! - Four things a page needed and could not have: a paragraph set against the
  right edge, one justified, a gap that pins what follows to the foot of the
  page, and space after a box that falls outside its background.

  **A paragraph can be aligned, and justified.**

  ```tsx
  <Text align="end">36.390,62 €</Text>
  <Text className="text-right">Lanzagrava S.L.</Text>
  <Text align="justify">…un párrafo legal que llena la caja…</Text>
  ```

  Alignment existed only on a table column, so the one way to put a figure
  against the right margin was to make it a table — and a table cannot be nested,
  which meant it could not go in a header, a footer, or inside a box with a
  background. An invoice is full of things that are not tables and still have to
  line up on the right: a company address in the masthead, a total in its own
  box. There was no way to say so.

  It is the same `Align` a table column takes, deliberately: an amount under a
  table has to line up with the amounts in it, and two notions of "the right
  edge" would eventually disagree by a fraction of a point. `text-left`,
  `text-right` and `text-center` resolve to it too — Tailwind spells alignment
  with the same utility as size and colour, so those are recognised before
  `right` can be looked up as either and reported as neither.

  Left-aligned text is untouched and costs nothing: a line that is not shifted is
  emitted exactly as before, with no box around it. That matters at the size this
  engine is built for — a box per line across fifty thousand pages would be a box
  per line.

  `justify` is the odd one out and is not a fourth direction to shove the line
  in: nothing moves, the spaces widen until the line reaches the far edge. Only
  the spaces — scaling every advance would also hit the number and would set the
  words in a font nobody chose. The last line of a paragraph keeps the width it
  earned, and a line with no spaces in it is left alone rather than letter-spaced,
  which is a different typographic decision and not one to make on somebody's
  behalf.

  What lands on the margin is the last glyph anybody can see. Almost every line a
  breaker returns ends in a space, and a space that counts towards the line
  leaves the text short by exactly its own advance — the same amount on every
  line, so the right edge comes out straight but inset, which reads as flush
  until something else on the page is set against the same margin. It hangs past
  the edge instead.

  **`spaceAfter` no longer grows the box it was meant to follow.**

  At the top level it always behaved: the space became a spacer emitted after the
  box. Composed — in a header, in a footer, or nested inside another container —
  it was folded into the box's own bottom padding instead, so a box with a
  background or a border grew by exactly that much and whatever followed stayed
  welded to it. An author asking for room after a tinted panel got a taller
  tinted panel, and nothing said otherwise.

  The folding is right for a paragraph, which is what it was written for: text
  has nothing painted behind it, and a paragraph has to sit the same whether or
  not it has a neighbour. A decorated container does have something painted
  behind it, and that is the whole difference. Nothing changes when `spaceAfter`
  is zero — the box is returned as it was, with no wrapper around it.

  **`<Spacer grow />` takes whatever is left of the page.**

  ```tsx
  …totals…
  <Spacer grow />
  <Row background="#F9FAFB">…payment terms…</Row>
  ```

  The one measurement an author cannot make for themselves: only the packer
  knows where the content stopped. Without it, a block meant for the foot of the
  page — payment terms, a signature line — sits wherever the content above it
  happened to end. `height` becomes the least the gap may be, and it is what the
  atom is budgeted at while the run is being fitted, so the arithmetic that chose
  the page and the height that gets painted cannot disagree.

  It keeps with what follows, deliberately. A gap that swallowed the whole page
  would push that block onto the next one, which is the opposite of what was
  asked for; what it actually takes is the room left once the rest of its run is
  accounted for. Inside a box there is no page to take the rest of, and that is
  reported rather than ignored — a gap that does nothing looks exactly like a gap
  nobody asked for.

## 0.1.0-alpha.3

### Patch Changes

- [`376787f`](https://github.com/imprenta-project/imprenta/commit/376787f7fd0fd5293b5471bfa1c6244b0b72d085) Thanks [@AbianS](https://github.com/AbianS)! - A table row's `style` is now resolved the way a box's is.

  `RowProps.style` is typed as a box's props, and it was handed to the engine
  exactly as written: a colour string where the engine holds a border per side, a
  single number where it holds four, and a `className` nobody ever looked at. A
  row asking for a hairline underneath produced a document the engine could not
  read at all — `invalid type: string, expected struct Edges` — while a `<Box>`
  with the same three words drew one.

  Two fields, `background` and `radius`, happen to have the same shape on both
  sides. That is what made the rest look like it worked too, and it is why the
  test asserts a row's resolved style against a box's rather than against a
  literal: whatever a box learns to accept, a row now accepts with it.

## 0.1.0-alpha.2

### Minor Changes

- [#3](https://github.com/imprenta-project/imprenta/pull/3) [`d85bc05`](https://github.com/imprenta-project/imprenta/commit/d85bc050b45f0c9a4adb0ebbae200a8b2b944b0a) Thanks [@AbianS](https://github.com/AbianS)! - One engine that runs everywhere, instead of ten binaries that run somewhere

  Both engines are now a single WebAssembly module each, and the native addons
  are gone. What went with them: ten per-platform npm packages, a five-way build
  matrix, three hundred lines of publish workflow, and the reason all of it
  existed — a `.node` has to be linked on a machine that can produce that target.

  The modules **import nothing at all**. No Node-API, no WASI, no glue layer for
  a host to provide, which is what lets one file run in Node, the browser, Deno,
  Bun, on an edge worker, and on every platform the matrix never covered: musl,
  which is to say Alpine, which is to say a great many Docker images. A test
  asserts the import list is empty against the built module, because that
  property is the whole argument and nothing about a future change would make its
  loss obvious.

  `render`, `renderToFile`, `Printer`, `write`, `writeToFile` and `Book` keep the
  signatures they had — every one of them still returns a promise and still runs
  off the calling thread, now on a worker rather than on libuv's pool. Two things
  did change:

  - **The bytes come back as a `Uint8Array`, not a `Buffer`.** `Buffer` is Node's
    and the same module runs in a browser. A `Buffer` _is_ a `Uint8Array`, so
    `Buffer.from(pdf)` gets the string helpers back at no cost.
  - **`Engine` and `Writer` are new**, exported from `@imprentajs/pdf/engine` and
    `@imprentajs/xlsx/writer`. They are synchronous and block the calling thread,
    which is right in a worker, a CLI or a browser and wrong on a server — where
    the promise-returning calls already do the right thing.

  Alongside it, table rows are measured across every core rather than one at a
  time, which takes a 2,113-page ledger from 1,385 ms to 635 ms where threads are
  available. Measuring in batches also cut what the engine holds while it works,
  from 331 MB to 176 MB on the same document.

  **And it is faster than the addon was.** A WebAssembly module has no threads,
  so one engine renders on one core — 2.8 s on that ledger. The pool gets the
  cores back, and for a long document it does it by splitting the document
  itself: measure everywhere at once, pack once to find where the pages fall,
  paint the ranges at once, merge. **500 ms against the addon's 696**, with the
  same page count and the same numbering, because every piece is told which page
  it starts on before a glyph is placed. Nothing is renumbered afterwards, which
  is the approach this engine exists to replace.

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
