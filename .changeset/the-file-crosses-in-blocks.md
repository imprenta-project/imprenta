---
"@imprentajs/pdf": patch
---

The finished PDF crosses the wasm boundary in blocks, never as one buffer.

The writer has always produced the file in blocks that are never moved; the
join that made them contiguous for `imprenta_out_ptr` was the last place the
engine held a document twice — and linear memory never shrinks, so that peak
was the footprint (#7). The module now hands over exactly the blocks the
writer produced, through `imprenta_out_blocks` / `imprenta_out_block_ptr` /
`imprenta_out_block_len`, and the one contiguous copy is assembled on the JS
heap, where memory goes back.

`renderToFile` no longer assembles at all: the worker writes block by block,
so a long document exists whole nowhere but on disk — not in linear memory,
not in the Node heap. That was the real reason to want this.

The output is byte for byte what it was; the equivalence tests still compare
bytes and still pass. Measured on a 10,680-page ledger producing a 21.78 MB
file: **68.0 → 47.2 MB** of linear memory. The saving is the size of the
file, so it grows with the document.
