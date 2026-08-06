import { describe, expect, it } from 'vitest';
import { resolve } from '../src/pdf/tailwind.js';
import { PT_PER_REM } from '../src/tailwind.js';

/**
 * Utilities resolve straight to the engine's style vocabulary. There is no
 * CSS anywhere in this, which is the point: no parse, no cascade, no
 * stylesheet — a class name is looked up and a style prop comes out.
 */
describe('resolve', () => {
  it('gives nothing back for nothing', () => {
    expect(resolve('')).toEqual({});
    expect(resolve('   ')).toEqual({});
  });

  describe('text', () => {
    it('turns a named size into points', () => {
      // 0.875rem at 16px to the rem, and 0.75pt to the px.
      expect(resolve('text-sm')).toEqual({ size: 10.5 });
      expect(resolve('text-base')).toEqual({ size: 12 });
    });

    it('turns a named colour into the hex the engine reads', () => {
      expect(resolve('text-slate-700')).toEqual({ color: '#314158' });
      expect(resolve('text-white')).toEqual({ color: '#ffffff' });
    });

    it('takes a colour or a size written out in brackets', () => {
      expect(resolve('text-[#1b3a5c]')).toEqual({ color: '#1b3a5c' });
      expect(resolve('text-[9pt]')).toEqual({ size: 9 });
      expect(resolve('text-[12px]')).toEqual({ size: 9 });
    });

    it('reads a weight as bold or not', () => {
      // The engine has two faces, not nine. Anything a designer would call
      // heavy is bold; anything they would not is regular.
      expect(resolve('font-bold')).toEqual({ weight: 'bold' });
      expect(resolve('font-semibold')).toEqual({ weight: 'bold' });
      expect(resolve('font-black')).toEqual({ weight: 'bold' });
      expect(resolve('font-normal')).toEqual({ weight: 'regular' });
      expect(resolve('font-light')).toEqual({ weight: 'regular' });
    });

    it('reads italic both ways', () => {
      expect(resolve('italic')).toEqual({ italic: true });
      expect(resolve('not-italic')).toEqual({ italic: false });
    });
  });

  describe('boxes', () => {
    it('turns padding into points on every side', () => {
      expect(resolve('p-4')).toEqual({ padding: { top: 12, right: 12, bottom: 12, left: 12 } });
    });

    it('takes padding on one axis or one side', () => {
      expect(resolve('px-2')).toEqual({ padding: { left: 6, right: 6 } });
      expect(resolve('py-1')).toEqual({ padding: { top: 3, bottom: 3 } });
      expect(resolve('pt-8')).toEqual({ padding: { top: 24 } });
    });

    it('merges padding written in several classes, later winning', () => {
      expect(resolve('p-4 pt-0')).toEqual({ padding: { top: 0, right: 12, bottom: 12, left: 12 } });
    });

    it('takes a background', () => {
      expect(resolve('bg-slate-100')).toEqual({ background: '#f1f5f9' });
      expect(resolve('bg-[#e8eef4]')).toEqual({ background: '#e8eef4' });
    });

    it('takes a width', () => {
      expect(resolve('w-64')).toEqual({ width: 192 });
      expect(resolve('w-[200pt]')).toEqual({ width: 200 });
    });

    it('reads a bottom margin as the space after', () => {
      // The engine has no margins — a block is followed by space or it is
      // not — so this is the one that maps, and it maps exactly.
      expect(resolve('mb-6')).toEqual({ spaceAfter: 18 });
    });

    it('takes a border, its width, its colour and its sides', () => {
      expect(resolve('border')).toEqual({ borderWidth: 0.75 });
      expect(resolve('border-2')).toEqual({ borderWidth: 1.5 });
      expect(resolve('border-slate-300')).toEqual({ border: '#cad5e2' });
      expect(resolve('border-b')).toEqual({ borderWidth: 0.75, borderSides: ['bottom'] });
      expect(resolve('border-t-2')).toEqual({ borderWidth: 1.5, borderSides: ['top'] });
    });

    it('collects the sides a border is drawn on', () => {
      expect(resolve('border-t border-b')).toMatchObject({ borderSides: ['top', 'bottom'] });
    });

    it('colours one side with a colour written out', () => {
      // A written value swallows everything up to the bracket, so this parses
      // as the utility `border-b` and used to fall through to "not a utility
      // this engine has" — which is true of the string and no use to somebody
      // holding a brand colour. The cell side takes it; a page must too, or
      // the same class means two things depending on what it is printed onto.
      expect(resolve('border-b-[#11307D]')).toEqual({
        border: '#11307D',
        borderSides: ['bottom'],
      });
      expect(resolve('border-t-2 border-t-[#11307D]')).toEqual({
        borderWidth: 1.5,
        border: '#11307D',
        borderSides: ['top', 'top'],
      });
    });

    it('takes a corner radius', () => {
      expect(resolve('rounded')).toEqual({ radius: 3 });
      expect(resolve('rounded-lg')).toEqual({ radius: 6 });
      expect(resolve('rounded-none')).toEqual({ radius: 0 });
      expect(resolve('rounded-[2pt]')).toEqual({ radius: 2 });
    });

    it('takes a radius bigger than any box, for a pill', () => {
      // `rounded-full` means "as round as it goes". The engine brings it
      // down to what the box can hold rather than refusing it.
      expect(resolve('rounded-full')).toEqual({ radius: 7920 });
    });
  });

  describe('several at once', () => {
    it('reads a whole class list', () => {
      expect(resolve('bg-slate-100 p-3 rounded-lg text-sm text-slate-700 font-bold')).toEqual({
        background: '#f1f5f9',
        padding: { top: 9, right: 9, bottom: 9, left: 9 },
        radius: 6,
        size: 10.5,
        color: '#314158',
        weight: 'bold',
      });
    });

    it('lets the last of two conflicting classes win', () => {
      expect(resolve('text-sm text-lg')).toEqual({ size: 13.5 });
    });

    it('does not mind how the classes are spaced', () => {
      expect(resolve('  p-4\n  bg-white  ')).toEqual({
        padding: { top: 12, right: 12, bottom: 12, left: 12 },
        background: '#ffffff',
      });
    });
  });

  describe('what it will not do', () => {
    it('refuses a class it does not know, rather than ignoring it', () => {
      // Silently dropping half a class list is how a document comes out
      // unstyled and nobody can say why. Every other engine does it.
      expect(() => resolve('flex')).toThrow(/flex/);
      expect(() => resolve('p-4 shadow-lg')).toThrow(/shadow-lg/);
    });

    it('says so when a utility means nothing on paper', () => {
      const refused = () => resolve('hover:bg-slate-100');

      expect(refused).toThrow(/hover/);
    });

    it('refuses a width it cannot express', () => {
      // A box takes a width in points; the engine has no percentages for
      // one. Saying so beats laying the box out at some other width.
      expect(() => resolve('w-1/2')).toThrow(/w-1\/2/);
    });

    it('names the colour it does not have', () => {
      // Tailwind's palette is wide and grows; the message has to name what
      // was asked for, because guessing which of three hundred names is
      // missing is not a thing anyone should have to do.
      expect(() => resolve('text-burgundy-400')).toThrow(/burgundy/);
      expect(() => resolve('bg-slate-450')).toThrow(/slate-450/);
    });
  });

  describe('the theme', () => {
    it("takes colours of the caller's own", () => {
      const brand = { colors: { brand: '#1b3a5c', 'brand-soft': '#e8eef4' } };

      expect(resolve('text-brand bg-brand-soft', brand)).toEqual({
        color: '#1b3a5c',
        background: '#e8eef4',
      });
    });

    it("lets a caller's colour shadow one of Tailwind's", () => {
      expect(resolve('text-blue-500', { colors: { 'blue-500': '#000080' } })).toEqual({
        color: '#000080',
      });
    });

    it('takes a different idea of how big a rem is', () => {
      // A document set in a smaller measure wants the whole scale to shrink
      // with it, not every class rewritten.
      expect(resolve('text-base', { ptPerRem: 10 })).toEqual({ size: 10 });
      expect(resolve('p-4', { ptPerRem: 10 })).toEqual({
        padding: { top: 10, right: 10, bottom: 10, left: 10 },
      });
    });

    it('has a default a printer would recognise', () => {
      // 16 CSS pixels to the rem, 0.75 points to the pixel.
      expect(PT_PER_REM).toBe(12);
    });
  });
});
