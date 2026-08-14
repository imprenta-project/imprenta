# @imprentajs/pdf

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

## 0.1.0-alpha.6

## 0.1.0-alpha.5

### Minor Changes

- [`d8acc38`](https://github.com/imprenta-project/imprenta/commit/d8acc3838a6e4f935f995746e626688319ed9430) Thanks [@AbianS](https://github.com/AbianS)! - An engine that renders one very large document gives the memory back.

  WebAssembly has no instruction to shrink a linear memory, so an instance's
  footprint is the high-water mark of the largest document it has ever rendered,
  for as long as it lives — once per engine in the pool. A service that prints
  one ledger a month and invoices the rest of the time carried that ledger's
  memory from the day it first arrived.

  A worker now takes a fresh instance once a document leaves one swollen. The
  compiled module is reused, so the warm-up is not paid again, and it happens
  after the reply rather than before it, so nobody waits for it.

  ```ts
  await render(ir, { fonts, recycleAbove: 64 * 1024 * 1024 }); // the default
  await render(ir, { fonts, recycleAbove: Infinity }); // never recycle
  ```

  Measured: 21.1 MB held after a 423-page ledger, 2.6 MB with it.

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

- [`d8acc38`](https://github.com/imprenta-project/imprenta/commit/d8acc3838a6e4f935f995746e626688319ed9430) Thanks [@AbianS](https://github.com/AbianS)! - The PDF is written by `imprenta-pdf-write` now, which is ours.

  A page is serialised, compressed and forgotten the moment it is finished; what
  survives it is its bytes in the output and one cross-reference entry. The
  writer this replaces kept every painted page until the document closed and then
  walked the whole collection twice into a buffer the size of the file.

  It is built on the same two crates that one was — `pdf-writer` for the object
  syntax and `subsetter` for the fonts — so subsetting, CID fonts and the
  `ToUnicode` map are not re-derived. What it drops is everything this engine
  cannot reach: no transparency groups, no patterns, no shadings, no clip paths,
  no tagged structure, no encryption.

  What it buys, beyond the memory: the file is **6% smaller**, rendering is
  faster, and a defect that had every long document failing at around 2 400 pages
  — a recursion once per page, on a target with no threads — cannot come back
  with the next upgrade.

  Colours, borders, radii, opacity, images, links and every other way a document
  can look are unchanged, and the engine's whole test suite passes against it
  untouched.

- [`d8acc38`](https://github.com/imprenta-project/imprenta/commit/d8acc3838a6e4f935f995746e626688319ed9430) Thanks [@AbianS](https://github.com/AbianS)! - `{{pages}}` no longer costs the whole document.

  Nothing can know how many pages there are until the last one is packed, so a
  footer saying "de 4 849" used to be bought by holding every painted page in
  memory until then. Measured on a five-column ledger that was **twenty-three
  times** the memory of the same document without it, and it was the largest
  single reason a long one ran out.

  It is now bought by walking the document twice: once to count the pages,
  painting none of them, and once to paint them knowing the answer. The counting
  pass goes through the same measurer and the same packer a real render does —
  a cheaper estimate would be a second paginator, and the two would disagree on
  exactly the documents that print their own length. The file is byte for byte
  what holding produced.

  A fed document has no second walk of its own, since its rows are gone once they
  have been read, so a `Printer` printing `{{pages}}` keeps the _pieces it was
  given_ instead of the pages it painted. A row weighs a few hundred bytes where
  the page it lands on weighs six kilobytes, and it costs the caller nothing.

  Measured through the WebAssembly module, streaming a ledger with `{{pages}}`:

  |  pages |           before |        after |
  | -----: | ---------------: | -----------: |
  |    668 |          64.8 MB |  **22.6 MB** |
  |  2 670 |         244.1 MB |  **49.0 MB** |
  | 10 680 | would not finish | **148.3 MB** |

### Patch Changes

- [`d8acc38`](https://github.com/imprenta-project/imprenta/commit/d8acc3838a6e4f935f995746e626688319ed9430) Thanks [@AbianS](https://github.com/AbianS)! - Fix: a header or footer went missing from every page but the last few.

  A document is painted and dropped as it goes, every few hundred atoms, and
  those pages were released through a code path that built no bands at all. So a
  declared ledger of 1 200 rows came out with a footer on **one page of
  eighteen**, and a header on none. Streamed and sharded documents had it too.

  Every test written around the feature used a document short enough never to
  reach the first flush, which is why it survived: at 200 rows everything is
  painted at the end and everything is correct.

  `Walk` now carries what a band is built from and flushes with it, so a page
  released half-way through a document gets the same header and footer as one
  painted at the end. There are four tests, and each of them counts: a footer on
  every page is exactly one more text run per page than the same document without
  one.

- [`d8acc38`](https://github.com/imprenta-project/imprenta/commit/d8acc3838a6e4f935f995746e626688319ed9430) Thanks [@AbianS](https://github.com/AbianS)! - Rendering is 25–45% faster, because every cell was being laid out twice.

  Measuring a table cell shaped its text, and then the check for characters the
  font cannot draw shaped it all over again — a second full trip through the
  layout engine for every cell in the document, which on a ledger was **half of
  all the time spent measuring**. The lines that measuring produces already hold
  the answer: a glyph that came back as `.notdef` is a character the face could
  not draw, and it carries the byte range it came from.

  The same for a paragraph, which was shaped once to be checked and once to be
  broken into lines.

  A worker also gets its own shaper only where there is a second core to give it
  to. Building one parses every font file the document declares, and inside a
  WebAssembly module — where there are no threads at all — that was paid per
  batch of rows, throwing away the shaping cache each time, in exchange for
  nothing.

  Measured through the module, streaming the same ledger:

  |  pages |   before |        after |
  | -----: | -------: | -----------: |
  |    668 |   602 ms |   **388 ms** |
  |  2 670 | 2 274 ms | **1 307 ms** |
  | 10 680 | 8 863 ms | **4 994 ms** |

  `Shaper::layouts()` counts trips through the layout engine, so a test can hold
  the line. No assertion about a height could see this one.

- [`533da7a`](https://github.com/imprenta-project/imprenta/commit/533da7a3b60f133b73d97da12f3d2f76f6054158) Thanks [@AbianS](https://github.com/AbianS)! - Text too wide for its box is now reported, instead of being painted over the
  edge in silence.

  It happens when nothing in the line can be broken — a URL, a reference code, an
  IBAN written without spaces. The engine breaks what it can, runs out of places
  to break, and paints the rest past the edge. Nothing said so, which made it the
  worst kind of defect this project has: the page looks deliberate, every test is
  green, and a line of it is over the side. It went unnoticed here through an
  entire invoice design until somebody happened to look at the file.

  ```
  text-overflow — "ref=XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX" is 268pt wide
                   where 112pt were available, so it is painted outside its box
  ```

  The engine is the only place this can be caught. The checks in the CLI read the
  IR and have no fonts, so `wider-than-the-page` can tell that a _declared_ width
  is too big and never that a _measured_ line is.

  A table cell has had this all along, as `cell-overflow`. The two are the same
  idea in the two places text is measured, and the names are a pair on purpose —
  what was missing was the paragraph, which is most of a document.

  One report per paragraph, naming its widest line and quoting the first forty
  characters, because a warning per line of a long paragraph is a warning nobody
  reads. It is a warning rather than an error: the document is still usable, and
  the author may have judged that a millimetre over a box edge does not matter.

  This says what happened; it does not fix it. Breaking inside a word that has
  nowhere else to break — CSS's `overflow-wrap: anywhere` — is a separate
  decision and not one to make on an author's behalf.

- [`d8acc38`](https://github.com/imprenta-project/imprenta/commit/d8acc3838a6e4f935f995746e626688319ed9430) Thanks [@AbianS](https://github.com/AbianS)! - Fix: text that changed colour mid-line could not be copied out of the PDF.

  Every glyph carries the range of source text it stands for, and that range is
  what becomes the document's `ToUnicode` map — the thing that lets a reader
  select, copy, search or read the page aloud. On a line that changed style
  without changing font, the second stretch was handed the ranges belonging to
  the first: `Total 1.234,00` in two colours extracted as `Total Total 1.`.

  Nothing on the page moved, which is what makes it worth writing down. The
  document looked perfect in every viewer and every screenshot; only the text
  underneath was wrong.

  A bold stretch was never affected — a different weight is a different font, so
  the shaper starts a new run and the walk restarted correctly by accident. It
  took two stretches of the _same_ face in different ink to show it.

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

- [`228b615`](https://github.com/imprenta-project/imprenta/commit/228b615b8e98d674875659fa6bb04e13459c08ef) Thanks [@AbianS](https://github.com/AbianS)! - A `<Row>` now lays its children side by side wherever it is, not only at the
  top level of a document.

  Nested — inside a `<Box>`, inside another `<Row>`, or inside a `<Header>` or
  `<Footer>`, all of which are composed rather than walked — a row was treated as
  a box and its children stacked. There was no diagnostic and no error: the
  document rendered, and it was simply wrong. A two-column invoice header came
  out as a logo above the company address, and a footer meant to put the page
  number opposite the legal text put it underneath.

  The cause was two copies of the same placement logic, of which only the one
  used at the top level had ever been taught what a row is. They are one now, and
  a test asserts the second panel's coordinates rather than the shape of the
  output — the same lesson as the `spaceAfter` that used to be dropped inside a
  row, which no assertion about shapes had caught either.

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
