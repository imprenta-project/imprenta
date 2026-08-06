---
"@imprentajs/pdf": patch
---

Fix: text that changed colour mid-line could not be copied out of the PDF.

Every glyph carries the range of source text it stands for, and that range is
what becomes the document's `ToUnicode` map — the thing that lets a reader
select, copy, search or read the page aloud. On a line that changed style
without changing font, the second stretch was handed the ranges belonging to
the first: `Total 1.234,00` in two colours extracted as `Total Total 1.`.

Nothing on the page moved, which is what makes it worth writing down. The
document looked perfect in every viewer and every screenshot; only the text
underneath was wrong.

A bold stretch was never affected — a different weight is a different font, so
the shaper starts a new run and the walk restarted correctly by accident. It
took two stretches of the *same* face in different ink to show it.
