---
"@imprentajs/react": patch
---

A table row's `style` is now resolved the way a box's is.

`RowProps.style` is typed as a box's props, and it was handed to the engine
exactly as written: a colour string where the engine holds a border per side, a
single number where it holds four, and a `className` nobody ever looked at. A
row asking for a hairline underneath produced a document the engine could not
read at all — `invalid type: string, expected struct Edges` — while a `<Box>`
with the same three words drew one.

Two fields, `background` and `radius`, happen to have the same shape on both
sides. That is what made the rest look like it worked too, and it is why the
test asserts a row's resolved style against a box's rather than against a
literal: whatever a box learns to accept, a row now accepts with it.
