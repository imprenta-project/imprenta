---
"@imprentajs/pdf": minor
"@imprentajs/xlsx": minor
"@imprentajs/react": minor
"@imprentajs/fonts": minor
"@imprentajs/cli": minor
---

One engine that runs everywhere, instead of ten binaries that run somewhere

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
  and the same module runs in a browser. A `Buffer` *is* a `Uint8Array`, so
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
