# @imprentajs/pdf

The engine as one WebAssembly module, and the JavaScript that drives it. See
the root `CLAUDE.md` for the rules that apply everywhere.

## What is generated and what is written

```
imprenta-pdf.wasm   compiled from crates/imprenta-pdf-wasm — never committed
dist/               tsc output
src/module.ts       the ABI as JavaScript sees it; the only file that knows about pointers
src/engine.ts       Engine and the synchronous Printer. Blocks the calling thread
src/worker.ts       one engine on its own thread
src/pool.ts         the workers, and who gets one
src/index.ts        render / renderToFile — the async surface
src/stream.ts       Printer — the async surface for a document fed in pieces
```

There is no per-platform binary and no matrix. That is the whole reason this
package is WebAssembly: a `.node` had to be linked on a machine that could
produce that target, which is what `publish.yml` used to be three hundred lines
of.

## Build before you test

```bash
rustup target add wasm32-unknown-unknown   # once
pnpm --filter @imprentajs/pdf build
```

Any change under `crates/` needs this before a Node test means anything. A
stale module makes tests pass by comparing two documents that are both missing
the field you just added — the same trap the addon had, and it caught me once
during this migration.

## The API surface, and why it is that shape

- `render(ir, options)` → `{ pdf, pages, bytes, diagnostics }`
- `renderToFile(ir, path, options)` → the bytes never reach the calling thread
- `new Printer(page, options)` from `@imprentajs/pdf/stream` — feed a document
  in pieces
- `Engine` from `@imprentajs/pdf/engine` — **synchronous**, for when you are
  already off the main thread or in a browser

`ir` is a **JSON string**, not an object. Faster than walking an object across
the boundary, and it is also what comes back from a file, a queue or an HTTP
body.

The bytes come back as a **`Uint8Array`, not a `Buffer`**. `Buffer` is Node's
and the same module runs in a browser; a Node caller who wants the string
helpers wraps it, which costs nothing because a `Buffer` *is* a `Uint8Array`.

## Rules

- **Nothing runs on the calling thread.** A WebAssembly call is synchronous and
  a long document takes seconds. Every promise here is a worker, which is the
  same contract the addon had over libuv's pool. `Engine` is the escape hatch
  and says so.
- **Never cache a view over linear memory.** `alloc` can grow the memory and
  growing it detaches every `ArrayBuffer` handed out before. `Memory` builds a
  fresh view every time and `read` copies. A cached view is a bug that works
  until a document gets big.
- **Release the result.** `collect()` calls `imprenta_out_release` as soon as it
  has read. WebAssembly memory never goes back to the host, so an instance that
  kept its last PDF would hold the largest one it ever made.
- **Fonts load once, at pool start.** Keyed by the assets, so a service with one
  family keeps one pool and never re-copies a typeface.
- **One call in flight per document.** A `Printer` leases a worker and refuses a
  second call while one is running — a rejection, never a silent queue, because
  an unawaited loop would put the whole ledger back in memory by the one route
  streaming exists to avoid.
- **Tests import from `dist`.** A test that ran against `src` while the module
  was stale would prove nothing about what ships.

## Threads, and why there are none

Inside the module: none, and not for want of trying. `std::thread::spawn`
returns an error on `wasm32-wasip1-threads` with the pinned toolchain, and
`wasm32-unknown-unknown` needs a nightly rebuild of the standard library plus a
hand-written rayon spawn handler. See `crates/imprenta-pdf-wasm/CLAUDE.md`.

So *one engine* is single-core. The pool is what gets the cores back: across
different documents by dispatching them, and within one document by splitting
it — see below, where a 2,113-page ledger comes out **1.39× faster than the
native addon this replaced**.

## Sharding one document

For the case that matters most — a person waiting on one large document — the
engine is split across the pool. On 150,000 rows over 2,113 pages, twelve
cores:

| | |
|---|---:|
| one engine | 2,840 ms |
| **sharded across twelve** | **500 ms** |
| the native addon it replaced | 696 ms |

**1.39× faster than the addon, and 5.7× faster than one engine.** The same page
count, and every page numbered as one document.

### Why it cannot simply cut the rows into twelve

Because pagination is a function of every row before it. Cut a ledger twelve
ways and each piece starts a fresh page at the cut: measured, 2,124 pages where
the document has 2,113. Page numbers and running totals restart too. So it goes
in four passes and the order is the design:

1. **measure**, everywhere at once, each engine over its own rows. Only the
   heights cross — four bytes a row, against the kilobytes the row weighs.
2. **plan**, on one engine, over every height. Packing is arithmetic over
   heights and break flags; 2,113 pages take 7 ms. It says which row each page
   begins at, and how many pages there are.
3. **paint**, everywhere at once, each engine given a run of pages and told
   which number its first one carries. Because every piece starts where a page
   started, its pagination cannot drift.
4. **merge**, on one engine: one object id space, one page tree, one file.
   51 ms.

### The two things that make it worth doing

**Rows are measured once.** The engine that measured a row is the engine that
paints it — that is why the pool is *leased* rather than dispatched to.
Measuring is three fifths of a render, so doing it to plan and again to paint
cost exactly the margin over the addon: the first version of this landed at
688 ms, dead level with native. Reusing the measurement took it to 500.

The seam is the exception: the page that straddles two engines needs rows the
first never measured, so the host hands over that page's worth to be measured
there. One page in twelve hundred.

**A fragment is told the total.** `{{pages}}` normally forces the composer to
hold every page, because nothing can know the total until the last page is
packed. The plan knows it before any painting starts, so a sharded render
prints "3 of 2113" and still streams. Step 2 is what makes that free.

### What is refused, and why

`shardable()` in `src/shard.ts` takes a document that is exactly one table with
no declared header row, and no running totals. Everything else renders on one
engine — not as a fallback, but because:

- another node before the table, or a repeated header row, is another **atom**,
  and the plan counts atoms. Every page boundary after it would be off by one.
- a **running total** is not a height, and the planner packs heights. Until it
  carries the contributions too, "suma y sigue" has to go the one-engine way,
  where it is correct.

Both are worth lifting. Neither is worth guessing at.

## Testing

`packages/pdf/test/` is for what genuinely needs a runtime. Engine behaviour
belongs in `cargo test`, where it runs in a fraction of the time. What is here
and why:

| | |
|---|---|
| `module.test.ts` | the module imports nothing, every export is prefixed, instantiation stays under a millisecond |
| `browser.test.ts` | no static `node:` import in the browser-facing files, and a render with `Buffer` deleted and the module handed in as bytes |

| `engine.test.ts` | the synchronous surface, and that it cannot disagree with the promise-returning one |
| `pool.test.ts` | the event loop stays free, documents queue, a session holds an engine and gives it back |
| `shard.test.ts` | a split document has the pages the whole one has, and is faster |
| `render.test.ts`, `stream.test.ts` | the public API, unchanged from when it was an addon |

What no test here can reach is a real browser, so it was checked by hand: the
module fetched, a 900-row invoice rendered to 14 pages in **79 ms**, a second
render on the same instance, 800 rows streamed while holding 200 atoms, and the
spreadsheet writer loaded on the same page — in Chrome, from a plain static
server, with `crossOriginIsolated` false and not one console message. Worth
repeating whenever the loading path changes; the recipe is a page that imports
`dist/engine.js`, `fetch`es the `.wasm`, and prints what comes back.

Two of those are load-bearing in a way that is easy to miss:

- **`imports nothing`** is the whole portability argument. It cannot fail here
  — it fails on somebody else's runtime, as "works on mine".
- **content fed in pieces is byte for byte what the same content declared whole
  produces.** Keep it, and keep it comparing bytes.
