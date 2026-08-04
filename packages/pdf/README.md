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

## Installing

The compiled engine ships as a per-platform package —
`@imprentajs/pdf-darwin-arm64` and its siblings — pulled in through
`optionalDependencies` and picked at run time. macOS, Linux and Windows on x64,
plus arm64 on macOS and Linux.

## Status

Alpha. It works and is built test-first, but the API is not settled.

Apache-2.0 · [github.com/imprenta-project/imprenta](https://github.com/imprenta-project/imprenta)
