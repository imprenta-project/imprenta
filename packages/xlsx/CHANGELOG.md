# @imprentajs/xlsx

## 0.1.0-alpha.9

### Patch Changes

- [`21cf2e7`](https://github.com/imprenta-project/imprenta/commit/21cf2e701543953e705256140367b97b3a2c32ac) Thanks [@AbianS](https://github.com/AbianS)! - `render` for a workbook hands back UTF-8 bytes, not one JS string.

  V8 caps a string at 512 MiB of characters, and a fourteen-million-cell export
  died there — while serialising, before the writer was involved at all ([#12](https://github.com/imprenta-project/imprenta/issues/12)).
  The IR is now stringified in pieces small enough that no single string
  approaches the cap, each piece encoded as it is made, and the result returned
  as a `Uint8Array`. The bytes are byte for byte what `JSON.stringify` would
  have produced — the test asserts equality against the real thing, because
  "equivalent" is where two serialisers start to drift.

  `write` and `writeToFile` accept the bytes as they always accepted a string —
  their signatures now say so — so a caller that pipes `render` into `write`
  changes nothing. A caller that treated the result as a string wraps it in
  `TextDecoder` or, better, stops needing to.

  Measured end to end: a million declared rows to a finished `.xlsx` in 7.2 s,
  where the string cap used to end the run before the engine saw a byte.

- [`50a0327`](https://github.com/imprenta-project/imprenta/commit/50a03275da7fc312441bd389518a9b5e6da8a301) Thanks [@AbianS](https://github.com/AbianS)! - A pointer past 2 GiB no longer comes back negative.

  A wasm32 pointer crosses the boundary as an `i32` and JavaScript read it as
  signed, so once a module's linear memory had grown past 2 GiB every pointer it
  handed back was negative — and the very next line used it as an offset, which
  died with "offset is out of bounds". An error about bounds reads like memory
  corruption and points away from the real cause; it cost an afternoon before
  anyone found it ([#12](https://github.com/imprenta-project/imprenta/issues/12)).

  Every export is now wrapped once, at instantiation, so its result is read as
  an unsigned 32-bit value. One place rather than a `>>> 0` at each call site,
  deliberately: a future export cannot forget. The regression test grows a real
  instance past the signed line and round-trips a write through it.

  This moves the wall from 2 GiB to wasm32's 4 GiB. What it really buys is the
  failure mode: a workbook that is merely large now fails, if it fails, with an
  answer that names the actual limit.

## 0.1.0-alpha.8

## 0.1.0-alpha.7

### Minor Changes

- [`50c2c5c`](https://github.com/imprenta-project/imprenta/commit/50c2c5caff09f741747466f9a90d94dea2ffdc0c) Thanks [@AbianS](https://github.com/AbianS)! - A header row can carry the autofilter.

  ```tsx
  <Row filter className="bg-slate-100 font-bold">
    <Cell>Fecha</Cell>
    <Cell>Concepto</Cell>
    <Cell>Importe</Cell>
  </Row>
  ```

  The dropdowns Excel puts on a table so whoever opens it can sort and filter
  each column. It is the first thing anybody does with an export of twenty-five
  columns, and there is a reason for shipping it switched on rather than leaving
  it to them: with a frozen pane in the way, turning it on by hand means
  selecting the right range starting at the labels, and a slip makes Excel read
  the title row as the header and offer to filter by `MOVIMIENTOS CONTABLES`.

  Marked **on the row** rather than declared as a range, because the range ends
  at the last row of the sheet — and a producer feeding a million rows in batches
  has not got there yet. The engine works it out when the sheet closes, which is
  the only moment anybody can, and that makes the declared and the streamed sheet
  say the same thing with the same words.

  The range covers the labels and everything under them. Excel reads the first
  row of an autofilter as the header, so a range that started below would filter
  by the first row of data — and it would open either way.

  Everything under them means **everything**, including a total row. There is no
  way to say where the table stops, because the sheet's last row is the only end
  a streaming producer can be asked for — so a sheet whose totals sit under the
  data has them inside the filter, offering `Sumas del ejercicio` as a value to
  filter by and hiding the totals the moment anybody filters. Put the filter on a
  table that runs to the end of its sheet, which in practice means giving the
  totals a sheet of their own or leaving them off. `examples/facturacion` marks
  the one sheet of three where that holds, and says so.

  `imprenta dev` draws the dropdowns. The grid is built from the IR and the IR
  carries a flag on the row, so a marked header looked identical to an unmarked
  one — and the slip this exists to prevent is marking the _wrong_ row. The
  preview is the one place that is visible before somebody opens the export.

  One to a sheet, which is what Excel has. Two rows asking for it is refused and
  names both, rather than letting the second quietly win: it is what somebody
  copying a header block gets, and the file opens with the wrong half filterable.

## 0.1.0-alpha.6

### Minor Changes

- [`6e877e3`](https://github.com/imprenta-project/imprenta/commit/6e877e385d2a89c701d582648d12bd98acb5a73e) Thanks [@AbianS](https://github.com/AbianS)! - A sheet can carry a picture.

  ```tsx
  <Cell>
    <Image src="logo" width={120} />
  </Cell>
  ```

  ```ts
  await write(ir, { images: [{ name: "logo", data: bytes }] });
  ```

  Written **inside a cell** rather than declared beside the sheet with a row and
  a column. Coordinates would be a second thing to keep in step with the rows:
  insert a header above and the logo stays where it was, which is the bug an
  anchor exists to prevent. It floats over the grid rather than sitting in the
  cell, so the cell it names stays blank and `COUNTA` is unaffected.

  There is a width and no height, exactly as on a page. The image's own pixels
  give the ratio, because asking for both is the one way to squash a logo and it
  is always somebody copying the numbers off the last one. The anchor is
  `oneCellAnchor` for the same reason — the two-cell form stretches a picture
  between two corners, so widening a column distorts it.

  The bytes go beside the IR and never in it. A workbook is JSON that goes on a
  queue, into a cache or through an HTTP body, and a logo inline would make every
  one of those carry it. An image the sheets never name is not written into the
  package at all, and a picture naming an image nobody handed over stops the
  write — rather than producing a workbook with a hole where the logo was, which
  nobody notices until a customer opens it.

  Four parts go into the package for one picture — the media, the drawing, the
  drawing's relationships and the sheet's — and Excel opens a repair dialog
  naming none of them if any is missing. A workbook without a picture is byte for
  byte the workbook it was before: no extra parts, no extra namespace on the
  worksheet, no extra content type.

  `imprenta dev` and `imprenta build` hand the project's configured images to the
  sheet side as they already did to the page side, and the checks gained
  `missing-image` for a workbook, so a picture with no image behind it is named
  with its sheet instead of surfacing as a write that failed.

  That rule runs **before** the write, which is the only place it can. The engine
  refuses to produce a workbook with a hole where the logo was, so a rule checked
  after the write can never fire: every workbook that would trip it fails first,
  with the engine's wording, naming neither the sheet nor the document. `refuse()`
  holds the short list of rules the writer will not get past.

  A workbook whose rows are streamed takes its images the same way — `new Book(
sheets, { images })`. It has to, because a letterhead on a million-row ledger is
  the case streaming exists for, and a picture is placed from the rows and merges
  that were _written_ rather than the ones declared up front: a session keeps the
  heights of the rows a placed picture's block covers, and forgets every other row
  as it goes, so a centred logo lands where the same workbook declared whole puts
  it without the sheet costing anything to hold. The one thing it cannot recover
  from is a merge declared after its own rows have gone past — there is nothing
  left to measure by then, so it says so rather than guessing.

  The header reader that turns eight bytes of PNG or JPEG into a size has moved
  to `imprenta-core`. An image's own size is vocabulary rather than model, and
  two readers of the same eight bytes would be two places for a JPEG with an EXIF
  segment in front of its frame header to be got wrong.

  `imprenta dev` draws it too. The grid is built from the IR, the IR carries only
  a name, so a sheet with a letterhead showed a workbook the file did not
  contain — the engine wrote the picture and the author could not see it. The
  preview now serves the project's configured images and hangs each one off its
  anchor cell, spilling over the cells beside it the way Excel does.

  A picture can be placed inside the block it hangs from, with `align` and
  `valign`. This has to be the engine's arithmetic and cannot be the author's:
  centring needs the picture's _height_, and the height comes from the image's
  own pixels, which only the engine has read. Somebody computing an offset by
  hand gets it right for the logo in front of them and wrong for the next one —
  silently, because the picture is still on the page.

  ```tsx
  <Cell colSpan={2} rowSpan={4}>
    <Image src="logo" width={120} align="center" valign="center" />
  </Cell>
  ```

  The block is the **merge** that swallowed the anchor, not the anchor cell: a
  letterhead hangs off `A1` and the author combined `A1:B4` to make room for it,
  so centring in `A1` alone would put it in the corner of what the eye reads as
  one cell. `offset` still applies on top, as a nudge from wherever the placement
  put it. A picture larger than its block is left in the corner rather than given
  a negative offset, which would push it off the edge of the sheet where it can
  neither be seen nor dragged back.

  Sizing a merged block means converting Excel's column unit — characters of the
  body font — into points, which is the one measurement in a workbook that is not
  a length. `imprenta dev` does not repeat that arithmetic: the grid draws a merge
  as one element, so the browser already knows how big it is.

## 0.1.0-alpha.5

## 0.1.0-alpha.4

## 0.1.0-alpha.3

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
