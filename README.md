# Imprenta

**Document engine in Rust, authored in React.** PDF and XLSX; DOCX to follow.

> Status: **alpha.** Published under the `next` dist-tag, which is why every
> install below carries it: `npm i @imprentajs/cli` deliberately gets you nothing.
> The engine works and is built test-first, but the API is not settled and a
> minor version may move it.

[Why](#why) · [Measured](#measured) · [Authoring](#authoring) · [Printing](#printing) ·
[Spreadsheets](#spreadsheets) · [The preview](#the-preview) · [Development](#development)

---

## Quick start

```bash
npm i @imprentajs/react@next @imprentajs/pdf@next   # print from a server
npm i @imprentajs/xlsx@next                         # …or export a spreadsheet
npm i -D @imprentajs/cli@next                       # preview as you write

npx imprenta init                                   # a project that renders straight away
npx imprenta dev                                    # open the preview
```

`init` writes `imprenta.config.ts`, a `documents/` folder with a working
invoice, and a line in `.gitignore`. It never overwrites anything already
there, so it is safe to run inside a project you already have.

Printing from a backend is two calls and no CLI at all:

```ts
const ir = await render(<Factura {...data} />);   // @imprentajs/react
const { pdf } = await toPdf(ir, { fonts });       // @imprentajs/pdf
res.type('application/pdf').send(pdf);
```

## Why

Generating a paginated document today means picking one of two bad options.

**Print from a headless browser.** HTML renders well, but a browser is hundreds
of megabytes of dependency, it gives you almost no control over where a page
breaks, and its per-page header runs in a sandbox with no access to your data.
Past a certain size the input has to be sliced up and the output stitched back
together — which breaks page numbering, running totals and repeated table
headers.

**Draw the PDF from JavaScript.** The authoring model is right, and pagination
control tends to stop at "try not to break inside this box".

Neither lets you say: *this page's header depends on what is on this page.*

## What Imprenta does differently

**Pagination is the product, not an afterthought.** The engine works in three
phases — measure (parallel), pack (serial, pure arithmetic), paint (parallel).
Packing 9,231 pages takes ~10 ms, so the engine can make good page-break
decisions instead of guessing:

- Automatic segmentation that the developer never sees. No chunk size, no merge,
  no post-hoc page-number stamping. Memory stays flat regardless of length.
- Headers and footers are functions of the page, with your data in scope.
- State that carries across page boundaries — running totals, "continued from
  page 12", "items 51–100 of 3,482".
- Widows, orphans and repeated table headers decided from measured heights, not
  from a hardcoded rows-per-page constant.
- Layout problems reported at build time, not discovered by whoever opens the
  PDF.

**No HTML, no CSS engine.** React produces a typed JSON IR. Skipping the
browser's parse–cascade–DOM pipeline is where the speed comes from: a prototype
of this design measured **1.11 ms/page against Chromium's 5.0**, at a ninth of
the memory. Tailwind still works — utilities resolve to the style struct
directly, with a theme in millimetres rather than pixels.

## Measured

A 50,000-page accounting ledger — four million rows — on a laptop:

| | |
|---|---:|
| Time | **23.8 s** · 0.48 ms/page |
| Peak memory | **579 MB** · 11.7 KB/page |
| Output | 128 MB · 2.5 KB/page |
| Embedded fonts | **1**, subset |
| Text | selectable and searchable throughout |

For scale, the same document by other routes:

| | Time | Peak memory |
|---|---:|---:|
| **Imprenta** | **23.8 s** | **579 MB** |
| Imprenta without streaming | ~68 s | ~10.6 GB |
| Chromium, extrapolated | ~250 s | ~200 GB |

Per page, against what a Node service typically reaches for:

| Engine | ms/page | MB/page |
|---|---:|---:|
| Chromium via Playwright | 5.0 | 4.02 |
| fulgur (HTML→PDF in Rust) | 16.1 | 4.32 |
| **Imprenta** | **0.47** | **0.016** |

Ten times faster and two hundred and fifty times lighter than a headless
browser. The memory figure is the one that matters: a document of any length
costs a fixed amount per page, because pages are painted and released as the
content is fed in.

## Authoring

### React

```tsx
import { B, Document, Table, Text, render } from '@imprentajs/react/pdf';

const Invoice = ({ number, items }) => (
  <Document margin={40}>
    <Text size={22} color="#1b3a5c"><B>FACTURA</B></Text>
    <Text size={11}><B>{number}</B></Text>
    <Table
      columns={[{ width: 46 }, { width: 'auto' }, { width: 90, align: 'end' }]}
      header={{ cells: [{ text: 'Ref.' }, { text: 'Concepto' }, { text: 'Importe' }] }}
      rows={items.map((i) => ({ cells: [{ text: i.ref }, { text: i.concept }, { text: i.total }] }))}
    />
  </Document>
);

const ir = await render(<Invoice number="FV-2026-00418" items={items} />);
```

Components, hooks and context all work — it is a real React renderer, not a
tree walk — and what comes out is the IR, not HTML. `<Text>` flattens what is
nested inside it into styled runs, joining neighbours that match so that where
JSX split a string never reaches the shaper.

`@imprentajs/react` depends on React and nothing else — not on the native addon —
so a document can be declared anywhere, including where the engine is not
installed.

### Tailwind

```tsx
import { Box, Document, Text } from '@imprentajs/react/pdf';
import { Theme } from '@imprentajs/react';

<Theme colors={{ brand: '#1b3a5c' }}>
  <Document className="p-10">
    <Box className="bg-slate-50 border border-slate-300 p-4 mb-4">
      <Text className="text-sm text-brand font-bold">Panel</Text>
    </Box>
  </Document>
</Theme>
```

No CSS is involved at any point — no parse, no cascade, no stylesheet. A class
is looked up and a number or a colour comes out, which is the same reason the
engine skips the browser. Tailwind's palette is converted from oklch to sRGB
once, at build time, so paper gets the colours it can actually print.

Sizes keep the web's arithmetic: a rem is sixteen CSS pixels at three quarters
of a point each, so `text-sm` on paper is the size a designer expects. One
number on `<Theme>` rescales the whole document.

**A class it cannot honour is an error, not a shrug.** `flex` has no meaning on
a page, `hover:` needs a state a printed page is never in, and `w-1/2` cannot be
expressed for a box; each says so by name, at render time, rather than producing
a document that quietly came out wrong.

Corners round. A radius follows the background always, and the border with it
when the border runs all the way round in one width and colour — where two
sides differ the corner belongs to neither, so the rules stay straight and the
engine says which box it happened to.

### Headers, footers and what carries across a page

```tsx
<Document accumulators={['saldo']}>
  <Header height={38}>
    <Text>Libro mayor — ejercicio 2026</Text>
  </Header>
  <Footer height={30}>
    <Text size={8}>
      Suma anterior <RunningTotal name="saldo" at="opening" /> · Suma y sigue{' '}
      <B><RunningTotal name="saldo" /></B>   Página <PageNumber /> de <PageCount />
    </Text>
  </Footer>
  <Table … />
</Document>
```

A band is declared once and built again for every page, because a page number
and a carried-forward total are different words on every sheet and glyphs
cannot be substituted after they are shaped. Its height comes out of the
content box rather than the margin, so it can never overlap the last line.

`<PageCount />` is the one thing that costs something: nothing can know the
total until the last page is packed, so a document that prints one is held
whole. A footer that only numbers its pages pays none of that and still
streams.

## Printing

### From a Node backend

The usual way to use this: a controller imports the component, passes it data,
and returns the bytes.

```ts
const ir = await render(<Factura {...data} />);   // @imprentajs/react
const { pdf } = await toPdf(ir, { fonts });       // @imprentajs/pdf
res.type('application/pdf').send(pdf);
```

Both packages ship ESM and CommonJS, so a NestJS app that is still CJS can
`require` them. `examples/backend` is exactly this, with the fonts loaded once
when the process starts rather than per request.

Neither call touches the main thread: during a 649 ms render, a 1 ms
`setInterval` fired 515 times. A service stays answerable while it prints. For
anything large there is `renderToFile`, which writes from Rust so a 128 MB
ledger never becomes a 128 MB Buffer on its way to disk:

```ts
const { pages, bytes } = await renderToFile(ir, 'ledger.pdf', {
  fonts: [{ data: regular }, { weight: 'bold', data: bold }],
  images: [{ name: 'logo', data: logo }],
});
```

The document crosses as a JSON string, not a JS object — `JSON.stringify` plus
serde beats walking an object across the boundary, and a string is also what
arrives from a file, a queue or an HTTP body. Images are handed over as bytes
and nothing else; format and dimensions are read from the file.

### Streaming a long document

For a document too big to hold, `toChunks` yields it in the pieces the engine
reads, and they go straight into a `Printer`:

```ts
for await (const chunk of toChunks(<Ledger rows={rows} />, { batch: 1000 })) {
  …
}
```

Or feed it directly, when the rows come from a cursor rather than a component:

```ts
import { Printer } from '@imprentajs/pdf/stream';

const printer = new Printer(page, { fonts });
await printer.openTable(head);
for await (const batch of ledger.batches(1000)) {
  await printer.rows(batch);
}
await printer.closeTable();
const { pages } = await printer.finish('ledger.pdf');
```

The largest thing the caller holds is one batch. **A document with no table in
it streams the same way** — `printer.nodes(batch)` for a transcript, a log, a
book — and batching matters just as much there.

Measured against the same content declared whole:

| a hundred thousand ledger rows | declared | streamed |
|---|---:|---:|
| Time | 1.33 s | **1.32 s** |
| JS heap | 60 MB | **10 MB** |
| Peak RSS | 370 MB | **214 MB** |

| forty thousand paragraphs, no table | declared | streamed |
|---|---:|---:|
| Time | 1.48 s | **1.44 s** |
| JS heap | 28 MB | **7 MB** |
| Peak RSS | 236 MB | **175 MB** |

Same pages, same bytes, same time — a test pins that content fed in pieces is
byte for byte the same content declared whole, and that how the pieces are cut
makes no difference to the page.

Batch size is a memory-against-overhead trade and the one way to make this
worse. One item per batch takes 2.5 s on the ledger and 1.98 s on the
transcript, because the hop to the engine's thread dominates; a hundred takes
1.35 and 1.44; a thousand takes 1.33 and 1.50; ten thousand buys nothing and
doubles the heap. Anywhere from a hundred to a thousand.

### From the command line

```bash
imprenta build --out ./pdfs --strict
```

Renders every document through the same compile the preview uses, so nothing
can come out one way on screen and another in CI. Each file reports its pages
and anything the checks found; `--strict` turns a finding into a failed build.
One document failing does not stop the rest — a build of forty is worth knowing
about in one go.

## Spreadsheets

A spreadsheet is not a document with different options, so it is not one here
either. There is no page, no margin and nothing is painted — and a cell carries
a **value and a type**, which a printed page never has to think about:

```tsx
import { Cell, Column, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';

<Workbook>
  <Sheet name="Ventas" freeze={{ rows: 1 }}>
    <Column width={10} />
    <Column width={38} />
    <Column width={16} format="#,##0.00 €" />

    <Row className="bg-slate-100 font-bold">
      <Cell>Ref.</Cell><Cell>Concepto</Cell><Cell className="text-right">Importe</Cell>
    </Row>
    <Row>
      <Cell>007</Cell>                    {/* text: the noughts stay */}
      <Cell>Licencia anual</Cell>
      <Cell value={1200} />               {/* a number: SUM adds it up */}
    </Row>
    <Row className="border-t font-bold">
      <Cell colSpan={2}>Total</Cell>
      <Cell formula="SUM(C2:C2)" cached={1200} />
    </Row>
  </Sheet>
</Workbook>
```

That distinction is the whole point. Write `1200` as text and Excel shows the
same thing, `SUM` returns zero, and the recipient has a total that is wrong.
Dates come through as dates — `<Cell value={new Date(...)} />` becomes the
serial Excel keeps underneath one, with a format that makes it readable — and
a formula can carry its answer, so a script that only reads still sees the
number.

**Tailwind works, with a different capability table.** Excel's format record is
font, fill, border, alignment and number format, which is most of what a class
list says; and Excel measures type in points, so `text-sm` is the same size on
paper and on a sheet. What a cell cannot do says so by name and points at what
it can:

| | |
|---|---|
| `p-4` | a cell has no padding — the nearest thing is `indent-1` |
| `w-32` | a column's width is `<Column width>`, not the cell's |
| `leading-6` | a row's height is `<Row height>` |
| `rounded`, `shadow` | a cell has none of those |
| `border-8` | Excel has three widths: `border`, `border-2`, `border-4` |

### From a Node backend

```ts
const ir = await toWorkbook(<Ventas lines={lines} />);   // @imprentajs/react/xlsx
const { xlsx } = await write(ir);                        // @imprentajs/xlsx
res.type('…spreadsheetml.sheet').send(xlsx);
```

`examples/backend` serves both formats from one controller.

### Streaming a large export

```ts
import { Book } from '@imprentajs/xlsx/stream';

const book = new Book([{ name: 'Libro', columns: [{ width: 16 }] }], { path: 'libro.xlsx' });
for await (const batch of cursor.batches(1000)) {
  await book.rows(batch);
}
await book.finish();
```

| | declared | streamed |
|---|---:|---:|
| 200,000 rows | 357 MB · 0.44 s | **12.9 MB · 0.38 s** |
| 1,000,000 rows | 1,773 MB · 2.14 s | **47.9 MB · 1.93 s** |

Same bytes, at any batch size. Unlike the PDF side, batch size makes no
difference to the time at all — 0.38 s whether the batch is 1 or 100,000 —
because there is no thread to hop; it changes only what the caller holds.

## The preview

```bash
npx imprenta dev
```

A server that lists the documents in your project and shows the real PDF for
the one you pick — not a rendering of the page, the file the engine produced.
Save a document and the pane redraws.

```ts
// imprenta.config.ts
import { defineConfig, google } from '@imprentajs/cli';

export default defineConfig({
  documents: './documents',
  fonts: google('Roboto', { weights: ['regular', 'bold'] }),
  images: { logo: './assets/logo.png' },
});
```

`google()` fetches the faces once into `.imprenta/fonts` and uses them from
there, the way `next/font/google` self-hosts what it downloads — nothing to
find, download or check into a repository, and a build with no network works
off the cache. A font of your own is still `{ path: './assets/Mine.ttf' }`,
and the two mix.

It lives in `@imprentajs/fonts`, which needs neither the CLI nor a config file,
so a server can do the same:

```ts
// once, when the process starts — not per request
const fonts = await loadFonts(google('Roboto', { weights: ['regular', 'bold'] }), {
  cache: './.imprenta/fonts',
});
```

Note where fonts are *not*: a document declares what it looks like, and which
files it is set in belongs to whoever is printing it. That is what lets the
same component render in the preview with one set and on the server with the
brand's own.

It asks Google for TrueType specifically. A modern browser is answered with
woff2 and something as old as MSIE with EOT; the engine can read neither, and
either would have surfaced as an unreadable-font error far from the cause.

Sample data lives beside the document and ships nowhere — the preview renders
`<Factura {...Factura.PreviewProps} />`, and production passes real data to the
same component:

```tsx
export default function Factura({ number, lines }: Props) { … }

Factura.PreviewProps = {
  number: 'FV-2026-00418',
  lines: [{ ref: '001', concept: 'Licencia anual', price: 1200 }],
} satisfies Props;
```

### What it will tell you

Along the bottom is a panel that says whether the document is any good. Not
whether it is handsome — whether it will survive being printed, which is a
question you cannot answer by looking at a screen.

| | |
|---|---|
| `tiny-text` | type below 6pt, where print stops being legible |
| `unprintable-margin` | ink inside the 5mm most printers cannot reach |
| `faint-text` | a contrast under 3:1, which survives a screen and not paper |
| `low-resolution-image` | a picture printed larger than its pixels can carry |
| `missing-face` | bold or italic the project configured no font for |
| `wider-than-the-page` | a box the page will cut off at the margin |
| `ragged-row` | a table row with fewer cells than the table has columns |
| `unopenable-link` | an href a reader cannot follow out of a PDF |
| `empty-document` | nothing in it, which is a component that returned nothing |

Plus whatever the engine itself noticed — a missing glyph, a clipped cell — so
there is one list rather than two. Errors sort above warnings, the same fault
in several places is counted rather than repeated, and a clean document says
**Ready to print**.

Anything that stopped the document rendering is shown in the browser by name.
`examples/facturacion` is a project laid out the way one really would be, with
a `mal-hecho.tsx` that gets seven things wrong on purpose.

## Where the memory goes

Pages stream: they are painted and dropped as content is fed, so a document of
any length costs a fixed amount per page. For the ledger above:

| | |
|---|---:|
| IR tree built in Rust, no JSON | 27 MB · 270 B/row |
| Reading the same IR from JSON | 3 allocations per row |
| Painting 1,409 pages on top | ~50 KB/page |

Reading it used to cost sixteen allocations a row and three kilobytes, because
serde deserialises an internally tagged enum — `{"t": "table", …}` — by
buffering the whole map into an intermediate tree before it knows which
variant to build, and for a node holding a hundred thousand rows that
intermediate tree is the document several times over. `Node` reads its tag by
hand now; peak live memory while parsing went from 289 MB to 80 MB, and
`tests/allocations.rs` holds the line with a counting allocator.

The other half was the caller's: declaring the document cost 171 MB in Node
before the engine was called at all. That is what the streaming input above
removed — a producer emits rows and the whole document never exists.

## Design commitments

Things that are cheap on day one and near-impossible to retrofit, so they are in
from the start:

- **Bidirectional and CJK text.** Correct from the first commit, not a later
  feature.
- **Text is a sequence of styled runs**, never a bare string.
- **Semantic roles on every node**, so tagged PDF, PDF/UA and PDF/A stay
  reachable.
- **Fonts always embedded and subset.**
- **Deterministic output** — byte-identical for identical input, which makes
  visual diffing in CI possible.

## Repository layout

```
crates/
  imprenta-core/   units, colour, diagnostics, IR envelope — format-neutral
  imprenta-pdf/    the engine: measure, pack, paint, and the declared IR
  imprenta-pdf-napi/   the Node binding
packages/
  pdf/             @imprentajs/pdf   — the native addon, and the streaming Printer
  react/           @imprentajs/react — declare a document in React
  fonts/           @imprentajs/fonts — fetch and cache the faces it is set in
  cli/             @imprentajs/cli   — init, the preview, and the build
examples/
  facturacion/     a project laid out the way one really would be
  backend/         a controller: React in, PDF bytes out, no CLI anywhere
```

The IR is versioned JSON, and the engine does not know React exists — any
producer in any language can target it.

## Development

Everything is written test-first. No production code without a failing test.

```bash
pnpm install

pnpm run ci                     # Node packages: test, build, lint, types

cargo test --workspace          # Rust
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

`@imprentajs/pdf` is compiled from `crates/imprenta-pdf-napi`, so **after any change
to Rust, rebuild it before trusting a Node test** — a stale binary makes tests
pass by comparing two documents that are both wrong:

```bash
pnpm --filter @imprentajs/pdf build
```

To look at a real page rather than an assertion:

```bash
cargo run -p imprenta-pdf  --example invoice --release  # writes into preview/
cargo run -p imprenta-xlsx --example ventas  --release  # and a spreadsheet
pnpm --filter facturacion dev                           # the preview server
pnpm --filter backend start                             # both, over HTTP
```

`CLAUDE.md` at the root and in each crate and package records the conventions
and the traps — read the one nearest what you are changing.

## Licence

Apache-2.0
