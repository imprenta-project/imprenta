# @imprentajs/pdf

**The Imprenta page engine, as a native Node addon.** Measures text, paginates
it, and places every glyph itself — no browser, no HTML, no CSS.

```bash
npm i @imprentajs/pdf@alpha
```

```ts
import { render } from '@imprentajs/pdf';

const { pdf, pages, diagnostics } = await render(ir, {
  fonts: [{ weight: 'regular', italic: false, data: robotoBytes }],
});
```

`ir` is a **JSON string**, not an object: it is faster across the addon
boundary, and it is also what arrives from a file, a queue or an HTTP body. The
usual way to produce it is [`@imprentajs/react`](https://www.npmjs.com/package/@imprentajs/react).

## What it is for

Documents whose pagination is the point — a fifty-thousand-page ledger, an
invoice with a carried-forward total, a report whose header depends on what is
on the page. Pages are painted and released as content arrives, so memory stays
flat per page however long the document is.

- `render(ir, options)` → `{ pdf, pages, bytes, diagnostics }`
- `renderToFile(ir, path, options)` — no Buffer ever exists; use it for anything large
- `new Printer(page, options)` from `@imprentajs/pdf/stream` — feed a document in pieces

Await every `Printer` call before the next, and send rows in batches of a
hundred to a thousand: one at a time costs a round trip each and is *slower*
than not streaming at all.

## One engine, every runtime

The engine is a single WebAssembly module that **imports nothing at all** — no
Node-API, no WASI, no shim — so there is no per-platform package to install and
nothing to pick at run time. The same file runs in Node, the browser, Deno, Bun
and on an edge worker, and on every platform a native build matrix leaves out:
musl, which is to say Alpine, which is to say a great many Docker images.

The calls above go to a worker, so nothing blocks the thread that answers
requests. In a browser there is no worker to hide behind and no filesystem to
read the module from, so reach for the engine directly:

```ts
import { Engine } from '@imprentajs/pdf/engine';

const engine = await Engine.load({ wasm, fonts });
const { pdf } = engine.render(ir);   // synchronous: put it in a Worker
```

No cross-origin isolation, no `SharedArrayBuffer`, no COOP/COEP headers.

The bytes come back as a `Uint8Array` rather than a `Buffer`, because `Buffer`
is Node's. `Buffer.from(pdf)` gets the string helpers back at no cost.

**Speed.** A WebAssembly module has no threads, so one engine renders on one
core. The pool gets the cores back — across documents by dispatching them, and
within one long document by splitting it. On 150,000 rows over 2,113 pages,
twelve cores:

| | |
|---|---:|
| one engine | 2,840 ms |
| **split across twelve** | **500 ms** |
| the native addon this replaced | 696 ms |

Same page count, same numbering, one file. Splitting happens on its own for a
document long enough to be worth it; `shard: false` turns it off.

## Status

Alpha. It works and is built test-first, but the API is not settled.

Apache-2.0 · [github.com/imprenta-project/imprenta](https://github.com/imprenta-project/imprenta)
