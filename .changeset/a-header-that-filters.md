---
"@imprentajs/xlsx": minor
"@imprentajs/react": minor
"@imprentajs/cli": minor
---

A header row can carry the autofilter.

```tsx
<Row filter className="bg-slate-100 font-bold">
  <Cell>Fecha</Cell>
  <Cell>Concepto</Cell>
  <Cell>Importe</Cell>
</Row>
```

The dropdowns Excel puts on a table so whoever opens it can sort and filter
each column. It is the first thing anybody does with an export of twenty-five
columns, and there is a reason for shipping it switched on rather than leaving
it to them: with a frozen pane in the way, turning it on by hand means
selecting the right range starting at the labels, and a slip makes Excel read
the title row as the header and offer to filter by `MOVIMIENTOS CONTABLES`.

Marked **on the row** rather than declared as a range, because the range ends
at the last row of the sheet — and a producer feeding a million rows in batches
has not got there yet. The engine works it out when the sheet closes, which is
the only moment anybody can, and that makes the declared and the streamed sheet
say the same thing with the same words.

The range covers the labels and everything under them. Excel reads the first
row of an autofilter as the header, so a range that started below would filter
by the first row of data — and it would open either way.

Everything under them means **everything**, including a total row. There is no
way to say where the table stops, because the sheet's last row is the only end
a streaming producer can be asked for — so a sheet whose totals sit under the
data has them inside the filter, offering `Sumas del ejercicio` as a value to
filter by and hiding the totals the moment anybody filters. Put the filter on a
table that runs to the end of its sheet, which in practice means giving the
totals a sheet of their own or leaving them off. `examples/facturacion` marks
the one sheet of three where that holds, and says so.

`imprenta dev` draws the dropdowns. The grid is built from the IR and the IR
carries a flag on the row, so a marked header looked identical to an unmarked
one — and the slip this exists to prevent is marking the *wrong* row. The
preview is the one place that is visible before somebody opens the export.

One to a sheet, which is what Excel has. Two rows asking for it is refused and
names both, rather than letting the second quietly win: it is what somebody
copying a header block gets, and the file opens with the wrong half filterable.
