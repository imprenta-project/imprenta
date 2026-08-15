---
"@imprentajs/react": patch
---

A sheet takes its rows as plain data, the way a table already did.

A React element per cell costs a fiber, an `Instance` and a props object for
the duration of one synchronous render — measured at 6,427 bytes of heap per
row against the 226 bytes of IR it produces (#11). `<Sheet>` now also takes a
`rows` prop: the same shape `<Row>` and `<Cell>` spell out — typed values,
formulas, formats, `className`, spans, anchored images — minus the elements.
Data rows are appended after whatever the children declare, so a header band
stays JSX and the hundred thousand rows under it are just an array.

The two forms go through the same functions and produce identical IR; the
test holds that line with equality, not similarity.

Measured on 200,000 rows of five cells:

| | children | `rows` prop |
| --- | ---: | ---: |
| heap after the host tree | 1,177 MB | **183 MB** |
| time to build it | 891 ms | **58 ms** |
| heap per row | 5,885 B | **916 B** |

Parity with `<Table rows>`, which was the goal, because the table was already
shipping.
