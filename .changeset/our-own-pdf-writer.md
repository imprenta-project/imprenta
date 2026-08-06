---
"@imprentajs/pdf": minor
---

The PDF is written by `imprenta-pdf-write` now, which is ours.

A page is serialised, compressed and forgotten the moment it is finished; what
survives it is its bytes in the output and one cross-reference entry. The
writer this replaces kept every painted page until the document closed and then
walked the whole collection twice into a buffer the size of the file.

It is built on the same two crates that one was — `pdf-writer` for the object
syntax and `subsetter` for the fonts — so subsetting, CID fonts and the
`ToUnicode` map are not re-derived. What it drops is everything this engine
cannot reach: no transparency groups, no patterns, no shadings, no clip paths,
no tagged structure, no encryption.

What it buys, beyond the memory: the file is **6% smaller**, rendering is
faster, and a defect that had every long document failing at around 2 400 pages
— a recursion once per page, on a target with no threads — cannot come back
with the next upgrade.

Colours, borders, radii, opacity, images, links and every other way a document
can look are unchanged, and the engine's whole test suite passes against it
untouched.
