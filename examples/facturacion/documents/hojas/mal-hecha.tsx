import { Cell, Column, Row, Sheet, Workbook } from '@imprentajs/react/xlsx';

/**
 * A workbook that gets several things wrong on purpose.
 *
 * The spreadsheet counterpart of `mal-hecho.tsx`, and the way the panel's
 * rules are seen working. Every fault here is one somebody ships for real:
 * none of them stops the file opening, and all of them reach the recipient.
 */
export default function MalHecha() {
  return (
    <Workbook>
      <Sheet name="Ventas">
        <Column width={30} />
        <Column width={16} format="#,##0.00 €" />

        <Row className="font-bold">
          <Cell>Concepto</Cell>
          <Cell>Importe</Cell>
        </Row>

        <Row>
          <Cell>Licencia anual</Cell>
          <Cell value={1200} />
        </Row>
        <Row>
          <Cell>Implantación</Cell>
          <Cell value={3400} />
        </Row>
        <Row>
          {/* Text in a column of numbers. Looks identical on screen, and SUM
              skips it — so the total is short by nine hundred. */}
          <Cell>Formación</Cell>
          <Cell>900</Cell>
        </Row>

        <Row className="font-bold border-t">
          {/* Not a fault: a colSpan fills what it covers with blanks, so the
              merge rules cannot be broken from JSX at all. They are there for
              producers that write the IR themselves. */}
          <Cell colSpan={2}>Total</Cell>
        </Row>
        <Row>
          <Cell>Comentario</Cell>
          {/* Grey on white at 1.5 to 1. Fine on the screen it was picked on. */}
          <Cell className="text-slate-300">Pendiente de revisar</Cell>
        </Row>
      </Sheet>

      <Sheet name="Resumen">
        <Column width={30} />
        <Column width={16} />
        <Row>
          <Cell>Total de ventas</Cell>
          {/* The sheet is called "Ventas", not "Ventas 2026". Excel opens the
              file and shows #REF!. */}
          <Cell formula="SUM('Ventas 2026'!B2:B5)" />
        </Row>
      </Sheet>

      {/* A sheet a component forgot to fill. */}
      <Sheet name="Pendiente" />
    </Workbook>
  );
}
