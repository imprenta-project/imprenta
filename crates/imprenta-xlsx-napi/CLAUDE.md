# imprenta-xlsx-napi

The Node binding for the spreadsheet writer. Compiled into `packages/xlsx`.
See the root `CLAUDE.md` for the rules that apply everywhere.

## Why it is a second addon and not a second entry point

The PDF engine carries a text shaper and a font subsetter, which are most of
that binary. A spreadsheet writer is a zip and some XML. An application that
only exports data has no business downloading a typography stack.

The cost is a second platform matrix in CI, and the rule below.

## The rule that cost a day

**No `#[global_allocator]` here.** Two Rust global allocators in one process do
not coexist, and a service that prints an invoice *and* exports a spreadsheet
loads both `.node` files. With the PDF addon loaded first, its next render
aborted the process at `shape.rs` with "the font contains no usable family" —
the bytes were being corrupted underneath it, and nothing about the fonts was
wrong. Loading them in the other order worked, which is what gave it away.

`packages/xlsx/test/together.test.ts` holds the line, in both orders. If a
global allocator is ever wanted for both, it has to be **one**.

## Everything else

- `lib.rs` is glue: `#[napi]` signatures in, results out. Logic goes below it,
  in `job.rs` and `stream.rs`, where `cargo test` reaches it without Node.
- **Nothing on the main thread.** Every job goes to libuv's pool.
- **The workbook crosses as a JSON string**, not a JS object.
- **Never panic across the boundary** — a panic inside a napi task cannot
  unwind and takes the process with it. That is not theoretical here; see
  above.
- **`Sink` is a typed enum, not a boxed writer.** Getting the bytes back at
  the end of a streamed workbook by downcasting a `Box<dyn Write + Seek>`
  compiled and returned an empty vector: the export would have been zero bytes
  with no error at all. There are two places a workbook can go, and that is a
  question the compiler can answer.
- **A file is flushed explicitly**, not left to `Drop`, which swallows the
  error. A truncated spreadsheet that reported success is the worst outcome
  available.
- **Unlike the PDF session, this one is `Send`** — no `Rc` anywhere in it — so
  each batch goes to the pool rather than to a dedicated thread. A thread per
  open workbook would cost a thread per concurrent export, for nothing.
- **One call in flight at a time.** A spreadsheet is written in order and two
  promises in flight have no order; a second call is refused, never queued.

## After changing anything here

`packages/xlsx/index.d.ts` is **generated**. Rebuild, or Node keeps the old
shape and serde drops the fields the old binary does not know:

```bash
pnpm --filter @imprentajs/xlsx build
```
