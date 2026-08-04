import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { write } from '../index.js';

/**
 * What a stricter reader than ours makes of what we wrote.
 *
 * calamine is forgiving, which is what makes it a good automated check and a
 * poor one: it opens files Excel would argue with. openpyxl is stricter, warns
 * about things calamine does not look at, and has already found a real defect
 * here — a workbook with no default cell style, which it substitutes for and
 * Excel silently guesses at, so the file looked right in one reader and wrong
 * in another.
 *
 * Warnings are treated as failures on purpose. Every one openpyxl has raised
 * about our output so far has been worth fixing.
 *
 * Skipped where Python and openpyxl are not installed rather than failing:
 * this is a second opinion, not the gate. `test/together.test.ts` and the Rust
 * suite are the gate.
 */
const python = (() => {
  try {
    execFileSync('python3', ['-c', 'import openpyxl'], { stdio: 'ignore' });
    return true;
  } catch {
    return false;
  }
})();

const text = (v: string) => ({ value: { t: 'text', v } });
const number = (v: number) => ({ value: { t: 'number', v } });

/**
 * Runs a snippet of Python against a workbook we wrote, with warnings fatal.
 *
 * `-W error::UserWarning` is the load-bearing part: openpyxl says what it
 * thinks of a file, and every one of those it has said about ours has been
 * worth acting on.
 */
async function ask(workbook: unknown, lines: string[]): Promise<string> {
  const { xlsx } = await write(JSON.stringify(workbook));
  const home = mkdtempSync(join(tmpdir(), 'imprenta-strict-'));
  const path = join(home, 'book.xlsx');
  writeFileSync(path, xlsx);
  try {
    return execFileSync(
      'python3',
      [
        '-W',
        'error::UserWarning',
        '-c',
        `import openpyxl,sys\npath=sys.argv[1]\n${lines.join('\n')}`,
        path,
      ],
      { encoding: 'utf8' },
    ).trim();
  } finally {
    rmSync(home, { recursive: true, force: true });
  }
}

const RICH = {
  sheets: [
    {
      name: 'Ventas',
      columns: [{ width: 30 }, { width: 16, style: { format: '#,##0.00 €' } }],
      rows: [
        {
          cells: [text('Concepto'), text('Importe')],
          height: 20,
          style: {
            font: { bold: true, color: '#1b3a5c', size: 12 },
            fill: '#f1f5f9',
            align: { horizontal: 'center', vertical: 'middle', wrap: true },
            border: { bottom: { style: 'medium', color: '#0f172a' } },
          },
        },
        { cells: [text('Licencia anual'), number(1200)] },
        {
          cells: [text('Descuento'), { ...number(-125.25), style: { font: { color: '#dc2626' } } }],
        },
        { cells: [{ value: { t: 'date', v: 46237 } }, { value: { t: 'bool', v: true } }] },
        {
          cells: [
            text('Total'),
            { value: { t: 'formula', v: { formula: 'SUM(B2:B3)', cached: 1074.75 } } },
          ],
        },
      ],
      merges: [{ fromRow: 0, fromColumn: 0, toRow: 0, toColumn: 1 }],
      freeze: { rows: 1 },
    },
    { name: 'Notas', rows: [{ cells: [text('Una hoja sencilla')] }] },
  ],
};

describe.skipIf(!python)('a stricter reader', () => {
  it('opens a workbook with everything on it, without one warning', async () => {
    // What is really being asserted is that nothing was muttered: a warning
    // exits non-zero and `ask` throws before it can return anything.
    const out = await ask(RICH, [
      'wb = openpyxl.load_workbook(path)',
      'print(",".join(wb.sheetnames))',
    ]);

    expect(out).toBe('Ventas,Notas');
  });

  it('reads back every type as the type it was written as', async () => {
    const out = await ask(RICH, [
      'ws = openpyxl.load_workbook(path)["Ventas"]',
      'kinds = [type(ws.cell(row=r, column=c).value).__name__',
      '         for r, c in ((3,2), (4,1), (4,2), (5,2))]',
      'print(",".join(kinds))',
    ]);

    // A number, a date, a boolean and a formula — the four a printed page
    // never has to tell apart.
    expect(out).toBe('float,datetime,bool,str');
  });

  it('keeps the formatting a cell was given', async () => {
    const out = await ask(RICH, [
      'ws = openpyxl.load_workbook(path)["Ventas"]',
      'h = ws.cell(row=1, column=1)',
      'd = ws.cell(row=3, column=2)',
      'print(h.font.b, h.font.color.rgb, h.fill.fgColor.rgb,',
      '      h.alignment.horizontal, h.alignment.wrapText, h.border.bottom.style,',
      '      d.font.color.rgb, ws.freeze_panes,',
      '      str(list(ws.merged_cells.ranges)[0]),',
      '      ws.column_dimensions["B"].width, sep="|")',
    ]);

    expect(out).toBe('True|FF1B3A5C|FFF1F5F9|center|True|medium|FFDC2626|A2|A1:B1|16.0');
  });

  it("a cell's own style replaces the column's whole, and does not add to it", async () => {
    // Surprising, deliberate, and Excel's own semantics: a format record is
    // complete, and a column style is the default for cells that brought
    // none. So a cell given nothing but a colour, in a column formatted as
    // currency, comes out General — and a producer that wanted both says both.
    //
    // Worth a test of its own because it is the sort of thing somebody hits
    // once and cannot find written down anywhere.
    const out = await ask(RICH, [
      'ws = openpyxl.load_workbook(path)["Ventas"]',
      'plain = ws.cell(row=2, column=2)',
      'coloured = ws.cell(row=3, column=2)',
      'print(plain.number_format, coloured.number_format, sep="|")',
    ]);

    // The one that said nothing inherits the column; the one that said
    // something inherits nothing.
    expect(out).toBe('#,##0.00 €|General');
  });

  it('agrees with us about how many formats the workbook needs', async () => {
    // If the interning ever regresses this is where it shows: a handful
    // becomes one per cell, and openpyxl counts them for us.
    const out = await ask(RICH, [
      'wb = openpyxl.load_workbook(path)',
      'print(len(wb._cell_styles))',
    ]);

    expect(Number(out)).toBeLessThan(12);
  });
});
