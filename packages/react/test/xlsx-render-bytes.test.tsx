import { describe, expect, it } from 'vitest';
import { Cell, Column, Image, Row, Sheet, Workbook } from '../src/xlsx/elements.js';
import { render, toWorkbook } from '../src/xlsx/render.js';

/**
 * `render` hands back UTF-8 bytes rather than one JS string. V8 caps a string
 * at 512 MiB of characters, and a workbook of fourteen million cells died
 * there — while serialising, before the engine was involved at all (issue
 * #12). The writer has always taken bytes; encoding in pieces here removes
 * the cap and one full copy of the IR from the heap.
 *
 * The bytes must decode to exactly what `JSON.stringify` of the IR says —
 * not just an equivalent document — so that nothing downstream can tell the
 * encoding changed.
 */
describe('render as bytes', () => {
  const book = (
    <Workbook>
      <Sheet name="Ventas – façade ✓" freeze={{ rows: 1 }}>
        <Column width={12} format="#,##0.00" />
        <Column width={40} />
        <Row height={18} className="font-bold" filter>
          <Cell value={new Date(Date.UTC(2026, 1, 3))} format="dd/mm/yyyy" />
          <Cell colSpan={2}>Descripción, "entrecomillada" y con \ barra</Cell>
        </Row>
        <Row>
          <Cell value={-0.5} />
          <Cell formula="SUM(A1:A9)" cached={12.5} />
          <Cell value={false} />
        </Row>
        <Row>
          <Cell />
          <Cell>
            <Image src="logo" width={90} align="center" offset={{ x: 4, y: 2 }} />
          </Cell>
        </Row>
      </Sheet>
      <Sheet name="Vacía como llega" rows={[{ cells: [{ value: '007' }] }]} />
    </Workbook>
  );

  it('produces the very bytes JSON.stringify of the IR produces', async () => {
    const bytes = await render(book);

    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(new TextDecoder().decode(bytes)).toBe(JSON.stringify(await toWorkbook(book)));
  });

  it('still matches when the rows outnumber the encoder slice', async () => {
    const rows = Array.from({ length: 10_000 }, (_, i) => ({
      cells: [{ value: i }, { value: `fila ${i}` }],
    }));
    const long = (
      <Workbook>
        <Sheet name="Larga" rows={rows} />
      </Workbook>
    );

    const bytes = await render(long);

    expect(new TextDecoder().decode(bytes)).toBe(JSON.stringify(await toWorkbook(long)));
  });
});
