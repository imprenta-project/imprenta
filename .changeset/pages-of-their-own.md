---
"@imprentajs/pdf": patch
"@imprentajs/react": patch
"@imprentajs/cli": patch
---

Sections: pages of their own, with their own size, margins, bands and
numbering.

A report's cover has no header, no footer and wider margins, and its body is
numbered from one. A document had one page setup and one pair of bands for
every page, so the only way to get there was two documents stitched together,
with the body's page numbers unable to know the cover existed.

`{ "t": "section", "page": {…}, "header": null, "footer": null, "numbering":
"none", "children": […] }` — `<Section footer={false} numbering="none">` in
React. A section starts on a new page and the document's own settings resume
on a new page after it. Everything it leaves out is the document's: a page
field it does not name, a band it does not mention. `null` (or `false`) takes
a band away, and a `<Header>` or `<Footer>` written inside the section is its
own. `numbering` is `"continue"`, `"none"` — the pages carry no number and
`{{pages}}` does not count them — or `{ "restart": 1 }`.

A section drains what is in hand before it switches page, painted with the
bands it was laid out under, so pages of two sizes are never confused and a
section fed through a `Session` is byte for byte the section declared whole.
The CLI's checks walk into sections and judge their margins and widths against
the section's own page.

Also fixed on the way: a parity break (`pageBreak` to `odd` or `even`) counted
pages from the first one still held in memory rather than from the start of
the document, so on any document long enough to release pages a chapter meant
for the recto could open on the verso.
