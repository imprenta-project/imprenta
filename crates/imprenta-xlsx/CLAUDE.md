# imprenta-xlsx

The spreadsheet writer. A workbook model, and the OOXML package it becomes.
See the root `CLAUDE.md` for the rules that apply everywhere.

## The one thing that makes this a separate crate

**A cell carries a value and a type; the engine decides nothing about how it
looks.** That is the exact inverse of the PDF side, where every glyph on the
page was placed here.

```
<Cell>1200</Cell>     the characters 1200. SUM returns zero.
<Cell value={1200}/>  the number 1200. SUM works.
```

There is no measuring, no pagination and no painting in this crate. If you
find yourself wanting any of them, you are writing the wrong crate.

## Module map

| | |
|---|---|
| `ir` | the workbook as declared: sheets, columns, rows, cells, typed values |
| `style` | what a cell looks like, and the **interned table** Excel keeps them in |
| `serial` | dates, as the number Excel keeps underneath one |
| `sheet` | one worksheet's XML, a row at a time |
| `session` | the workbook, written as rows arrive |
| `package` | the zip and its several small parts |
| `xml` | escaping, and Excel's bijective base-26 column names |

## Rules

**1. Everything interns.** A hundred thousand ledger rows use perhaps six
formats between them. Emit a format per cell and `styles.xml` grows past the
data and Excel takes minutes to open the file. `style.rs` is this crate's
equivalent of the PDF engine's shaping cache, and a test asks for the same
style a hundred thousand times to hold the line.

**2. The element order inside a worksheet is fixed by the schema.**
`sheetViews` → `cols` → `sheetData` → `mergeCells`. A worksheet whose `<cols>`
follows its `<sheetData>` is well-formed XML, invalid OOXML, and opens as a
repair dialog that names nothing.

**3. Text is an inline string, never the shared string table.** The table is
smaller for repetitive text and **cannot be written until every string is
known**, which is what a streamed workbook never knows. One way of writing
text means a streamed file and a declared one are the same bytes.

**4. `write()` is a session fed everything at once.** Not a second
implementation. That is why the two agree by construction; the test that pins
it is a guard against somebody separating them again.

**5. Nothing is written that Excel would silently ignore.** Every `xf` that
uses a font, fill, border or number format sets the matching `applyX` flag —
without it the file is valid and the formatting simply does not appear.

**6. Determinism.** Zip entries carry a fixed timestamp, not "now". The same
input must produce the same bytes.

## Traps

- **Excel believes 29 February 1900 existed.** Lotus 1-2-3 had the bug and in
  1985 compatibility mattered more than the calendar. The arithmetic has a
  seam at serial 60, `serial.rs` handles it, and the reference values come
  from an independent calendar rather than from our own reasoning.
- **Fill index 1 must be `gray125`.** Nothing will ever use it. Excel indexes
  fills from a table it assumes begins with `none` and `gray125`, and a file
  that omits it comes back with every fill shifted by one.
- **`styles.xml` is written after the sheets.** The table is only complete
  once every cell has been seen. Zip entries have no required order beyond
  `[Content_Types].xml` first, and that is what allows one pass.
- **Border sides go left, right, top, bottom** — the schema's order, not the
  CSS one. Get it wrong and the top rule appears on the left.
- **A date with no number format shows as 46237.** `Value::Date` exists to
  carry that flag; it writes the same XML as a number.
- **`inf` and `nan` have no representation.** Writing them makes Excel refuse
  the whole workbook for the sake of one cell.

## Testing

- Unit tests in the module, `#[cfg(test)]` at the bottom.
- `tests/excel_reads_it.rs` reads what we wrote **with calamine**, an
  independent implementation. A file we merely agree with ourselves about is
  worth nothing.
- Beyond that, open one. `cargo run -p imprenta-xlsx --example ventas
  --release` writes `preview/ventas.xlsx`; `openpyxl` is stricter than
  calamine and found the first real defect here (no default cell style).
- Benchmarks are `examples/bench_*.rs`, `--release` only.
