---
"@imprentajs/pdf": patch
---

Fix: a header or footer went missing from every page but the last few.

A document is painted and dropped as it goes, every few hundred atoms, and
those pages were released through a code path that built no bands at all. So a
declared ledger of 1 200 rows came out with a footer on **one page of
eighteen**, and a header on none. Streamed and sharded documents had it too.

Every test written around the feature used a document short enough never to
reach the first flush, which is why it survived: at 200 rows everything is
painted at the end and everything is correct.

`Walk` now carries what a band is built from and flushes with it, so a page
released half-way through a document gets the same header and footer as one
painted at the end. There are four tests, and each of them counts: a footer on
every page is exactly one more text run per page than the same document without
one.
