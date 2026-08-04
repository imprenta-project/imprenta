# @imprentajs/cli

**Preview and build Imprenta documents.** `init`, a live preview that shows the
real PDF, a `build` that compiles documents exactly the way the preview does,
and rules that say whether a document will survive being printed.

```bash
npm i -D @imprentajs/cli@alpha

npx imprenta init    # a project that renders straight away
npx imprenta dev     # open the preview
npx imprenta build   # every document to a file
```

`init` writes `imprenta.config.ts`, a `documents/` folder with a working
invoice, and a line in `.gitignore`. It never overwrites anything already
there, so it is safe to run inside a project you already have.

## The preview shows the real file

Not a rendering of the page — the PDF the engine actually produced, in the
browser's own viewer, at the page's own proportions. Save a document and it
re-renders. A spreadsheet cannot be shown that way because no browser opens
one, so it is drawn as the grid its IR declares, and the pane says so rather
than letting you believe you have seen the artefact.

## The checks

Along the bottom is a panel that says whether the document is any good. Not
whether it is handsome — whether it will survive being printed, which is a
question you cannot answer by looking at a screen:

- type too small to read once it is ink
- margins outside what a printer can reach
- an image whose dpi is too low for the size it is placed at
- a colour that will not survive being printed grey
- a box wider than the page leaves room for

Each names what it found and where. `imprenta build --strict` fails a pipeline
on any of them.

## Status

Alpha. It works and is built test-first, but the API is not settled.

Apache-2.0 · [github.com/imprenta-project/imprenta](https://github.com/imprenta-project/imprenta)
