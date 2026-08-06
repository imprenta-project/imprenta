---
"@imprentajs/pdf": minor
---

An engine that renders one very large document gives the memory back.

WebAssembly has no instruction to shrink a linear memory, so an instance's
footprint is the high-water mark of the largest document it has ever rendered,
for as long as it lives — once per engine in the pool. A service that prints
one ledger a month and invoices the rest of the time carried that ledger's
memory from the day it first arrived.

A worker now takes a fresh instance once a document leaves one swollen. The
compiled module is reused, so the warm-up is not paid again, and it happens
after the reply rather than before it, so nobody waits for it.

```ts
await render(ir, { fonts, recycleAbove: 64 * 1024 * 1024 }); // the default
await render(ir, { fonts, recycleAbove: Infinity });         // never recycle
```

Measured: 21.1 MB held after a 423-page ledger, 2.6 MB with it.
