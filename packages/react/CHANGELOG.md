# @imprentajs/react

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

- [`0f3c256`](https://github.com/imprenta-project/imprenta/commit/0f3c256fa41e7420ecd006c298f93d146cfb716b) Thanks [@AbianS](https://github.com/AbianS)! - A sheet takes its rows as plain data, the way a table already did.

  A React element per cell costs a fiber, an `Instance` and a props object for
  the duration of one synchronous render — measured at 6,427 bytes of heap per
  row against the 226 bytes of IR it produces ([#11](https://github.com/imprenta-project/imprenta/issues/11)). `<Sheet>` now also takes a
  `rows` prop: the same shape `<Row>` and `<Cell>` spell out — typed values,
  formulas, formats, `className`, spans, anchored images — minus the elements.
  Data rows are appended after whatever the children declare, so a header band
  stays JSX and the hundred thousand rows under it are just an array.

  The two forms go through the same functions and produce identical IR; the
  test holds that line with equality, not similarity.

  Measured on 200,000 rows of five cells:

  |                          | children | `rows` prop |
  | ------------------------ | -------: | ----------: |
  | heap after the host tree | 1,177 MB |  **183 MB** |
  | time to build it         |   891 ms |   **58 ms** |
  | heap per row             |  5,885 B |   **916 B** |

  Parity with `<Table rows>`, which was the goal, because the table was already
  shipping.

## 0.1.0-alpha.8

### Patch Changes

- [`0b805f9`](https://github.com/imprenta-project/imprenta/commit/0b805f990097223eb795437c44081c6cbb7f0e2b) Thanks [@AbianS](https://github.com/AbianS)! - A table cell can cover several columns, so a header can name a group of them.

  ```tsx
  <Table
    columns={[
      { width: 120 },
      { width: 90, align: "end" },
      { width: 90, align: "end" },
    ]}
    header={[
      {
        cells: [
          { text: "Cuenta" },
          { text: "Importe del ejercicio", colSpan: 2 },
        ],
      },
      { cells: [{ text: "" }, { text: "Debe" }, { text: "Haber" }] },
    ]}
    rows={apuntes}
  />
  ```

  Several header rows arrived first, and they only got half way: a report could
  say which group this is and what its columns mean, but not that two of those
  columns are one thing said twice. The name had to sit over the left half of
  its pair and pretend.

  The cell takes the x of the first column it covers and the width of all of
  them, so it lines up with them exactly. **The columns it covers belong to it**
  — the next cell in the row starts after them, so a spanned row is written
  short rather than padded with the blanks a spreadsheet wants. It is `colSpan`
  and not `span` for the same reason: a sheet cell already calls it that, and
  `<Span>` is already a run of text.

  Alignment and overflow still come from the first column covered, because
  neither is a property of a cell here: a table's style is the caller's, and the
  column is where the caller put it.

  Nothing else moved. The packer never learns a row has fewer cells than
  columns, and a span past the last column stops at the last column rather than
  overlapping what is already placed.

  The `ragged-row` check counts the columns a row covers rather than the cells
  it holds, so a grouped header is no longer reported as an error — it used to
  say the engine would drop the difference, which it does not.

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

### Patch Changes

- [`6e877e3`](https://github.com/imprenta-project/imprenta/commit/6e877e385d2a89c701d582648d12bd98acb5a73e) Thanks [@AbianS](https://github.com/AbianS)! - `border-[#11307D]` on a cell now draws the colour it names, instead of quietly
  drawing a thin black line.

  A brand colour is a hex nobody has a Tailwind name for, which is why `bg-[#…]`
  and `text-[#…]` both take one written out. The border did not: the arbitrary
  value never reached the resolver, and what arrived instead was an empty
  suffix — which the width branch reads as a bare `border`. So the colour was
  dropped and the width was reset on the way past.

  ```tsx
  <Cell className="border-b-2 border-[#11307D]" />
  // wanted: a medium navy rule
  // drew:   a thin black one
  ```

  The worst kind of defect this project has, for the third time and in a new
  place: the sheet opens, the rule is there, every test is green, and it is the
  wrong rule. It was found by exporting a ledger whose title block is closed by a
  brand-coloured line, and looking at the file.

  A width written out is refused rather than accepted, because Excel has three
  and no others — `border-[3pt]` gets the same message `border-8` already gets,
  and so does `border-[2]`, which does land on one of the three. Whether the class
  is honoured cannot depend on the number somebody guessed at: the three widths
  have names, and one of them is what the author meant.

  `border-b-[#11307D]` names a side and a colour together, which is how anybody
  writes it having seen `border-b-2`. It used to be refused as "not a utility a
  spreadsheet has" — true of the string and no use to somebody holding a brand
  colour, because a written value swallows everything up to the bracket and the
  side went with it. Both surfaces take it now: a cell, and **a page**, which had
  the same gap and would otherwise have made one class mean two things depending
  on what it was printed onto.

## 0.1.0-alpha.5

### Minor Changes

- [`533da7a`](https://github.com/imprenta-project/imprenta/commit/533da7a3b60f133b73d97da12f3d2f76f6054158) Thanks [@AbianS](https://github.com/AbianS)! - A table can repeat several rows at the top of each page, not just one.

  ```tsx
  <Table
    header={[
      {
        style: { background: "#11307D" },
        cells: [{ text: "600000" }, { text: "Compras" }],
      },
      { cells: [{ text: "FECHA" }, { text: "DESCRIPCIÓN" }, { text: "DEBE" }] },
    ]}
    rows={apuntes}
  />
  ```

  A grouped report — a ledger, a journal, a balance by period — wants to say two
  things at the top of its table: which group this is, and what its columns mean.
  Both have to come back when the group runs over the page, and with one row an
  author had to choose which half of that question a reader on page 40 got
  answered. A browser has never had this problem: two `<tr>` in a `<thead>` and
  both repeat.

  Several rows are still **one atom**. A repeated prefix is one indivisible block
  by definition, so the rows are stacked into a single box before anything is
  paginated — the packer, the painter and the streaming composer never learn
  there was a second row, and none of them changed.

  The IR now holds `header` as a list, which is a breaking change to anything
  writing the IR by hand. `<Table header>` takes one row or an array, and
  `Printer.openTable` normalises a single row too, so the streaming API is no
  stricter than the declarative one: an author who wrote one row should not find
  out from a deserialiser that the engine wanted a sequence.

  The checks read the header as a list too, so every repeated row is checked like
  the row it is — one short of a cell is exactly as wrong as a body row short of
  one, and rather more visible.

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
