---
"@imprentajs/pdf": patch
"@imprentajs/react": patch
---

Layout fixes found building a long report.

- **A heading with room under it no longer strands at the foot of a page.**
  The space after a block is an atom of its own, and it was emitted without
  the block's `keepWithNext` — so the chain ended at the gap, the heading and
  its gap fitted at the foot of the page, and the table it introduced went
  overleaf without it. Any `keepWithNext` with a `spaceAfter` did this.
- **A growing spacer pushes everything after it to the foot, not just the next
  line.** A signature and a date after one: the gap left room for the first
  only, and the date went onto a page of its own. It now leaves room for
  everything up to the next forced break, and takes nothing when that does
  not fit on the page. The look-ahead stops at the foot of the page, so it
  costs at most a page of arithmetic and only where something grows.
- **A block taller than a whole page is reported** as `page-overflow`. It
  cannot be split and was painted past the foot without a word.
- **A list can sit inside a box.** It was refused as `not-inline`.
- **A canvas can paint in more than one colour.** `{ "op": "fill", "color" }`
  and `{ "op": "stroke", "color", "width" }` paint the path traced since the
  previous one and start another, so a chart's series, grid and axes share one
  canvas. A canvas without them paints as it always did.

Measured against the previous release on the ledger, prose and text-path
benchmarks: no change outside run-to-run noise.
