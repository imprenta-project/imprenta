import { describe, expect, it } from 'vitest';
import { resolve } from '../src/xlsx/tailwind.js';

describe('what a cell can honour', () => {
  it('reads the type scale in the points Excel measures in', () => {
    // The one place the two media agree exactly: Excel's font size is in
    // points, so `text-sm` is the same size on paper and on a sheet.
    expect(resolve('text-sm').font?.size).toBe(10.5);
    expect(resolve('text-2xl').font?.size).toBe(18);
  });

  it('takes a colour for the type and a colour for the cell', () => {
    expect(resolve('text-red-600').font?.color).toBe('#e7000b');
    expect(resolve('bg-slate-100').fill).toBe('#f1f5f9');
  });

  it('takes a colour of the caller own naming', () => {
    const brand = { colors: { brand: '#1b3a5c' } };
    expect(resolve('text-brand', brand).font?.color).toBe('#1b3a5c');
  });

  it('maps the faces Excel and the page engine both have', () => {
    expect(resolve('font-bold').font?.bold).toBe(true);
    expect(resolve('font-normal').font?.bold).toBe(false);
    expect(resolve('italic underline line-through').font).toEqual({
      italic: true,
      underline: true,
      strike: true,
    });
  });

  it('aligns in both directions', () => {
    expect(resolve('text-right').align?.horizontal).toBe('right');
    expect(resolve('text-center').align?.vertical).toBeUndefined();
    expect(resolve('align-middle').align?.vertical).toBe('middle');
  });

  it('tells a text colour from a text alignment, spelled the same', () => {
    // `text-right` and `text-red-600` differ only in what follows, which is
    // the same ambiguity Tailwind has and the same way out of it.
    expect(resolve('text-left').align?.horizontal).toBe('left');
    expect(resolve('text-left').font?.color).toBeUndefined();
  });

  it('wraps text when asked', () => {
    expect(resolve('whitespace-normal').align?.wrap).toBe(true);
    expect(resolve('whitespace-nowrap').align?.wrap).toBe(false);
  });

  it('indents in the steps Excel counts in', () => {
    expect(resolve('indent-2').align?.indent).toBe(2);
  });
});

describe('borders, which Excel has three of', () => {
  it('draws all four sides for a bare border', () => {
    expect(resolve('border').border).toEqual({
      top: { style: 'thin' },
      right: { style: 'thin' },
      bottom: { style: 'thin' },
      left: { style: 'thin' },
    });
  });

  it('maps the three widths that land exactly', () => {
    expect(resolve('border-2').border?.top?.style).toBe('medium');
    expect(resolve('border-4').border?.top?.style).toBe('thick');
  });

  it('says so for a width Excel does not have', () => {
    // Rounding border-8 down to thick is the shrug this project refuses: it
    // is off by half and nothing would say which class did it.
    expect(() => resolve('border-8')).toThrow(/three — border \(thin\)/);
  });

  it('draws one side when one is named', () => {
    expect(resolve('border-b').border).toEqual({ bottom: { style: 'thin' } });
    expect(resolve('border-t-2').border).toEqual({ top: { style: 'medium' } });
  });

  it('takes the line styles Excel happens to share', () => {
    expect(resolve('border-dashed').border?.top?.style).toBe('dashed');
    expect(resolve('border-b border-double').border).toEqual({ bottom: { style: 'double' } });
  });

  it('colours a border whichever order the classes came in', () => {
    // Tailwind spells a side, a width and a colour the same way, and an author
    // writes them in whatever order reads well. Neither can be settled until
    // the list ends.
    const after = resolve('border-b border-slate-300');
    const before = resolve('border-slate-300 border-b');
    expect(after).toEqual(before);
    expect(after.border).toEqual({ bottom: { style: 'thin', color: '#cad5e2' } });
  });

  it('takes a border away for border-0', () => {
    expect(resolve('border border-0').border).toBeUndefined();
  });
});

describe('what a cell cannot do, said by name', () => {
  it('sends padding to the one thing Excel has', () => {
    expect(() => resolve('p-4')).toThrow(/no padding or margin.*indent-1/s);
    expect(() => resolve('px-2')).toThrow(/no padding or margin/);
    expect(() => resolve('mb-4')).toThrow(/no padding or margin/);
  });

  it('sends a width to the column and a height to the row', () => {
    expect(() => resolve('w-32')).toThrow(/<Column width>/);
    expect(() => resolve('h-10')).toThrow(/<Row height>/);
    expect(() => resolve('leading-6')).toThrow(/<Row height>/);
  });

  it('refuses what a cell simply does not have', () => {
    expect(() => resolve('rounded')).toThrow(/no corners/);
    expect(() => resolve('shadow-lg')).toThrow(/no such thing/);
    expect(() => resolve('tracking-wide')).toThrow(/no letter spacing/);
  });

  it('refuses layout, because a sheet is already a grid', () => {
    expect(() => resolve('flex')).toThrow(/already/);
    expect(() => resolve('gap-4')).toThrow(/already/);
    expect(() => resolve('absolute')).toThrow(/where its row and column are/);
  });

  it('refuses a variant, since a closed workbook is in no state', () => {
    expect(() => resolve('hover:bg-slate-100')).toThrow(/never in a state/);
  });

  it('names a class it has never heard of rather than dropping it', () => {
    expect(() => resolve('prose')).toThrow(/not a utility a spreadsheet has/);
  });

  it('names a colour it does not have', () => {
    expect(() => resolve('bg-burgundy')).toThrow(/does not have: burgundy/);
  });
});

describe('what the two resolvers disagree about', () => {
  it('honours on a sheet what the page refuses, and the other way round', async () => {
    // The capability tables are the point: neither is a subset of the other,
    // so a shared resolver with one table would have to shrug at half of it.
    const page = await import('../src/pdf/tailwind.js');

    expect(() => page.resolve('align-middle')).toThrow();
    expect(resolve('align-middle').align?.vertical).toBe('middle');

    expect(page.resolve('p-4').padding).toBeDefined();
    expect(() => resolve('p-4')).toThrow();
  });

  it('agrees about the things that are genuinely the same', async () => {
    const page = await import('../src/pdf/tailwind.js');

    expect(page.resolve('text-sm').size).toBe(resolve('text-sm').font?.size);
    expect(page.resolve('bg-slate-100').background).toBe(resolve('bg-slate-100').fill);
  });
});
