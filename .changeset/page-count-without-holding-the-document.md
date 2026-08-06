---
"@imprentajs/pdf": minor
---

`{{pages}}` no longer costs the whole document.

Nothing can know how many pages there are until the last one is packed, so a
footer saying "de 4 849" used to be bought by holding every painted page in
memory until then. Measured on a five-column ledger that was **twenty-three
times** the memory of the same document without it, and it was the largest
single reason a long one ran out.

It is now bought by walking the document twice: once to count the pages,
painting none of them, and once to paint them knowing the answer. The counting
pass goes through the same measurer and the same packer a real render does —
a cheaper estimate would be a second paginator, and the two would disagree on
exactly the documents that print their own length. The file is byte for byte
what holding produced.

A fed document has no second walk of its own, since its rows are gone once they
have been read, so a `Printer` printing `{{pages}}` keeps the *pieces it was
given* instead of the pages it painted. A row weighs a few hundred bytes where
the page it lands on weighs six kilobytes, and it costs the caller nothing.

Measured through the WebAssembly module, streaming a ledger with `{{pages}}`:

| pages | before | after |
| ---: | ---: | ---: |
| 668 | 64.8 MB | **22.6 MB** |
| 2 670 | 244.1 MB | **49.0 MB** |
| 10 680 | would not finish | **148.3 MB** |
