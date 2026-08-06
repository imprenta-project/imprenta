---
"@imprentajs/react": patch
---

`border-[#11307D]` on a cell now draws the colour it names, instead of quietly
drawing a thin black line.

A brand colour is a hex nobody has a Tailwind name for, which is why `bg-[#…]`
and `text-[#…]` both take one written out. The border did not: the arbitrary
value never reached the resolver, and what arrived instead was an empty
suffix — which the width branch reads as a bare `border`. So the colour was
dropped and the width was reset on the way past.

```tsx
<Cell className="border-b-2 border-[#11307D]" />
// wanted: a medium navy rule
// drew:   a thin black one
```

The worst kind of defect this project has, for the third time and in a new
place: the sheet opens, the rule is there, every test is green, and it is the
wrong rule. It was found by exporting a ledger whose title block is closed by a
brand-coloured line, and looking at the file.

A width written out is refused rather than accepted, because Excel has three
and no others — `border-[3pt]` gets the same message `border-8` already gets,
and so does `border-[2]`, which does land on one of the three. Whether the class
is honoured cannot depend on the number somebody guessed at: the three widths
have names, and one of them is what the author meant.

`border-b-[#11307D]` names a side and a colour together, which is how anybody
writes it having seen `border-b-2`. It used to be refused as "not a utility a
spreadsheet has" — true of the string and no use to somebody holding a brand
colour, because a written value swallows everything up to the bracket and the
side went with it. Both surfaces take it now: a cell, and **a page**, which had
the same gap and would otherwise have made one class mean two things depending
on what it was printed onto.
