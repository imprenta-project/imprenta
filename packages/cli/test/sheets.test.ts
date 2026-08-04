import { describe, expect, it } from 'vitest';
import { checkWorkbook } from '../src/sheets.js';

const text = (v: string) => ({ value: { t: 'text', v } });
const number = (v: number) => ({ value: { t: 'number', v } });
const formula = (f: string) => ({ value: { t: 'formula', v: { formula: f } } });
const blank = () => ({ value: { t: 'blank' } });

const rules = (workbook: unknown) => checkWorkbook(workbook).map((f) => f.rule);
const detail = (workbook: unknown, rule: string) =>
  checkWorkbook(workbook).find((f) => f.rule === rule)?.detail ?? '';

/** A sheet of amounts with a header, which is the shape almost everything is. */
const amounts = (cells: { value: unknown }[][]) => ({
  sheets: [
    {
      name: 'Ventas',
      rows: [{ cells: [text('Concepto'), text('Importe')] }, ...cells.map((c) => ({ cells: c }))],
    },
  ],
});

describe('a number written as text', () => {
  it('is caught where a column of numbers has one text among them', () => {
    // The export that arrives with a total short by one row. On screen the
    // cell looks identical, and SUM skips it.
    const book = amounts([
      [text('Licencia'), number(1200)],
      [text('Soporte'), number(350)],
      [text('Formación'), text('900')],
    ]);

    expect(rules(book)).toContain('number-as-text');
    expect(detail(book, 'number-as-text')).toMatch(/“900”.*row 4.*column of 2 numbers/);
  });

  it('leaves a column that is text all the way down alone', () => {
    // Account codes, references, postcodes. Text on purpose, and flagging them
    // is how a panel teaches people to stop reading it.
    const book = {
      sheets: [
        {
          name: 'Cuentas',
          rows: [
            { cells: [text('430'), text('Clientes')] },
            { cells: [text('400'), text('Proveedores')] },
          ],
        },
      ],
    };

    expect(rules(book)).not.toContain('number-as-text');
  });

  it('leaves a leading zero alone even among numbers', () => {
    // 007 is a reference and not seven, whatever its neighbours are.
    const book = amounts([
      [text('a'), number(1)],
      [text('b'), text('007')],
    ]);

    expect(rules(book)).not.toContain('number-as-text');
  });

  it('counts the rest of the column rather than repeating itself', () => {
    const book = amounts([
      [text('a'), number(1)],
      [text('b'), text('20')],
      [text('c'), text('30')],
      [text('d'), text('40')],
    ]);

    expect(detail(book, 'number-as-text')).toMatch(/2 more in that column/);
  });
});

describe('formulas', () => {
  it('says which sheet a formula points at that is not there', () => {
    // Excel opens it and shows #REF!, which the author hears about from
    // whoever received the file.
    const book = {
      sheets: [
        { name: 'Resumen', rows: [{ cells: [formula("SUM('Libro major'!A1:A9)")] }] },
        { name: 'Libro mayor', rows: [{ cells: [number(1)] }] },
      ],
    };

    expect(rules(book)).toContain('formula-points-nowhere');
    expect(detail(book, 'formula-points-nowhere')).toMatch(/“Libro major”/);
  });

  it('is happy with a formula that stays on its own sheet', () => {
    const book = {
      sheets: [{ name: 'H', rows: [{ cells: [number(1)] }, { cells: [formula('SUM(A1:A1)')] }] }],
    };

    expect(rules(book)).not.toContain('formula-points-nowhere');
  });

  it('follows a reference to a sheet that does exist', () => {
    const book = {
      sheets: [
        { name: 'Resumen', rows: [{ cells: [formula("SUM('Libro mayor'!A1:A9)")] }] },
        { name: 'Libro mayor', rows: [{ cells: [number(1)] }] },
      ],
    };

    expect(rules(book)).not.toContain('formula-points-nowhere');
  });
});

describe('merges', () => {
  it('refuses two that overlap, because Excel refuses the file', () => {
    const book = {
      sheets: [
        {
          name: 'H',
          rows: [{ cells: [text('Total'), blank(), blank()] }],
          merges: [
            { fromRow: 0, fromColumn: 0, toRow: 0, toColumn: 1 },
            { fromRow: 0, fromColumn: 1, toRow: 0, toColumn: 2 },
          ],
        },
      ],
    };

    const overlap = checkWorkbook(book).find((f) => f.rule === 'merges-overlap');
    expect(overlap?.status).toBe('error');
  });

  it('warns when a merge covers something that will be thrown away', () => {
    // Excel keeps the top left and drops the rest without a word.
    const book = {
      sheets: [
        {
          name: 'H',
          rows: [{ cells: [text('Total'), number(1200)] }],
          merges: [{ fromRow: 0, fromColumn: 0, toRow: 0, toColumn: 1 }],
        },
      ],
    };

    expect(rules(book)).toContain('merge-hides-a-value');
  });

  it('is happy when a merge covers blanks, which is the normal case', () => {
    const book = {
      sheets: [
        {
          name: 'H',
          rows: [{ cells: [text('Total'), blank(), blank()] }],
          merges: [{ fromRow: 0, fromColumn: 0, toRow: 0, toColumn: 2 }],
        },
      ],
    };

    expect(rules(book)).toEqual([]);
  });
});

describe('what Excel will not take', () => {
  it('says a workbook needs a sheet', () => {
    expect(rules({ sheets: [] })).toContain('empty-workbook');
  });

  it('notices a sheet with nothing in it', () => {
    expect(rules({ sheets: [{ name: 'Vacía', rows: [] }] })).toContain('empty-sheet');
  });

  it('notices a sheet past the row Excel stops at', () => {
    const rows = Array.from({ length: 1_048_577 }, () => ({ cells: [] }));
    const book = { sheets: [{ name: 'Enorme', rows }] };

    const found = checkWorkbook(book).find((f) => f.rule === 'past-what-excel-holds');
    expect(found?.status).toBe('error');
    expect(found?.detail).toMatch(/1,048,576/);
  });
});

describe('what it shares with the document rules', () => {
  it('catches a contrast a reader will not thank anyone for', () => {
    // The same threshold as on paper, because the eye is the same eye.
    const book = {
      sheets: [
        {
          name: 'H',
          rows: [
            {
              cells: [{ ...text('apenas visible'), style: { font: { color: '#cad5e2' } } }],
            },
          ],
        },
      ],
    };

    expect(rules(book)).toContain('faint-text');
    expect(detail(book, 'faint-text')).toMatch(/1\.5 to 1/);
  });
});

describe('a clean workbook', () => {
  it('says nothing at all', () => {
    const book = amounts([
      [text('Licencia'), number(1200)],
      [text('Soporte'), number(350)],
    ]);

    expect(checkWorkbook(book)).toEqual([]);
  });

  it('does not run the document rules on it', () => {
    // A workbook has no `children`, so `empty-document` fires on every one of
    // them if the wrong list is used — which it was, once, and a panel that
    // says nonsense is worse than no panel.
    const book = amounts([[text('a'), number(1)]]);
    expect(rules(book)).not.toContain('empty-document');
  });
});

describe('a formula that will not open', () => {
  const sheet = (f: string) => ({ sheets: [{ name: 'H', rows: [{ cells: [formula(f)] }] }] });

  it('counts a bracket left open', () => {
    // What happens when a range is joined out of strings and one piece is
    // missing. Excel does not mark the cell — it calls the file damaged.
    expect(rules(sheet('SUM(A1:A9'))).toContain('broken-formula');
    expect(detail(sheet('SUM(A1:A9'), 'broken-formula')).toMatch(/1 bracket left open/);
  });

  it('counts a closing bracket with nothing to close', () => {
    expect(detail(sheet('SUM A1:A9)'), 'broken-formula')).toMatch(/nothing to close/);
  });

  it('notices a quotation mark left open', () => {
    expect(detail(sheet('IF(A1="sí,1,0)'), 'broken-formula')).toMatch(/quotation mark/);
  });

  it('notices an apostrophe left open, which is half a sheet name', () => {
    expect(detail(sheet("SUM('Libro mayor!A1:A9)"), 'broken-formula')).toMatch(/apostrophe/);
  });

  it('says so for a formula with nothing in it', () => {
    expect(detail(sheet('  '), 'broken-formula')).toMatch(/nothing in it/);
  });

  it('leaves brackets inside quotes alone', () => {
    // `"("` is a string and not an opening bracket. Counting naively would
    // flag half the CONCATENATE in the world.
    expect(rules(sheet('CONCATENATE(A1,"(",B1,")")'))).not.toContain('broken-formula');
  });

  it('is happy with the formulas people actually write', () => {
    for (const good of [
      'SUM(A1:A9)',
      "SUMIF('Libro mayor'!$C$4:$C$9,\"430\",'Libro mayor'!$F$4:$F$9)",
      'IF(A1<0,"debe","haber")',
      '=B3-C3',
      'ROUND(SUM(A1:A9)*1.21,2)',
    ]) {
      expect(rules(sheet(good)), good).not.toContain('broken-formula');
    }
  });
});
