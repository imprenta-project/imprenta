# imprenta-core

Format-neutral primitives shared by every output format. See the root
`CLAUDE.md` for the rules that apply everywhere.

## What belongs here

Only what a PDF, an XLSX and a DOCX would all need, described in the same
words:

- `units` — `Pt`, `Length`, `Edges<T>`, and the conversion constants. `Pt` is
  the one true internal unit; everything else converts on the way in.
- `color` — sRGB with straight (non-premultiplied) alpha.
- `envelope` — the outermost layer of every document: which format it targets
  and which schema version it was produced against, both checked before a
  single node is read.
- `diagnostic` — build-time findings, aggregated.

## What does not belong here

- Anything that mentions a page, a line, an atom, or a glyph. That is
  `imprenta-pdf`.
- Anything that mentions React, JSON field names of the PDF IR, or napi.
- A style *vocabulary* is welcome; a style *engine* is not.

The test for a new item: would XLSX want it in the same shape? If the answer
needs a qualification, it goes in the format crate instead.

## Rules

- **Adding a field to `Envelope` or bumping `CURRENT_SCHEMA_VERSION` is a
  breaking change to every producer**, in any language. Do it deliberately, and
  say so in the changeset.
- A version mismatch must be a **named error up front**, never a node quietly
  ignored halfway through a nine-thousand-page render.
- Diagnostics **aggregate**. One clipped column across a 9,000-page document is
  one diagnostic listing the pages, not nine thousand identical lines. Any new
  diagnostic must collapse the same way.
- No dependency beyond `serde`, `serde_json` and `thiserror`. This crate must
  stay cheap enough that a future format crate has no reason to avoid it.

## Testing

Unit tests in the same file, `#[cfg(test)]` at the bottom. Round-trip anything
serialisable — a shape that cannot be read back is a shape a producer cannot
write.
