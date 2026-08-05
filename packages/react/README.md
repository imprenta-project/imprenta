# @imprentajs/react

**Declare a PDF or a spreadsheet as React components.** A real reconciler, not
a tree walk: components, hooks and context all work, and what comes out is
Imprenta's IR rather than HTML.

```bash
npm i @imprentajs/react@alpha
```

```tsx
import { B, Document, Table, Text, render } from '@imprentajs/react/pdf';

const Invoice = ({ number, items }) => (
  <Document margin={40}>
    <Text size={22}><B>FACTURA</B></Text>
    <Text size={11}><B>{number}</B></Text>
    <Table
      columns={[{ width: 46 }, { width: 'auto' }, { width: 90, align: 'end' }]}
      header={{ cells: [{ text: 'Ref.' }, { text: 'Concepto' }, { text: 'Importe' }] }}
      rows={items.map((i) => ({ cells: [{ text: i.ref }, { text: i.concept }, { text: i.total }] }))}
    />
  </Document>
);

const ir = await render(<Invoice number="FV-2026-00418" items={items} />);
```

Hand that IR to [`@imprentajs/pdf`](https://www.npmjs.com/package/@imprentajs/pdf).

## One import path per format

`@imprentajs/react/pdf` and `@imprentajs/react/xlsx` are separate sets of
elements on purpose. A page is measured and painted by the engine; a sheet is a
value and a type that Excel draws later. A shared `<Text>` would carry props
that silently do nothing in one of the two, which is the failure this design
refuses.

## It depends on React and nothing else

Not on the engine. A document can be declared anywhere — a browser, a worker,
a machine where the engine is not installed — and rendered somewhere else
entirely.

## Tailwind

`className` works. No CSS is involved at any point: a class is looked up and a
number or a colour comes out. A class the page cannot honour — `flex`,
`hover:`, `w-1/2` on a box — is an error by name at render time, not a document
that quietly came out wrong.

## Status

Alpha. It works and is built test-first, but the API is not settled.

Apache-2.0 · [github.com/imprenta-project/imprenta](https://github.com/imprenta-project/imprenta)
