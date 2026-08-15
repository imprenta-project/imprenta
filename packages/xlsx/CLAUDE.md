# @imprentajs/xlsx

The spreadsheet writer as one WebAssembly module. See the root `CLAUDE.md` for
the rules that apply everywhere, and `packages/pdf/CLAUDE.md` for the shape —
this package is the same one, with a different model behind it.

## What is generated and what is written

```
imprenta-xlsx.wasm  compiled from crates/imprenta-xlsx-wasm — never committed
src/module.ts       the ABI; the only file that knows about pointers
src/writer.ts       Writer and SyncBook. Blocks the calling thread
src/worker.ts       one writer on its own thread
src/pool.ts         the workers, and who gets one
src/index.ts        write / writeToFile
src/stream.ts       Book — rows fed a batch at a time
```

Rebuild after any change under `crates/`:

```bash
pnpm --filter @imprentajs/xlsx build
```

## The API, and why it is that shape

- `write(ir)` → `{ xlsx, bytes, sheets }`
- `writeToFile(ir, path)` → the bytes never reach the calling thread
- `new Book(sheets, { path })` from `@imprentajs/xlsx/stream` — feed rows in
- `Writer` from `@imprentajs/xlsx/writer` — **synchronous**, for when you are
  already off the main thread or in a browser

`ir` is the workbook as **JSON — a string or the same JSON as UTF-8 bytes**,
which is what `@imprentajs/react/xlsx`'s `render` produces; bytes exist
because V8 caps a string at 512 MiB and a very large workbook got there
(issue #12). The bytes come back as a `Uint8Array` rather than a `Buffer` —
see the PDF package's note for why.

## Rules

- **Await every call before the next.** A second one while the first is running
  is refused. A spreadsheet is written in order.
- **The sheets are declared up front, the rows are not.** Names, columns and
  frozen panes go in the constructor, because the package names every sheet in
  its first entry; rows are what a streaming producer does not have yet.
- **Merges can come late.** They are written after the rows, which is what lets
  a total row's span be decided once the row count is known. `book.at` says how
  far down the sheet has got.
- **Batch size is not a speed question here.** Measured on the addon: 0.38 s for
  two hundred thousand rows whether the batch is 1 or 100,000. It changes only
  what the caller holds.
- **The pool is two, not one per core.** Writing a sheet is XML into a zip with
  no shaping and no layout, so a second writer is there to keep one long export
  from blocking a short one, not to go faster.

## The two modules cannot collide, and that used to matter

The addons could not each declare a `#[global_allocator]`: two of them in one
process aborted on a font that was perfectly good, and it showed as the PDF
engine dying rather than as anything to do with the allocator.

`test/together.test.ts` was written to hold that line and it still runs, but the
failure it guards against is now structurally impossible: two WebAssembly
modules have a linear memory each and share nothing. Keep the test — it is
cheap, and it is the only thing asserting that loading both in one process is a
supported thing to do.

## Testing

`test/write.test.ts` covers what needs Node running: promises, rejection,
ordering, the file-versus-buffer paths. `test/strict-reader.test.ts` runs the
output past a reader stricter than the ones the unit tests use, which has
already found a real defect.

The test that matters most: content fed in pieces is byte for byte what the
same content declared whole produces.
