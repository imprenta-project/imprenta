import { describe, expect, it } from 'vitest';
import { Theme } from '../src/element.js';
import { Cell, Column, Image, Row, Sheet, Workbook } from '../src/xlsx/elements.js';
import type { IrWorkbook } from '../src/xlsx/ir.js';
import { toWorkbook } from '../src/xlsx/render.js';

const first = (book: IrWorkbook) => book.sheets[0];
const cells = (book: IrWorkbook, row = 0) => first(book).rows?.[row]?.cells ?? [];

describe('a workbook', () => {
  it('keeps its sheets in the order they were written', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="Ventas" />
        <Sheet name="Resumen" />
      </Workbook>,
    );

    expect(book.sheets.map((s) => s.name)).toEqual(['Ventas', 'Resumen']);
  });

  it('refuses two tabs with the same name', async () => {
    // Excel will not open it, and what it says names neither of them.
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="Ventas" />
          <Sheet name="ventas" />
        </Workbook>,
      ),
    ).rejects.toThrow(/two sheets are called/);
  });

  it('refuses a tab name longer than Excel allows', async () => {
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name={'x'.repeat(32)} />
        </Workbook>,
      ),
    ).rejects.toThrow(/32 characters, and Excel allows 31/);
  });

  it('refuses a character Excel forbids on a tab', async () => {
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="Ventas/2026" />
        </Workbook>,
      ),
    ).rejects.toThrow(/forbids on a tab/);
  });

  it('needs at least one sheet', async () => {
    await expect(toWorkbook(<Workbook />)).rejects.toThrow(/at least one <Sheet>/);
  });
});

describe('what is in a cell, and what type it is', () => {
  it('keeps text as text, leading zeros and all', async () => {
    // The reason children are always text: a reference like 007 is not seven.
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell>007</Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(cells(book)[0].value).toEqual({ t: 'text', v: '007' });
  });

  it('carries a number through as a number', async () => {
    // The whole difference from a printed page. Written as text, SUM returns
    // zero and the recipient gets a total that is wrong.
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell value={1200} />
            <Cell value={-125.25} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(cells(book)[0].value).toEqual({ t: 'number', v: 1200 });
    expect(cells(book)[1].value).toEqual({ t: 'number', v: -125.25 });
  });

  it('tells a boolean from the word', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell value={true} />
            <Cell>true</Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(cells(book)[0].value).toEqual({ t: 'bool', v: true });
    expect(cells(book)[1].value).toEqual({ t: 'text', v: 'true' });
  });

  it('turns a Date into the serial Excel keeps underneath one', async () => {
    // 2026-08-03 is 46237, which is the number the Rust side arrives at from
    // the calendar. The two agreeing is the point.
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell value={new Date(Date.UTC(2026, 7, 3))} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(cells(book)[0].value).toEqual({ t: 'date', v: 46237 });
  });

  it('puts a time of day after the point', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell value={new Date(Date.UTC(2026, 7, 3, 12))} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(cells(book)[0].value).toEqual({ t: 'date', v: 46237.5 });
  });

  it('is blank when there is nothing in it', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(cells(book)[0].value).toEqual({ t: 'blank' });
  });

  it('takes a formula, with or without the equals sign', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell formula="SUM(A1:A9)" cached={42} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(cells(book)[0].value).toEqual({
      t: 'formula',
      v: { formula: 'SUM(A1:A9)', cached: 42 },
    });
  });

  it('refuses a cell given both a value and a formula', async () => {
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="H">
            <Row>
              <Cell value={1} formula="SUM(A1:A9)" />
            </Row>
          </Sheet>
        </Workbook>,
      ),
    ).rejects.toThrow(/a value or a formula/);
  });

  it('refuses a number a spreadsheet cannot hold', async () => {
    // Infinity has no representation in the file. Writing it makes Excel
    // refuse the whole workbook for the sake of one cell.
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="H">
            <Row>
              <Cell value={1 / 0} />
            </Row>
          </Sheet>
        </Workbook>,
      ),
    ).rejects.toThrow(/cannot hold/);
  });
});

describe('the shape of a sheet', () => {
  it('takes column widths, before the rows', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Column width={12} />
          <Column width={40} format="#,##0.00" />
          <Row>
            <Cell>a</Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(first(book).columns).toEqual([
      { width: 12 },
      { width: 40, style: { format: '#,##0.00' } },
    ]);
  });

  it('says so when a column comes after a row', async () => {
    // Excel writes the columns before the data, and an author who put one
    // half way down has almost certainly mistaken it for a cell.
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="H">
            <Row>
              <Cell>a</Cell>
            </Row>
            <Column width={12} />
          </Sheet>
        </Workbook>,
      ),
    ).rejects.toThrow(/columns come first/);
  });

  it('turns a colSpan into a merge and keeps the columns after it straight', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell colSpan={3}>Total</Cell>
            <Cell value={99} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(first(book).merges).toEqual([{ fromRow: 0, fromColumn: 0, toRow: 0, toColumn: 2 }]);
    // The number has to land in D, not in B.
    expect(cells(book)).toHaveLength(4);
    expect(cells(book)[3].value).toEqual({ t: 'number', v: 99 });
  });

  it('freezes the rows it was asked to', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H" freeze={{ rows: 1 }} />
      </Workbook>,
    );

    expect(first(book).freeze).toEqual({ rows: 1 });
  });
});

describe('formatting', () => {
  it('resolves classes on a cell, a row and a column', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="H">
          <Row className="bg-slate-100 font-bold">
            <Cell className="text-right">Importe</Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(first(book).rows?.[0].style).toEqual({
      fill: '#f1f5f9',
      font: { bold: true },
    });
    expect(cells(book)[0].style).toEqual({ align: { horizontal: 'right' } });
  });

  it('takes a theme wrapped round the whole workbook', async () => {
    const book = await toWorkbook(
      <Theme colors={{ brand: '#1b3a5c' }}>
        <Workbook>
          <Sheet name="H">
            <Row>
              <Cell className="text-brand">Hola</Cell>
            </Row>
          </Sheet>
        </Workbook>
      </Theme>,
    );

    expect(cells(book)[0].style?.font?.color).toBe('#1b3a5c');
  });

  it('says which class it could not use', async () => {
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="H">
            <Row>
              <Cell className="p-4">Hola</Cell>
            </Row>
          </Sheet>
        </Workbook>,
      ),
    ).rejects.toThrow(/no padding or margin/);
  });

  it('lets a component throw all the way to the caller', async () => {
    // React's own habit is to log and commit a tree with a hole in it, which
    // is right for a screen and wrong for a file nobody opens until later.
    const Broken = () => {
      throw new Error('the rows could not be loaded');
    };

    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="H">
            <Broken />
          </Sheet>
        </Workbook>,
      ),
    ).rejects.toThrow(/the rows could not be loaded/);
  });
});

describe('a picture', () => {
  it('hangs off the cell it is written in', async () => {
    // Written inside the cell rather than declared with coordinates beside
    // the sheet. A logo at `{ row: 0, column: 0 }` is a second place to keep
    // in step with the rows, and inserting a header row above would leave it
    // behind — which is exactly what the anchor exists to prevent.
    const ir = await toWorkbook(
      <Workbook>
        <Sheet name="Hoja">
          <Row>
            <Cell>Concepto</Cell>
          </Row>
          <Row>
            <Cell />
            <Cell>
              <Image src="logo" width={120} />
            </Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(ir.sheets[0].pictures).toEqual([{ image: 'logo', row: 1, column: 1, width: 120 }]);
  });

  it('leaves the cell it hangs from empty', async () => {
    // The picture floats over the grid; the cell is only the anchor. Writing
    // something into it would put a value where the author put an image.
    const ir = await toWorkbook(
      <Workbook>
        <Sheet name="Hoja">
          <Row>
            <Cell>
              <Image src="logo" width={60} />
            </Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(ir.sheets[0].rows?.[0].cells?.[0].value).toEqual({ t: 'blank' });
  });

  it('takes an offset into the cell, in points', async () => {
    const ir = await toWorkbook(
      <Workbook>
        <Sheet name="Hoja">
          <Row>
            <Cell>
              <Image src="logo" width={60} offset={{ x: 6, y: 3 }} />
            </Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(ir.sheets[0].pictures?.[0]).toMatchObject({ dx: 6, dy: 3 });
  });

  it('has no height to give it, because the image has one', async () => {
    // The same rule as `<Image width>` on a page. Asking for both is the one
    // way to squash a logo, and it is always somebody copying numbers.
    const ir = await toWorkbook(
      <Workbook>
        <Sheet name="Hoja">
          <Row>
            <Cell>
              <Image src="logo" width={60} />
            </Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(ir.sheets[0].pictures?.[0]).not.toHaveProperty('height');
  });

  it('is refused outside a cell, where it would have nothing to hang from', async () => {
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="Hoja">
            <Image src="logo" width={60} />
          </Sheet>
        </Workbook>,
      ),
    ).rejects.toThrow(/<image>/);
  });
});

describe('placing a picture', () => {
  const placed = async (props: Record<string, unknown>) =>
    (
      await toWorkbook(
        <Workbook>
          <Sheet name="Hoja">
            <Row>
              <Cell>
                <Image src="logo" width={60} {...props} />
              </Cell>
            </Row>
          </Sheet>
        </Workbook>,
      )
    ).sheets[0].pictures?.[0];

  it('carries where it should sit, and leaves the engine to work out where that is', async () => {
    // The React side cannot centre anything: it has never seen the image, so
    // it does not know how tall the picture is. All it can do is say so.
    expect(await placed({ align: 'center', valign: 'center' })).toMatchObject({
      align: 'center',
      valign: 'center',
    });
  });

  it('says nothing when it goes where a picture goes anyway', async () => {
    // Absent is the corner. A field written out on every picture would make
    // every sheet already declared produce a different IR for no change.
    expect(await placed({})).not.toHaveProperty('align');
    expect(await placed({ align: 'start' })).not.toHaveProperty('align');
  });
});

describe('an autofilter', () => {
  const rowOf = async (props: Record<string, unknown>) =>
    (
      await toWorkbook(
        <Workbook>
          <Sheet name="Hoja">
            <Row {...props}>
              <Cell>Fecha</Cell>
              <Cell>Importe</Cell>
            </Row>
          </Sheet>
        </Workbook>,
      )
    ).sheets[0].rows?.[0];

  it('is marked on the row whose cells are the labels', async () => {
    // The range it becomes ends at the last row of the sheet, which a producer
    // streaming a million rows has not got yet. All the author can say is
    // which row the labels are on.
    expect(await rowOf({ filter: true })).toMatchObject({ filter: true });
  });

  it('says nothing when nobody asked for one', async () => {
    // Absent is how every sheet written so far says it, and a field written
    // out on every row would change all of them for no change.
    expect(await rowOf({})).not.toHaveProperty('filter');
    expect(await rowOf({ filter: false })).not.toHaveProperty('filter');
  });
});
