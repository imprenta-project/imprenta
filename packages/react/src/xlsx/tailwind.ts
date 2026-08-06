import { color, length, parse, refuseVariant, type Theme, textSize } from '../tailwind.js';
import type { CellStyle, Line, Side } from './ir.js';

/**
 * Turns a Tailwind class list into cell formatting.
 *
 * More of Tailwind maps here than one would guess, because Excel's own format
 * record is font, fill, border, alignment and number format — which is most of
 * what a class list says about a run of text. Excel even measures type in
 * points, so the type scale carries over exactly.
 *
 * What does not map is anything about **space and shape**: a cell has no
 * padding, no corners and no width of its own. Those are refused by name, and
 * the message says where the thing they were reaching for actually lives —
 * `indent-*` for the first, `<Column width>` and `<Row height>` for the rest.
 * Refusing beats a shrug for the same reason it does on paper: a spreadsheet
 * that came out unstyled with no explanation is worse than one that would not
 * build.
 */
export function resolve(classes: string, theme: Theme = {}): CellStyle {
  const work: Work = { style: {}, sides: new Set() };
  for (const name of classes.split(/\s+/).filter(Boolean)) {
    apply(name, theme, work);
  }
  return settle(work);
}

/**
 * A class list part way through.
 *
 * The border is accumulated rather than written as it goes, because Tailwind
 * spells four separate things the same way and they arrive in any order:
 * `border-b border-slate-300` and `border-slate-300 border-b` have to mean the
 * same thing, and neither can be resolved until the list ends.
 */
interface Work {
  style: CellStyle;
  sides: Set<Side>;
  line?: Line;
  borderColor?: string;
  /** Whether anything asked for a border at all. */
  bordered?: boolean;
}

const SIDES: Record<string, Side> = { t: 'top', r: 'right', b: 'bottom', l: 'left' };
const EVERY: Side[] = ['top', 'right', 'bottom', 'left'];

const ACROSS = ['left', 'center', 'right', 'justify'] as const;
const DOWN = { top: 'top', middle: 'middle', bottom: 'bottom' } as const;

/**
 * Excel's border widths, by the Tailwind class that reaches them.
 *
 * Three widths and no others. This is the one place a stylesheet and a
 * spreadsheet genuinely cannot agree — everywhere else `border-2` is two
 * pixels — so the classes that land exactly are honoured and the rest say so
 * rather than rounding into the nearest thing.
 */
const WIDTHS: Record<string, Line | 'none'> = {
  '': 'thin',
  '0': 'none',
  '2': 'medium',
  '4': 'thick',
};

const STYLES = ['dashed', 'dotted', 'double'] as const;

/**
 * What a cell cannot do, and where the author should look instead.
 *
 * Spelled out rather than left to fall through to "not a utility", because
 * every one of these is a reasonable thing to have tried and the useful half
 * of the answer is the redirection.
 */
const ELSEWHERE: [RegExp, string][] = [
  [/^-?[pm][xytrbl]?-/, 'a cell has no padding or margin — the nearest thing is `indent-1`'],
  [/^(w|min-w|max-w)-/, "a column's width is set on <Column width>, not on what is in it"],
  [/^(h|min-h|max-h)-/, "a row's height is set on <Row height>, not on what is in it"],
  [/^leading-/, 'a cell has no line height — its row has a height, as <Row height>'],
  [/^tracking-/, 'Excel has no letter spacing'],
  [/^rounded/, 'a cell has no corners to round'],
  [/^(shadow|opacity|blur|ring)/, 'a cell has no such thing'],
  [/^(flex|grid|gap-|justify-|items-|order-|col-|row-)/, 'a sheet is a grid already'],
  [/^(absolute|relative|fixed|sticky|top-|left-|z-)/, 'a cell is where its row and column are'],
];

function apply(name: string, theme: Theme, work: Work): void {
  refuseVariant(name, 'a cell in a workbook');

  for (const [pattern, because] of ELSEWHERE) {
    if (pattern.test(name)) {
      throw new Error(`"${name}" cannot apply to a spreadsheet: ${because}`);
    }
  }

  const { utility, suffix, written } = parse(name);
  const out = work.style;
  const font = () => {
    out.font ??= {};
    return out.font;
  };
  const align = () => {
    out.align ??= {};
    return out.align;
  };

  switch (true) {
    case name === 'italic':
      font().italic = true;
      return;
    case name === 'not-italic':
      font().italic = false;
      return;
    case name === 'underline':
      font().underline = true;
      return;
    case name === 'no-underline':
      font().underline = false;
      return;
    case name === 'line-through':
      font().strike = true;
      return;

    case name === 'whitespace-normal':
      align().wrap = true;
      return;
    case name === 'whitespace-nowrap':
      align().wrap = false;
      return;

    case utility === 'font':
      // Excel has real weights, but the rest of Imprenta has two faces, and a
      // heading that reads the same in both formats is worth more than the
      // other seven.
      font().bold = ['bold', 'semibold', 'extrabold', 'black'].includes(suffix ?? '');
      return;

    case utility === 'indent': {
      // Excel counts indents in units of about three characters, so putting
      // them on the spacing scale would be a lie. The number is the number.
      const steps = Number(suffix);
      if (!Number.isInteger(steps) || steps < 0) {
        throw new Error(
          `"${name}" is not an indent: Excel counts them in whole steps, as indent-2`,
        );
      }
      align().indent = steps;
      return;
    }

    case utility === 'text': {
      if (written !== undefined) {
        if (written.startsWith('#')) {
          font().color = written;
        } else {
          font().size = length(written, name, theme);
        }
        return;
      }
      const across = ACROSS.find((a) => a === suffix);
      if (across) {
        align().horizontal = across;
        return;
      }
      const size = textSize(suffix as string, theme);
      if (size !== undefined) {
        font().size = size;
      } else {
        font().color = color(suffix as string, name, theme);
      }
      return;
    }

    case utility === 'align': {
      const down = DOWN[suffix as keyof typeof DOWN];
      if (!down) {
        throw new Error(
          `"${name}" is not a vertical alignment: a cell has align-top, align-middle and align-bottom`,
        );
      }
      align().vertical = down;
      return;
    }

    case utility === 'bg':
      out.fill = written?.startsWith('#') === true ? written : color(suffix ?? '', name, theme);
      return;

    // `border-b-[#11307D]` parses as the utility `border-b`, because a written
    // value swallows everything after the last dash before the bracket. That
    // is the way anybody writes it having seen `border-b-2`, so the side is
    // taken off here rather than left to fall through to "not a utility".
    case utility === 'border' || utility.startsWith('border-'):
      border(name, utility.slice('border-'.length) || (suffix ?? ''), written, theme, work);
      return;
  }

  throw new Error(`"${name}" is not a utility a spreadsheet has`);
}

/** A width, a colour, a side or a line style, told apart by the suffix. */
function border(
  name: string,
  key: string,
  written: string | undefined,
  theme: Theme,
  work: Work,
): void {
  work.bordered = true;

  const [head, ...rest] = key.split('-');
  const side = SIDES[head];

  // `border-[#11307D]`, and `border-b-[#11307D]` for one side. A brand colour
  // is a hex nobody has a name for, and the other two places a colour is taken
  // — `bg-[#…]`, `text-[#…]` — both accept one written out.
  //
  // A width written out is refused whatever it says, including `border-[2]`,
  // which does land on a width Excel has. Honouring the ones that happen to
  // land would make the class work or not depending on the number in it, and
  // the number is the thing the author was guessing at. There are three
  // widths and they have names.
  if (written !== undefined) {
    if (!written.startsWith('#')) {
      throw new Error(TOO_MANY_WIDTHS(name));
    }
    if (side && head !== '') {
      work.sides.add(side);
    }
    work.borderColor = written;
    return;
  }

  if (side && head !== '') {
    work.sides.add(side);
    const width = rest.join('-');
    if (width) {
      work.line = widthOf(width, name, work);
    }
    return;
  }

  const asStyle = STYLES.find((s) => s === key);
  if (asStyle) {
    work.line = asStyle;
    return;
  }

  if (key === '' || /^\d+$/.test(key)) {
    work.line = widthOf(key, name, work);
    return;
  }

  work.borderColor = color(key, name, theme);
}

/**
 * One message, so a width refused for landing nowhere and a width refused for
 * being written out both name the three that exist.
 */
const TOO_MANY_WIDTHS = (name: string) =>
  `"${name}" is a width Excel does not have: it has three — border (thin), border-2 (medium) and border-4 (thick)`;

function widthOf(width: string, name: string, work: Work): Line | undefined {
  const found = WIDTHS[width];
  if (found === undefined) {
    throw new Error(TOO_MANY_WIDTHS(name));
  }
  if (found === 'none') {
    // `border-0` takes the border away rather than drawing a nought-wide one.
    work.bordered = false;
    work.sides.clear();
    return undefined;
  }
  return found;
}

/** Assembles what the whole class list came to. */
function settle(work: Work): CellStyle {
  if (!work.bordered) {
    return work.style;
  }
  // A style or a colour with no side named applies all the way round, the way
  // a bare `border` does.
  const sides = work.sides.size ? [...work.sides] : EVERY;
  const edge = {
    style: work.line ?? 'thin',
    ...(work.borderColor ? { color: work.borderColor } : {}),
  };
  work.style.border = Object.fromEntries(sides.map((side) => [side, { ...edge }]));
  return work.style;
}
