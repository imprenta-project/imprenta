---
"@imprentajs/pdf": patch
"@imprentajs/react": patch
"@imprentajs/cli": patch
---

A table cell can cover several columns, so a header can name a group of them.

```tsx
<Table
  columns={[{ width: 120 }, { width: 90, align: 'end' }, { width: 90, align: 'end' }]}
  header={[
    { cells: [{ text: 'Cuenta' }, { text: 'Importe del ejercicio', colSpan: 2 }] },
    { cells: [{ text: '' }, { text: 'Debe' }, { text: 'Haber' }] },
  ]}
  rows={apuntes}
/>
```

Several header rows arrived first, and they only got half way: a report could
say which group this is and what its columns mean, but not that two of those
columns are one thing said twice. The name had to sit over the left half of
its pair and pretend.

The cell takes the x of the first column it covers and the width of all of
them, so it lines up with them exactly. **The columns it covers belong to it**
— the next cell in the row starts after them, so a spanned row is written
short rather than padded with the blanks a spreadsheet wants. It is `colSpan`
and not `span` for the same reason: a sheet cell already calls it that, and
`<Span>` is already a run of text.

Alignment and overflow still come from the first column covered, because
neither is a property of a cell here: a table's style is the caller's, and the
column is where the caller put it.

Nothing else moved. The packer never learns a row has fewer cells than
columns, and a span past the last column stops at the last column rather than
overlapping what is already placed.

The `ragged-row` check counts the columns a row covers rather than the cells
it holds, so a grouped header is no longer reported as an error — it used to
say the engine would drop the difference, which it does not.
