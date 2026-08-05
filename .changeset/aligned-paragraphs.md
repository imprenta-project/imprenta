---
"@imprentajs/pdf": minor
"@imprentajs/react": minor
---

Three things a page needed and could not have: a paragraph set against the
right edge, one justified, and space after a box that falls outside its
background.

**A paragraph can be aligned, and justified.**

```tsx
<Text align="end">36.390,62 €</Text>
<Text className="text-right">Lanzagrava S.L.</Text>
<Text align="justify">…un párrafo legal que llena la caja…</Text>
```

Alignment existed only on a table column, so the one way to put a figure
against the right margin was to make it a table — and a table cannot be nested,
which meant it could not go in a header, a footer, or inside a box with a
background. An invoice is full of things that are not tables and still have to
line up on the right: a company address in the masthead, a total in its own
box. There was no way to say so.

It is the same `Align` a table column takes, deliberately: an amount under a
table has to line up with the amounts in it, and two notions of "the right
edge" would eventually disagree by a fraction of a point. `text-left`,
`text-right` and `text-center` resolve to it too — Tailwind spells alignment
with the same utility as size and colour, so those are recognised before
`right` can be looked up as either and reported as neither.

Left-aligned text is untouched and costs nothing: a line that is not shifted is
emitted exactly as before, with no box around it. That matters at the size this
engine is built for — a box per line across fifty thousand pages would be a box
per line.

`justify` is the odd one out and is not a fourth direction to shove the line
in: nothing moves, the spaces widen until the line reaches the far edge. Only
the spaces — scaling every advance would also hit the number and would set the
words in a font nobody chose. The last line of a paragraph keeps the width it
earned, and a line with no spaces in it is left alone rather than letter-spaced,
which is a different typographic decision and not one to make on somebody's
behalf.

What lands on the margin is the last glyph anybody can see. Almost every line a
breaker returns ends in a space, and a space that counts towards the line
leaves the text short by exactly its own advance — the same amount on every
line, so the right edge comes out straight but inset, which reads as flush
until something else on the page is set against the same margin. It hangs past
the edge instead.

**`spaceAfter` no longer grows the box it was meant to follow.**

At the top level it always behaved: the space became a spacer emitted after the
box. Composed — in a header, in a footer, or nested inside another container —
it was folded into the box's own bottom padding instead, so a box with a
background or a border grew by exactly that much and whatever followed stayed
welded to it. An author asking for room after a tinted panel got a taller
tinted panel, and nothing said otherwise.

The folding is right for a paragraph, which is what it was written for: text
has nothing painted behind it, and a paragraph has to sit the same whether or
not it has a neighbour. A decorated container does have something painted
behind it, and that is the whole difference. Nothing changes when `spaceAfter`
is zero — the box is returned as it was, with no wrapper around it.
