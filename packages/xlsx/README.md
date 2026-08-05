# @imprentajs/xlsx

**The Imprenta spreadsheet writer, as a native Node addon.** Writes real XLSX,
where a number stays a number.

```bash
npm i @imprentajs/xlsx@alpha
```

```ts
import { write } from '@imprentajs/xlsx';

const { xlsx, sheets, bytes } = await write(ir);
```

`ir` is a **JSON string**. The usual way to produce it is
[`@imprentajs/react`](https://www.npmjs.com/package/@imprentajs/react).

## Why a separate package from the PDF side

A page and a sheet are not the same model, and pretending otherwise is how
spreadsheets get ruined. In a PDF the engine decides what every glyph looks
like; in a workbook it decides nothing — a cell carries a **value and a type**,
and Excel renders it when somebody opens the file. Writing `1200` as text into
a PDF gives you the characters. Writing it as text into a sheet makes `SUM`
return zero, and the recipient gets a wrong total.

So this package shares the vocabulary with the PDF engine and none of the
model.

- `write(ir)` → `{ xlsx, bytes, sheets }`
- `writeToFile(ir, path)` — no Buffer ever exists; use it for anything large
- `new Book(sheets, { path })` from `@imprentajs/xlsx/stream` — feed rows in

Sheets, columns and frozen panes are declared up front because the format names
every sheet in its first entry. Rows are not, because a streaming producer does
not have them yet. Merges can come last, which is what lets a total row's span
be decided once the row count is known.

## One writer, every runtime

The writer is a single WebAssembly module that **imports nothing at all**, so
there is no per-platform package to install and nothing to pick at run time.
The same file runs in Node, the browser, Deno, Bun and on an edge worker.

The calls above go to a worker. In a browser, reach for the writer directly:

```ts
import { Writer } from '@imprentajs/xlsx/writer';

const writer = await Writer.load({ wasm });
const { xlsx } = writer.write(ir);   // synchronous: put it in a Worker
```

The bytes come back as a `Uint8Array` rather than a `Buffer`, because `Buffer`
is Node's. `Buffer.from(xlsx)` gets the string helpers back at no cost.

## Status

Alpha. It works and is built test-first, but the API is not settled.

Apache-2.0 · [github.com/imprenta-project/imprenta](https://github.com/imprenta-project/imprenta)
