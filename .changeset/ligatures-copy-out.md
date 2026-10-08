---
"@imprentajs/pdf": patch
---

Fix: a word with a ligature in it copied out of the PDF with a letter missing.

Roboto sets "fi" as one glyph, and the glyph was recorded as standing for the
"f" alone. The page looked right; selecting "fin" gave "fn", a search for
"fiscal" or "configuración" found nothing, and a screen reader skipped the
letter — on every page of every document set in a font with ligatures. The
letters a ligature swallows are now part of the text its glyph stands for, in
either direction of writing.
