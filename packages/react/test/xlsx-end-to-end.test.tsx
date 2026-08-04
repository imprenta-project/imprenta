import { write } from '@imprentajs/xlsx';
import { describe, expect, it } from 'vitest';
import { Cell, Column, Row, Sheet, Workbook } from '../src/xlsx/elements.js';
import { render } from '../src/xlsx/render.js';

/**
 * React all the way to a file, and back out through a reader.
 *
 * The only test that can catch the two sides of the IR disagreeing. Everything
 * else here checks that the React layer produced the shape it meant to; serde
 * drops a field it does not recognise without a word, so a prop this side
 * invented arrives as nothing and every unit test still passes. That is not
 * hypothetical — it happened once already, on the PDF side, with a border.
 *
 * It needs a freshly built `@imprentajs/xlsx`. A stale addon makes this pass by
 * comparing two equally wrong workbooks.
 */

/**
 * Writes what React produced, and hands back the bytes.
 *
 * What the assertions then read is the XML inside the package rather than a
 * parsed model of it, because the question here is precisely whether a field
 * survived the crossing — and a reader that normalises what it finds would
 * paper over exactly the failure being looked for.
 */
async function through(element: React.ReactElement): Promise<{ bytes: Buffer }> {
  const { xlsx } = await write(await render(element));
  return { bytes: xlsx };
}

describe('React to a spreadsheet', () => {
  it('produces a workbook the writer accepts whole', async () => {
    const { bytes } = await through(
      <Workbook>
        <Sheet name="Ventas" freeze={{ rows: 1 }}>
          <Column width={12} />
          <Column width={30} />
          <Column width={16} format="#,##0.00 €" />
          <Row className="bg-slate-100 font-bold" height={20}>
            <Cell>Ref.</Cell>
            <Cell>Concepto</Cell>
            <Cell className="text-right">Importe</Cell>
          </Row>
          <Row>
            <Cell>001</Cell>
            <Cell>Licencia anual</Cell>
            <Cell value={1200} />
          </Row>
          <Row className="border-t font-bold">
            <Cell colSpan={2}>Total</Cell>
            <Cell formula="SUM(C2:C2)" cached={1200} />
          </Row>
        </Sheet>
      </Workbook>,
    );

    expect(bytes.subarray(0, 2).toString()).toBe('PK');
    expect(bytes.length).toBeGreaterThan(0);
  });

  it('sends every style field across in a shape the writer knows', async () => {
    // The point of the test. A field this side spells differently is dropped
    // in silence, so the check is that what came back has it — read out of the
    // XML the writer produced rather than out of what React said.
    const { bytes } = await through(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell className="text-red-600 bg-slate-100 font-bold italic underline align-middle text-right whitespace-normal indent-2 border-b-2 border-slate-300">
              Todo
            </Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    const styles = await partOf(bytes, 'xl/styles.xml');

    expect(styles).toContain('<b/>');
    expect(styles).toContain('<i/>');
    expect(styles).toContain('<u/>');
    expect(styles).toContain('FFE7000B'); // text-red-600, alpha first
    expect(styles).toContain('FFF1F5F9'); // bg-slate-100
    expect(styles).toContain('horizontal="right"');
    expect(styles).toContain('vertical="center"'); // Excel spells middle "center"
    expect(styles).toContain('wrapText="1"');
    expect(styles).toContain('indent="2"');
    expect(styles).toContain('<bottom style="medium">');
  });

  it('writes a date as a date and a number as a number', async () => {
    const { bytes } = await through(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell value={new Date(Date.UTC(2026, 7, 3))} />
            <Cell value={1200} />
            <Cell>1200</Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    const sheet = await partOf(bytes, 'xl/worksheets/sheet1.xml');

    // The date is the serial, and it carries a format that makes it a date.
    expect(sheet).toContain('<v>46237</v>');
    // The number has no type attribute, which is what "number" is spelled as.
    expect(sheet).toContain('<c r="B1"><v>1200</v></c>');
    // The text does, and keeps its characters.
    expect(sheet).toContain('t="inlineStr"');

    const styles = await partOf(bytes, 'xl/styles.xml');
    expect(styles).toContain('yyyy-mm-dd');
  });

  it('puts a merge where the colSpan was', async () => {
    const { bytes } = await through(
      <Workbook>
        <Sheet name="H">
          <Row>
            <Cell colSpan={3}>Total</Cell>
          </Row>
        </Sheet>
      </Workbook>,
    );

    const sheet = await partOf(bytes, 'xl/worksheets/sheet1.xml');
    expect(sheet).toContain('<mergeCell ref="A1:C1"/>');
  });

  it('refuses before writing anything when a class cannot apply', async () => {
    // The failure has to happen in React, not as a malformed file. An author
    // gets the class name and the line; a reader would get a repair dialog.
    await expect(
      render(
        <Workbook>
          <Sheet name="H">
            <Row>
              <Cell className="rounded-lg">Hola</Cell>
            </Row>
          </Sheet>
        </Workbook>,
      ),
    ).rejects.toThrow(/no corners/);
  });
});

/** One part of the package, as text. */
async function partOf(bytes: Buffer, name: string): Promise<string> {
  const { execFileSync } = await import('node:child_process');
  const { writeFileSync, rmSync } = await import('node:fs');
  const { join } = await import('node:path');
  const { tmpdir } = await import('node:os');

  const path = join(tmpdir(), `imprenta-e2e-${process.pid}-${Math.random()}.xlsx`);
  writeFileSync(path, bytes);
  try {
    return execFileSync('unzip', ['-p', path, name], { encoding: 'utf8', maxBuffer: 1 << 28 });
  } finally {
    rmSync(path, { force: true });
  }
}
