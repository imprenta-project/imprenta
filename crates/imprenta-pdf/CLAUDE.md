# imprenta-pdf

The engine. Measure → pack → paint, plus the IR authors declare. See the root
`CLAUDE.md` for the rules that apply everywhere.

## Module map

Read in this order; each `//!` header says what its module deliberately does
not do.

| | |
|---|---|
| `ir` | the document as declared — **intent only**, never measurement |
| `shape` | phase A½: text → positioned glyphs, advances normalised to the em |
| `measure` | phase A: content → `Atom`s. The seam; everything past it is arithmetic |
| `atom` | the unit of pagination: one indivisible slice of measured content |
| `pack` | phase B: atoms → pages. Serial, pure arithmetic, no fonts, no text |
| `content` | what a placed atom draws, once the packer has decided where |
| `render` | phase C: painting a packed page, page geometry, bands. The bytes are `imprenta-pdf-write`'s |
| `compose` | streaming composition — pages painted and released as fed |
| `build` | walks the IR, measures, feeds the composer. Knows both sides |
| `session` | the same, fed in chunks instead of declared whole |
| `table`, `list`, `widows`, `decoration`, `image`, `parallel` | mechanism |

## The rules that keep this fast

**1. The packer must stay ignorant of *what*, not of *how much*.**
`pack.rs` must never learn whether an atom came from a paragraph, a table or
something not yet written. What it may know is arithmetic: heights, break
flags, `keep_with_next`, `grow`. Widows and orphans are the worked example on
one side — both reduce to `keep_with_next`, so the packer knows nothing about
either — and `grow` is the example on the other: how much of a page is left
over is a fact only the packer holds, so no measurer could have reduced it to
anything. Reach for a property that already exists first; add one only when the
answer genuinely lives here, and say so on the field.

**2. Adding a primitive touches four places, and usually not the packer.**
An `ir::Node` variant → a measurer producing atoms → a `content::Content`
variant → a `render` arm that paints it. Plus its `Decoration`, which is always
supplied by the caller. A case in `pack.rs` keyed on the *kind* of thing an
atom is remains wrong; a new arithmetic property on `Atom` is not.

**3. A box and its content are one atom, not two stacked.**
Two atoms lay out one after the other, so the text lands below its own
background. Containment is the difference between a decorated strip and a table
row.

**4. Composition streams; anything that cannot must say so.**
Pages are painted and dropped as content arrives, so memory is flat per page at
any length. `{{pages}}` is the one place that is not flat, because nothing can
know the total until the last page is packed — and it now pays for the answer
with a **second walk** rather than by holding the document:
`Composer::counting()` packs and releases pages painting none of them, and the
document is then painted as `resuming(1, total, …)`, a fragment that happens to
be the whole of itself. A fed document has no second walk of its own, so
`Session` keeps the pieces it was given; a row is a few hundred bytes where the
page it lands on is six kilobytes. Do not add a second silent exception.

**5. Style is never decided here.**
There is no styled `Table` and there never will be. A table is column widths,
cell placement and split rules; every colour, rule and padding arrives as a
`Decoration` the caller supplies. That is what lets the React layer offer an
unstyled `<Table>`.

**6. Parallel where it is safe, serial where it must be — and only where
there is a second core.**
Measuring parallelises: each block is independent, and each worker gets its own
`Shaper` and therefore its own cache. Packing does not, and that ordering is
what makes running totals possible. Painting does not either: a page is written
into the file as it closes.

A worker's own `Shaper` costs a parse of every font file, which is a fair price
for a core and no price at all is worth paying for none — so `worth_splitting()`
asks how many threads rayon actually has. Inside a WebAssembly module the
answer is one, and taking the split away there was **−18% on a ten thousand
page ledger**.

## Traps

- **`ir::Node` has a hand-written `Deserialize`.** `#[serde(tag = "t")]`
  buffers the entire map into an intermediate tree before choosing a variant,
  which for a node holding forty thousand rows is the document several times
  over. Peak live memory while parsing went 289 MB → 80 MB when this was
  written by hand. `tests/allocations.rs` guards it with a counting allocator
  in its own test binary. **Do not replace it with a derive.**
- **The shaping cache is where the speed comes from.** Advances are stored
  normalised to the em so 7 pt and 14 pt are one entry. Anything that puts a
  point size into a cache key throws the cache away.
- **Ask the layout engine once.** Every table cell used to go through it twice
  — once to measure and once, through `report_missing`, only to ask whether the
  font could draw it — and that second pass was **half of all the time spent
  measuring** on a ledger. The lines measuring produces already hold the
  answer: see `shape::missing_in`. `Shaper::layouts()` counts layouts so a test
  can hold the line, because no assertion about a height can see this.
- **A glyph's `text_range` indexes the whole line, not its own stretch.** A
  line that changes ink without changing font is several glyph runs over *one*
  shaped run, and walking that run's clusters afresh for each of them hands the
  second stretch the ranges of the first. Nothing moves on the page; the
  `ToUnicode` map names the wrong letters and the text copies out as nonsense.
- **A group must be opened before its rows.** `open_repeat`/`close_repeat` and
  `Flow::resuming` exist because a repeated table header broke when the flush
  interval landed mid-group. When changing `FLUSH_EVERY` or the group logic,
  test across a flush boundary specifically.
- **Bands are rebuilt per page.** A page number and a carried-forward total are
  different words on every sheet, and glyphs cannot be substituted after
  shaping. Band height comes out of the content box, never the margin.
- **A page is written the moment it is finished.** `imprenta-pdf-write` puts
  the bytes in the output and keeps one offset; nothing comes back to a page
  later. So a band, a link or a repeated header that was not painted when the
  page closed is not on it, and there is no second visit to notice.

## Testing

- Unit tests live in the module, `#[cfg(test)]` at the bottom. There are ~350;
  `pack.rs`, `table.rs`, `shape.rs` and `render.rs` carry most of them.
- `tests/allocations.rs` is a separate binary because its allocator is global.
- **Pin equivalence, not just correctness.** Content fed in chunks must be byte
  for byte what the same content declared whole produces, and how the chunks
  are cut must make no difference. Those tests have caught more real bugs than
  any assertion about a height.
- Benchmarks are `examples/bench_*.rs` and must be run `--release`. A number
  from a debug build is not a number.
- `examples/*.rs` write real PDFs into `preview/`. Open them. A list marker
  touching its text passes every assertion.
