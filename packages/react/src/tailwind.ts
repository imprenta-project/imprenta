import { COLORS, SPACING_REM, TEXT } from './theme.js';

/**
 * The parts of resolving a Tailwind class that no output format owns.
 *
 * Looking up a colour, reading `text-sm` off the type scale, turning `[12pt]`
 * into a number, refusing `hover:` — none of that is about a page or a cell.
 * What each format owns is its **capability table**: which utilities mean
 * something to it, and what they become.
 *
 * That split is the whole design. `bg-slate-100` is a fill on paper and a fill
 * in a spreadsheet; `p-4` is padding on paper and has no counterpart in a cell
 * at all. One resolver with two tables says so by name in both directions,
 * where one resolver with a union of everything would have to shrug at half of
 * it — and a class quietly dropped is how a document comes out unstyled with
 * nobody able to say which line did it.
 */

/**
 * Points to a rem: sixteen CSS pixels, at three quarters of a point each.
 *
 * Keeping the web's arithmetic means `text-sm` is the size a designer expects
 * `text-sm` to be, and a scale carried over from a screen lands where it looks
 * like it should. Excel measures type in points too, so the same number serves
 * both formats.
 */
export const PT_PER_REM = 12;

export interface Theme {
  /** Colours of the caller's own. These shadow Tailwind's. */
  colors?: Record<string, string>;
  /** How many points a rem is worth. */
  ptPerRem?: number;
}

/** A class name taken apart: `text-[12pt]` and `text-sm` differ in shape. */
export interface Parsed {
  /** `text`, `border`, `bg` — everything before the value. */
  utility: string;
  /** What followed it, for a class from the scale. */
  suffix?: string;
  /** What was in the brackets, for an arbitrary value. */
  written?: string;
}

export function parse(name: string): Parsed {
  const arbitrary = name.match(/^([a-z]+(?:-[a-z]+)*)-\[(.+)\]$/);
  if (arbitrary) {
    return { utility: arbitrary[1], written: arbitrary[2] };
  }
  const utility = name.split('-')[0];
  return { utility, suffix: name.slice(utility.length + 1) };
}

/**
 * Refuses a variant, whatever the format.
 *
 * `hover:` needs a state, and neither a printed page nor a cell in a closed
 * workbook is ever in one.
 */
export function refuseVariant(name: string, medium: string): void {
  if (name.includes(':')) {
    const [prefix] = name.split(':');
    throw new Error(`"${name}" applies on ${prefix}, and ${medium} is never in a state`);
  }
}

export function points(rem: number, theme: Theme): number {
  return round(rem * (theme.ptPerRem ?? PT_PER_REM));
}

/** A size off Tailwind's type scale, in points. */
export function textSize(suffix: string, theme: Theme): number | undefined {
  const rem = TEXT[suffix];
  return rem === undefined ? undefined : points(rem, theme);
}

export function scale(table: Record<string, number>, key: string, name: string): number {
  const found = table[key];
  if (found === undefined) {
    throw new Error(`"${name}" is not a size this engine has`);
  }
  return found;
}

export function spacing(suffix: string | undefined, name: string): number {
  const steps = Number(suffix);
  if (!suffix || !Number.isFinite(steps)) {
    const utility = name.split('-')[0];
    throw new Error(
      `"${name}" is not a size this engine has: use a number of steps, or write it out as ${utility}-[12pt]`,
    );
  }
  return steps * SPACING_REM;
}

/** A length written out: points, pixels, rems, millimetres, or bare points. */
export function length(value: string, name: string, theme: Theme): number {
  const match = value.match(/^(-?[\d.]+)(pt|px|rem|mm)?$/);
  if (!match) {
    throw new Error(`"${name}" does not hold a length`);
  }
  const amount = Number(match[1]);
  switch (match[2]) {
    case 'px':
      return round(amount * 0.75);
    case 'rem':
      return round(amount * (theme.ptPerRem ?? PT_PER_REM));
    case 'mm':
      return round(amount * 2.8346457);
    default:
      return round(amount);
  }
}

export function color(key: string, name: string, theme: Theme): string {
  const found = theme.colors?.[key] ?? COLORS[key];
  if (!found) {
    throw new Error(`"${name}" names a colour this engine does not have: ${key}`);
  }
  return found;
}

/** Points to four places, so a quarter of a rem is 3 and not 2.9999999999996. */
export function round(value: number): number {
  return Math.round(value * 10_000) / 10_000;
}
