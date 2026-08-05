---
"@imprentajs/pdf": patch
---

A `<Row>` now lays its children side by side wherever it is, not only at the
top level of a document.

Nested — inside a `<Box>`, inside another `<Row>`, or inside a `<Header>` or
`<Footer>`, all of which are composed rather than walked — a row was treated as
a box and its children stacked. There was no diagnostic and no error: the
document rendered, and it was simply wrong. A two-column invoice header came
out as a logo above the company address, and a footer meant to put the page
number opposite the legal text put it underneath.

The cause was two copies of the same placement logic, of which only the one
used at the top level had ever been taught what a row is. They are one now, and
a test asserts the second panel's coordinates rather than the shape of the
output — the same lesson as the `spaceAfter` that used to be dropped inside a
row, which no assertion about shapes had caught either.
