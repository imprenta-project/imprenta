import { describe, expect, it } from 'vitest';
import { Theme } from '../src/element.js';
import { Cell, Column, Image, Row, Sheet, Workbook } from '../src/xlsx/elements.js';
import { toWorkbook } from '../src/xlsx/render.js';

/**
 * Rows as a prop — plain data React never looks inside — against the same
 * content declared as children. The two must produce identical IR: the prop
 * exists because a fiber per cell costs 6 427 B of heap per row where the
 * data costs 865 (issue #11), and it would be no answer at all if it could
 * disagree with the children form about what a row means.
 */
describe('a sheet given rows as data', () => {
  it('produces the IR the same rows declared as children produce', async () => {
    const date = new Date(Date.UTC(2026, 0, 15));

    const declared = await toWorkbook(
      <Workbook>
        <Sheet name="Ventas">
          <Column width={12} />
          <Column width={40} format="#,##0.00" />
          <Row height={18} className="font-bold">
            <Cell value={date} format="dd/mm/yyyy" />
            <Cell className="text-right">Servicios prestados</Cell>
          </Row>
          <Row>
            <Cell value={1200.5} />
            <Cell formula="SUM(A1:A9)" cached={1200.5} />
          </Row>
          <Row>
            <Cell />
            <Cell value="007" />
          </Row>
        </Sheet>
      </Workbook>,
    );

    const asData = await toWorkbook(
      <Workbook>
        <Sheet
          name="Ventas"
          rows={[
            {
              height: 18,
              className: 'font-bold',
              cells: [
                { value: date, format: 'dd/mm/yyyy' },
                { value: 'Servicios prestados', className: 'text-right' },
              ],
            },
            { cells: [{ value: 1200.5 }, { formula: 'SUM(A1:A9)', cached: 1200.5 }] },
            { cells: [{}, { value: '007' }] },
          ]}
        >
          <Column width={12} />
          <Column width={40} format="#,##0.00" />
        </Sheet>
      </Workbook>,
    );

    expect(asData).toEqual(declared);
  });

  it('appends data rows after the rows declared as children', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet name="S" rows={[{ cells: [{ value: 2 }] }, { cells: [{ value: 3 }] }]}>
          <Row>
            <Cell value={1} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(book.sheets[0].rows?.map((r) => r.cells?.[0].value)).toEqual([
      { t: 'number', v: 1 },
      { t: 'number', v: 2 },
      { t: 'number', v: 3 },
    ]);
  });

  it('records a span as a merge at the row the data lands on, not at zero', async () => {
    const book = await toWorkbook(
      <Workbook>
        <Sheet
          name="S"
          rows={[
            { cells: [{ value: 'a' }, { value: 'b' }] },
            { cells: [{ value: 'total', colSpan: 2 }] },
          ]}
        >
          <Row>
            <Cell value="head" colSpan={2} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(book.sheets[0].merges).toEqual([
      { fromRow: 0, fromColumn: 0, toRow: 0, toColumn: 1 },
      { fromRow: 2, fromColumn: 0, toRow: 2, toColumn: 1 },
    ]);
    // The columns the span covers exist and are blank, as in the children form.
    expect(book.sheets[0].rows?.[2].cells).toHaveLength(2);
  });

  it('anchors a picture in a data cell where that cell is', async () => {
    const declared = await toWorkbook(
      <Workbook>
        <Sheet name="S">
          <Row>
            <Cell />
            <Cell>
              <Image src="logo" width={90} align="center" offset={{ x: 4 }} />
            </Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    const asData = await toWorkbook(
      <Workbook>
        <Sheet
          name="S"
          rows={[
            {
              cells: [{}, { image: { src: 'logo', width: 90, align: 'center', offset: { x: 4 } } }],
            },
          ]}
        />
      </Workbook>,
    );

    expect(asData.sheets[0].pictures).toEqual(declared.sheets[0].pictures);
    expect(asData.sheets[0].rows).toEqual(declared.sheets[0].rows);
  });

  it('resolves classes against the theme in force, as children do', async () => {
    const declared = await toWorkbook(
      <Theme colors={{ marca: '#123456' }}>
        <Workbook>
          <Sheet name="S">
            <Row>
              <Cell className="bg-marca" value={1} />
            </Row>
          </Sheet>
        </Workbook>
      </Theme>,
    );

    const asData = await toWorkbook(
      <Theme colors={{ marca: '#123456' }}>
        <Workbook>
          <Sheet name="S" rows={[{ cells: [{ value: 1, className: 'bg-marca' }] }]} />
        </Workbook>
      </Theme>,
    );

    expect(asData).toEqual(declared);
  });

  it('refuses a cell given both a value and a formula, naming the row', async () => {
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="S" rows={[{ cells: [{ value: 1, formula: 'SUM(A:A)' }] }]} />
        </Workbook>,
      ),
    ).rejects.toThrow(/value or a formula/);
  });

  it('refuses a number a spreadsheet cannot hold', async () => {
    await expect(
      toWorkbook(
        <Workbook>
          <Sheet name="S" rows={[{ cells: [{ value: Number.NaN }] }]} />
        </Workbook>,
      ),
    ).rejects.toThrow(/cannot hold/);
  });
});
