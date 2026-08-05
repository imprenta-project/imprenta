---
"@imprentajs/pdf": patch
---

Text too wide for its box is now reported, instead of being painted over the
edge in silence.

It happens when nothing in the line can be broken — a URL, a reference code, an
IBAN written without spaces. The engine breaks what it can, runs out of places
to break, and paints the rest past the edge. Nothing said so, which made it the
worst kind of defect this project has: the page looks deliberate, every test is
green, and a line of it is over the side. It went unnoticed here through an
entire invoice design until somebody happened to look at the file.

```
text-overflow — "ref=XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX" is 268pt wide
                 where 112pt were available, so it is painted outside its box
```

The engine is the only place this can be caught. The checks in the CLI read the
IR and have no fonts, so `wider-than-the-page` can tell that a *declared* width
is too big and never that a *measured* line is.

A table cell has had this all along, as `cell-overflow`. The two are the same
idea in the two places text is measured, and the names are a pair on purpose —
what was missing was the paragraph, which is most of a document.

One report per paragraph, naming its widest line and quoting the first forty
characters, because a warning per line of a long paragraph is a warning nobody
reads. It is a warning rather than an error: the document is still usable, and
the author may have judged that a millimetre over a box edge does not matter.

This says what happened; it does not fix it. Breaking inside a word that has
nowhere else to break — CSS's `overflow-wrap: anywhere` — is a separate
decision and not one to make on an author's behalf.
