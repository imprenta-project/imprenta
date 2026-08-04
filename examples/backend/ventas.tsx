import { Cell, Column, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';

export interface Line {
  ref: string;
  concept: string;
  on: Date;
  paid: boolean;
  price: number;
}

/**
 * The same data as `factura.tsx`, as something the recipient can total.
 *
 * Worth reading beside the invoice, because the two look alike and are not.
 * There, a price becomes glyphs on a page and the engine decides where the
 * page breaks. Here it stays a number, and what it looks like is decided by
 * whoever opens the file, in their locale, with their column widths.
 */
export default function Ventas({ lines }: { lines: Line[] }) {
  const total = lines.reduce((sum, line) => sum + line.price, 0);
  const money = '#,##0.00 €';

  return (
    <Workbook>
      <Sheet name="Ventas" freeze={{ rows: 1 }}>
        <Column width={10} />
        <Column width={38} />
        <Column width={14} />
        <Column width={10} />
        <Column width={16} format={money} />

        <Row className="bg-slate-100 font-bold" height={20}>
          <Cell>Ref.</Cell>
          <Cell>Concepto</Cell>
          <Cell>Fecha</Cell>
          <Cell>Pagado</Cell>
          <Cell className="text-right">Importe</Cell>
        </Row>

        {lines.map((line) => (
          <Row key={line.ref}>
            {/* Text, so a reference of 007 keeps its noughts. */}
            <Cell>{line.ref}</Cell>
            <Cell>{line.concept}</Cell>
            <Cell value={line.on} />
            <Cell value={line.paid} />
            {/* A number, so the column adds up. */}
            <Cell value={line.price} className={line.price < 0 ? 'text-red-600' : ''} />
          </Row>
        ))}

        <Row className="border-t font-bold">
          <Cell colSpan={4}>Total</Cell>
          {/* Both the formula and its answer: Excel recalculates it, and a
              script that only reads still sees the number. */}
          <Cell formula={`SUM(E2:E${lines.length + 1})`} cached={total} />
        </Row>
      </Sheet>
    </Workbook>
  );
}
