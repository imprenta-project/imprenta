---
"@imprentajs/pdf": patch
"@imprentajs/xlsx": patch
---

A pointer past 2 GiB no longer comes back negative.

A wasm32 pointer crosses the boundary as an `i32` and JavaScript read it as
signed, so once a module's linear memory had grown past 2 GiB every pointer it
handed back was negative — and the very next line used it as an offset, which
died with "offset is out of bounds". An error about bounds reads like memory
corruption and points away from the real cause; it cost an afternoon before
anyone found it (#12).

Every export is now wrapped once, at instantiation, so its result is read as
an unsigned 32-bit value. One place rather than a `>>> 0` at each call site,
deliberately: a future export cannot forget. The regression test grows a real
instance past the signed line and round-trips a write through it.

This moves the wall from 2 GiB to wasm32's 4 GiB. What it really buys is the
failure mode: a workbook that is merely large now fails, if it fails, with an
answer that names the actual limit.
