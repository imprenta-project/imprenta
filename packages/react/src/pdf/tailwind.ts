import {
  color,
  length,
  parse,
  points,
  refuseVariant,
  scale,
  spacing,
  type Theme,
  textSize,
} from '../tailwind.js';
import { RADIUS } from '../theme.js';

/** What a class list resolves to, in the page engine's own vocabulary. */
export interface Resolved {
  size?: number;
  color?: string;
  weight?: 'regular' | 'bold';
  italic?: boolean;
  background?: string;
  padding?: { top?: number; right?: number; bottom?: number; left?: number };
  width?: number;
  border?: string;
  borderWidth?: number;
  borderSides?: ('top' | 'right' | 'bottom' | 'left')[];
  radius?: number;
  spaceAfter?: number;
  align?: 'start' | 'end' | 'center' | 'justify';
}

/**
 * Turns a Tailwind class list into style props for a page.
 *
 * No CSS is involved at any point — no parse, no cascade, no stylesheet. A
 * class is looked up and a number or a colour comes out, which is the same
 * reason the engine skips the browser: the work a stylesheet does is work this
 * document does not need done.
 *
 * A class this medium cannot honour is an error rather than a shrug. Dropping
 * half a class list quietly is how a document comes out unstyled with nobody
 * able to say which line did it.
 */
export function resolve(classes: string, theme: Theme = {}): Resolved {
  const out: Resolved = {};
  for (const name of classes.split(/\s+/).filter(Boolean)) {
    apply(name, theme, out);
  }
  return out;
}

const SIDES = { t: 'top', r: 'right', b: 'bottom', l: 'left' } as const;

/**
 * Tailwind spells alignment with the same utility as size and colour, so
 * `text-right` has to be recognised before `right` is looked up as either and
 * reported as neither.
 */
const ALIGNMENTS: Record<string, 'start' | 'end' | 'center' | 'justify' | undefined> = {
  left: 'start',
  right: 'end',
  center: 'center',
  start: 'start',
  end: 'end',
  justify: 'justify',
};

function apply(name: string, theme: Theme, out: Resolved): void {
  refuseVariant(name, 'a printed page');

  const pt = (rem: number) => points(rem, theme);
  const { utility, suffix, written } = parse(name);

  switch (true) {
    case name === 'italic':
      out.italic = true;
      return;
    case name === 'not-italic':
      out.italic = false;
      return;

    case utility === 'font': {
      // The engine has two faces, not nine. Anything a designer would call
      // heavy is bold and anything they would not is regular; a document that
      // needs medium as well as semibold needs a second font, not a second
      // name for the one it has.
      out.weight = ['bold', 'semibold', 'extrabold', 'black'].includes(suffix ?? '')
        ? 'bold'
        : 'regular';
      return;
    }

    case utility === 'text' && ALIGNMENTS[suffix ?? ''] !== undefined:
      out.align = ALIGNMENTS[suffix as string];
      return;

    case utility === 'text': {
      if (written !== undefined) {
        if (written.startsWith('#')) {
          out.color = written;
        } else {
          out.size = length(written, name, theme);
        }
        return;
      }
      const size = textSize(suffix as string, theme);
      if (size !== undefined) {
        out.size = size;
      } else {
        out.color = color(suffix as string, name, theme);
      }
      return;
    }

    case utility === 'bg':
      out.background =
        written?.startsWith('#') === true ? written : color(suffix ?? '', name, theme);
      return;

    case /^p[xytrbl]?$/.test(utility): {
      const amount =
        written !== undefined ? length(written, name, theme) : pt(spacing(suffix, name));
      out.padding = { ...out.padding, ...onSides(utility.slice(1), amount) };
      return;
    }

    case utility === 'mb':
      // The engine has no margins — a block is followed by space or it is
      // not — so this is the one that maps, and it maps exactly.
      out.spaceAfter =
        written !== undefined ? length(written, name, theme) : pt(spacing(suffix, name));
      return;

    case utility === 'w':
      out.width = written !== undefined ? length(written, name, theme) : pt(spacing(suffix, name));
      return;

    case utility === 'rounded':
      if (written !== undefined) {
        out.radius = length(written, name, theme);
      } else if (!suffix) {
        out.radius = pt(RADIUS.sm);
      } else if (suffix === 'none') {
        out.radius = 0;
      } else if (suffix === 'full') {
        // "As round as it goes". The engine brings a radius down to what the
        // box can hold, so anything past that says the same thing.
        out.radius = pt(RADIUS['4xl'] * 330);
      } else {
        out.radius = pt(scale(RADIUS, suffix, name));
      }
      return;

    case utility === 'border':
      border(name, suffix, written, theme, out);
      return;
  }

  throw new Error(`"${name}" is not a utility this engine has`);
}

/**
 * A border says three separate things and Tailwind spells them all the same.
 *
 * `border-2` is a width, `border-slate-300` is a colour and `border-b` is a
 * side, so which one it is comes out of the suffix rather than the utility.
 */
function border(
  name: string,
  suffix: string | undefined,
  written: string | undefined,
  theme: Theme,
  out: Resolved,
): void {
  const pt = (rem: number) => points(rem, theme);
  // Tailwind's hairline is one pixel, which it writes as 1px rather than as a
  // step on the spacing scale.
  const HAIRLINE = pt(1 / 16);

  if (written !== undefined) {
    if (written.startsWith('#')) {
      out.border = written;
    } else {
      out.borderWidth = length(written, name, theme);
    }
    return;
  }
  if (!suffix) {
    out.borderWidth ??= HAIRLINE;
    return;
  }

  const [head, ...rest] = suffix.split('-');
  const side = SIDES[head as keyof typeof SIDES];
  if (side) {
    out.borderSides = [...(out.borderSides ?? []), side];
    const width = rest.join('-');
    out.borderWidth = width ? pt(Number(width) / 16) : (out.borderWidth ?? HAIRLINE);
    return;
  }
  if (/^\d+$/.test(suffix)) {
    out.borderWidth = pt(Number(suffix) / 16);
    return;
  }
  out.border = color(suffix, name, theme);
}

/** The sides an axis letter stands for. Nothing means all four. */
function onSides(axis: string, amount: number): Record<string, number> {
  switch (axis) {
    case 'x':
      return { left: amount, right: amount };
    case 'y':
      return { top: amount, bottom: amount };
    case 't':
      return { top: amount };
    case 'r':
      return { right: amount };
    case 'b':
      return { bottom: amount };
    case 'l':
      return { left: amount };
    default:
      return { top: amount, right: amount, bottom: amount, left: amount };
  }
}
