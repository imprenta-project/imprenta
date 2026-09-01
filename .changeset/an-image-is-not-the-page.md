---
"@imprentajs/pdf": patch
---

Fix: one image that was never handed over took a whole header down with it.

A band is composed as one piece, so anything it could not build made the band
`None`, and a `None` band is not painted. A letterhead with a logo the
producer failed to hand over therefore lost the title, the invoice number, the
customer and the page count as well, on every page, and the render returned no
error and no diagnostic to say so.

An image nobody handed over is now reported as an error diagnostic that names
it, `unknown-image`, and left out. What surrounded it prints: the band keeps
its text, and inside a row the picture keeps its declared column, so nothing
beside it moves. `BuildError::UnknownAsset` is gone, and with it the last way
composing a node could fail — a band can no longer be refused by anything it
contains, which is the property that makes the fix hold rather than a case
that happens to be handled.

It is the sheet side's rule stood on its head, deliberately. A workbook that
names an image nobody supplied stops the write, because the package would be
malformed and the hole is not noticed until a customer opens it. A page is not
malformed by a missing picture, and the alternative here was not a hole where
the logo was but a document with no letterhead at all, silently, which is the
one outcome an engine should never choose. The diagnostic is an error, so the
dev server shows it and CI can refuse it.

Bytes that were handed over and cannot be read are still refused, as before:
a producer that passed something broken is told so at the door, and that is a
different mistake from never having passed anything.
