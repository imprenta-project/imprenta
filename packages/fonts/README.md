# @imprentajs/fonts

**Fetch and cache the faces a document is set in.** Needs no CLI and no config
file, so a server can load its fonts at startup and a build can load the same
ones.

```bash
npm i @imprentajs/fonts@alpha
```

```ts
import { google, loadFonts } from '@imprentajs/fonts';

const fonts = await loadFonts([
  google('Roboto', { weights: ['regular', 'bold'] }),
  { path: './assets/BrandSerif.ttf', weight: 'bold' },
]);

const { pdf } = await render(ir, { fonts });
```

`loadFonts` gives back exactly the `{ weight, italic, data }[]` that
[`@imprentajs/pdf`](https://www.npmjs.com/package/@imprentajs/pdf) takes, with
nothing to write in between. It mixes Google faces with local files in the
order given, because a brand's own typeface is not on Google and its body text
usually is.

## Downloaded once, then off disk

Faces are cached into `.imprenta/fonts` the way `next/font/google` self-hosts
what it downloads, so a build with no network works. Concurrent requests for
the same face dedupe to one download.

The bytes are validated before anything is cached. Google's CSS API serves
woff2 to a modern client and EOT to something old enough, and the engine reads
neither — a bad file in the cache would otherwise surface much later as an
unreadable-font error a long way from its cause.

## Status

Alpha. It works and is built test-first, but the API is not settled.

Apache-2.0 · [github.com/imprenta-project/imprenta](https://github.com/imprenta-project/imprenta)
