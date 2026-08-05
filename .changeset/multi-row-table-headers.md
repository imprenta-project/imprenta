---
"@imprentajs/pdf": minor
"@imprentajs/react": minor
"@imprentajs/cli": patch
---

A table can repeat several rows at the top of each page, not just one.

```tsx
<Table
  header={[
    { style: { background: '#11307D' }, cells: [{ text: '600000' }, { text: 'Compras' }] },
    { cells: [{ text: 'FECHA' }, { text: 'DESCRIPCIÓN' }, { text: 'DEBE' }] },
  ]}
  rows={apuntes}
/>
```

A grouped report — a ledger, a journal, a balance by period — wants to say two
things at the top of its table: which group this is, and what its columns mean.
Both have to come back when the group runs over the page, and with one row an
author had to choose which half of that question a reader on page 40 got
answered. A browser has never had this problem: two `<tr>` in a `<thead>` and
both repeat.

Several rows are still **one atom**. A repeated prefix is one indivisible block
by definition, so the rows are stacked into a single box before anything is
paginated — the packer, the painter and the streaming composer never learn
there was a second row, and none of them changed.

The IR now holds `header` as a list, which is a breaking change to anything
writing the IR by hand. `<Table header>` takes one row or an array, and
`Printer.openTable` normalises a single row too, so the streaming API is no
stricter than the declarative one: an author who wrote one row should not find
out from a deserialiser that the engine wanted a sequence.

The checks read the header as a list too, so every repeated row is checked like
the row it is — one short of a cell is exactly as wrong as a body row short of
one, and rather more visible.
