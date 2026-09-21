---
"@imprentajs/pdf": patch
"@imprentajs/react": patch
---

A band measures itself. `height` on a header or footer is now optional, and
when given it is the least the band takes rather than a hard cap.

The number was the author's guess at how tall the band would come out once
laid out, and the guess had no way of being right when the content came from
data: a company set its logo to 113 pt wide, the logo was square, and a header
sized for a 62 pt letterhead painted it over the report title and the table
header on every page. Nothing warned. A company name that wraps, a longer legal
line, a two-line title — the same.

The engine now lays each band out once, before the first page is packed, with
the widest words its tokens are likely to take (a five-digit page count, a
signed total in the hundreds of millions), and reserves what it measures or
the declared height, whichever is taller. Streaming is unchanged: the budget is
fixed before a row arrives, and it is fixed the same way whether the document
is declared whole, fed in chunks or planned and painted in fragments.

The one case measuring once cannot cover — a page whose running total comes
out wider than the band was measured with and wraps — is reported as a
`band-overflow` warning naming the band, the page and the overrun in points.
The page is still emitted. Silence was the behaviour and the worst option.

`<Header>` and `<Footer>` take an optional `height`, documented as a minimum.
