---
"@imprentajs/pdf": patch
"@imprentajs/react": patch
"@imprentajs/cli": patch
---

Anchors, links inside the document, an outline, and `{{pageof:id}}`.

A table of contents needs to know which page each chapter lands on, and it is
printed before any of them. There was no way to ask, so a producer laid each
chapter out on its own to count its pages and then laid out the whole body
again — twice the work, and exact only because every chapter happened to open
a page.

- `{ "t": "anchor", "id": "resumen", "bookmark": "Resumen ejecutivo", "level": 1 }`
  names the place where what follows it lands. It takes no room and keeps
  with the next thing, so it never names the foot of the page before.
  `<Anchor id bookmark level />` in React.
- A `link` whose `href` is `#resumen` jumps there. Written as a named
  destination resolved once at the end of the file, because the contents page
  is written long before the chapter it points at and a page is in the file
  the moment it closes.
- A `bookmark` puts the anchor in the outline a PDF reader shows beside the
  pages, nested by `level`; a document with one opens with it showing.
- `{{pageof:resumen}}` — `<PageOf id="resumen" />` — prints the number of the
  page the anchor landed on, in body text, table cells, list items and bands.
  It is the number the page carries: behind an unnumbered cover, the first
  page of the body is 1.

A document that prints a page reference is walked twice, as one that prints
`{{pages}}` already was: once to count, painting nothing, then once to paint.
The number printed can be a different width from the one counted with, and a
line that rewraps can move what follows it to another page, so the painted
document is checked against the count and painted again in the rare case it
moved. A document that uses no reference pays nothing at all — not even a scan
of its rows beyond looking for the token.

Every mistake is said out loud: a link or a reference to an anchor nobody
declared is `unknown-anchor`, a name given twice is `duplicate-anchor`, a
reference to a page that carries no number is `unnumbered-page`, and a
reference in a document fed in pieces — which has no second walk to answer it
with — is `page-reference-unavailable` rather than a wrong number. The CLI no
longer calls a `#name` link one a reader cannot follow.
