---
"@imprentajs/pdf": patch
---

Rendering is 25–45% faster, because every cell was being laid out twice.

Measuring a table cell shaped its text, and then the check for characters the
font cannot draw shaped it all over again — a second full trip through the
layout engine for every cell in the document, which on a ledger was **half of
all the time spent measuring**. The lines that measuring produces already hold
the answer: a glyph that came back as `.notdef` is a character the face could
not draw, and it carries the byte range it came from.

The same for a paragraph, which was shaped once to be checked and once to be
broken into lines.

A worker also gets its own shaper only where there is a second core to give it
to. Building one parses every font file the document declares, and inside a
WebAssembly module — where there are no threads at all — that was paid per
batch of rows, throwing away the shaping cache each time, in exchange for
nothing.

Measured through the module, streaming the same ledger:

| pages | before | after |
| ---: | ---: | ---: |
| 668 | 602 ms | **388 ms** |
| 2 670 | 2 274 ms | **1 307 ms** |
| 10 680 | 8 863 ms | **4 994 ms** |

`Shaper::layouts()` counts trips through the layout engine, so a test can hold
the line. No assertion about a height could see this one.
