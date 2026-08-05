/**
 * What is wrong with a document, said before anyone prints it.
 *
 * Every medium has its own ways of ruining a document, and paper's are
 * specific: type too small to read, ink inside the margin a printer cannot
 * reach, a colour that will not survive being printed grey. None of them shows
 * up on a screen, which is exactly why they have to be said out loud.
 *
 * Every rule here is about the finished sheet. None of them is a matter of
 * taste, and none guesses: each one names what it found and where.
 */
export interface Finding {
  rule: string;
  status: 'error' | 'warning';
  /** Whether the rule read the document, or the engine reported it. */
  source: 'document' | 'engine';
  detail: string;
  where?: string;
  /** How many places the same fault was found in. */
  occurrences: number;
}

/** A finding on its way through `collapse`, before the count is settled. */
export type Raw = Finding & { signature?: string };

/** Below this, print stops being legible for anyone. */
const SMALLEST_READABLE_PT = 6;

/** Most office printers refuse the outer five millimetres of a sheet. */
const UNPRINTABLE_PT = 14.17;

/** Contrast below which text stops being comfortable, on paper or on a screen. */
export const FAINT_RATIO = 3;

/** Below this a picture looks soft in print, however good the original was. */
const SOFT_DPI = 150;

/** And below this it is a screen image someone dropped into a document. */
const BAD_DPI = 100;

/**
 * What the project has, for the rules that cannot be answered by the document
 * alone.
 *
 * Optional throughout: a rule that does not know stays quiet rather than
 * accusing every document of missing every face.
 */
export interface Context {
  /** The faces the project configured. */
  faces?: { weight: 'regular' | 'bold'; italic: boolean }[];
  /** Pixel dimensions of the images it configured, by name. */
  images?: Record<string, { width: number; height: number }>;
}

interface Style {
  size?: number;
  color?: string;
  background?: string;
}

export function check(document: unknown, diagnostics: string[], context: Context = {}): Finding[] {
  const found: Raw[] = [];
  const doc = document as { page?: Record<string, unknown>; children?: unknown[] } | null;

  try {
    margins(doc?.page, found);
    const children = doc?.children ?? [];
    if (children.length === 0) {
      found.push({
        rule: 'empty-document',
        status: 'warning',
        source: 'document',
        detail:
          'this document has nothing in it, which is almost always a component that returned nothing',
        occurrences: 1,
      });
    }
    for (const child of children) {
      walk(child, { color: '#000000', background: '#ffffff' }, found, context, room(doc?.page));
    }
  } catch {
    // A rule must never take the preview down with it. The IR grows, and a
    // shape no rule expected is a gap in the rules, not a broken document.
  }

  for (const note of diagnostics) {
    found.push(fromEngine(note));
  }

  // Errors first: what stops the document being usable comes before what
  // merely spoils it.
  return collapse(found).sort((a, b) => rank(a) - rank(b));
}

/**
 * The same fault, said once.
 *
 * A pale paragraph of four runs is one problem, not four, and the engine
 * already aggregates its own diagnostics for exactly this reason. Two faults
 * are the same when the rule and the thing that is wrong match — the colours,
 * the size — not when the text does, since the text is the example rather
 * than the fault.
 */
export function collapse(found: Raw[]): Finding[] {
  const byFault = new Map<string, Raw>();
  for (const finding of found) {
    const key = `${finding.rule}\u0000${finding.signature ?? finding.detail}`;
    const seen = byFault.get(key);
    if (seen) {
      seen.occurrences += 1;
    } else {
      byFault.set(key, { ...finding, occurrences: 1 });
    }
  }
  return [...byFault.values()].map(({ signature: _signature, ...rest }) => rest);
}

export const rank = (f: Finding) => (f.status === 'error' ? 0 : 1);

function margins(page: Record<string, unknown> | undefined, found: Raw[]): void {
  const margin = (page?.margin ?? {}) as Record<string, number>;
  const tight = (['top', 'right', 'bottom', 'left'] as const).filter(
    (side) => typeof margin[side] === 'number' && margin[side] < UNPRINTABLE_PT,
  );
  if (tight.length === 0) {
    return;
  }
  found.push({
    rule: 'unprintable-margin',
    status: 'warning',
    source: 'document',
    occurrences: 1,
    detail: `the ${tight.join(', ')} margin is inside the five millimetres most printers cannot reach`,
    where: 'page',
  });
}

/** How wide content can be before the page cuts it off. */
function room(page: Record<string, unknown> | undefined): number {
  const width = typeof page?.width === 'number' ? page.width : 595.2756;
  const margin = (page?.margin ?? {}) as Record<string, number>;
  return width - (margin.left ?? 0) - (margin.right ?? 0);
}

function walk(
  node: unknown,
  inherited: Style,
  found: Raw[],
  context: Context,
  available: number,
): void {
  if (!node || typeof node !== 'object') {
    return;
  }
  const it = node as Record<string, unknown>;
  const style = (it.style ?? {}) as Style;

  switch (it.t) {
    case 'text': {
      const size = style.size ?? 12;
      const color = style.color ?? inherited.color;
      tiny(size, 'a paragraph', found);
      for (const run of (it.runs ?? []) as {
        text?: string;
        color?: string;
        weight?: string;
        italic?: boolean;
      }[]) {
        faint(run.color ?? color, inherited.background, quote(run.text), found);
        face(run.weight === 'bold', run.italic === true, quote(run.text), context, found);
      }
      return;
    }
    case 'box':
    case 'row': {
      const within = {
        color: inherited.color,
        background: style.background ?? inherited.background,
      };
      const width = (style as { width?: number }).width;
      if (typeof width === 'number' && width > available + 0.01) {
        found.push({
          rule: 'wider-than-the-page',
          status: 'error',
          source: 'document',
          detail: `a box is ${round(width)}pt wide where the page leaves ${round(available)}pt between its margins; the engine will lay it out and the page will cut it off`,
          occurrences: 1,
          signature: `${round(width)}/${round(available)}`,
        });
      }
      for (const child of (it.children ?? []) as unknown[]) {
        walk(child, within, found, context, Math.min(available, width ?? available));
      }
      return;
    }
    case 'image':
      resolution(it, context, found);
      return;
    case 'link': {
      if (!/^(https?:|mailto:|tel:)/.test(String(it.href ?? ''))) {
        found.push({
          rule: 'unopenable-link',
          status: 'warning',
          source: 'document',
          occurrences: 1,
          detail: `${JSON.stringify(it.href)} is not a link a reader can follow from a PDF: give it a scheme`,
          where: 'link',
        });
      }
      walk(it.child, inherited, found, context, available);
      return;
    }
    case 'list': {
      tiny((style as Style).size ?? 12, 'a list', found);
      return;
    }
    case 'table':
      table(it, inherited, found, context);
      return;
    default:
  }
}

function table(
  it: Record<string, unknown>,
  inherited: Style,
  found: Raw[],
  context: Context,
): void {
  const columns = ((it.columns ?? []) as unknown[]).length;
  // The repeated header is a list of rows: a grouped report puts the group on
  // one and the column labels on the next. Every one of them is a row like any
  // other and gets checked like one — a header short of a cell is exactly as
  // wrong as a body row short of one, and rather more visible.
  const header = (it.header ?? []) as { cells?: unknown[]; style?: Style }[];
  const rows = (it.rows ?? []) as { cells?: unknown[]; style?: Style }[];

  for (const row of [...header, ...rows]) {
    const cells = (row.cells ?? []) as {
      text?: string;
      size?: number;
      color?: string;
      weight?: string;
      italic?: boolean;
    }[];
    if (columns > 0 && cells.length !== columns) {
      found.push({
        rule: 'ragged-row',
        status: 'error',
        source: 'document',
        occurrences: 1,
        detail: `a row has ${cells.length} cells where the table declares ${columns} columns; the engine will drop the difference`,
        where: 'table',
      });
    }
    const behind = row.style?.background ?? inherited.background;
    for (const cell of cells) {
      tiny(cell.size ?? 9, 'a table cell', found);
      faint(cell.color ?? inherited.color, behind, quote(cell.text), found);
      face(cell.weight === 'bold', cell.italic === true, quote(cell.text), context, found);
    }
  }
}

/**
 * Warns when a document asks for a face the project never configured.
 *
 * The engine has no system fonts and falls back to what it was given, so a
 * heading meant to be bold simply is not — and nothing else in the chain
 * mentions it.
 */
function face(bold: boolean, italic: boolean, what: string, context: Context, found: Raw[]): void {
  if (!context.faces || (!bold && !italic)) {
    return;
  }
  const wanted = { weight: bold ? ('bold' as const) : ('regular' as const), italic };
  const has = context.faces.some(
    (face) => face.weight === wanted.weight && face.italic === wanted.italic,
  );
  if (has) {
    return;
  }
  const named = `${wanted.weight}${wanted.italic ? ' italic' : ''}`;
  found.push({
    rule: 'missing-face',
    status: 'warning',
    source: 'document',
    detail: `${what} asks for ${named}, which this project has no font for — the engine will set it in whatever it was given`,
    occurrences: 1,
    signature: named,
  });
}

/**
 * Warns when an image is printed larger than its pixels can carry.
 *
 * A screen image is 72 dpi and looks fine on a screen. On paper, below 150 it
 * is visibly soft, and nothing between here and the printer measures it.
 */
function resolution(it: Record<string, unknown>, context: Context, found: Raw[]): void {
  const source = context.images?.[String(it.src)];
  const width = it.width;
  if (!source || typeof width !== 'number' || width <= 0) {
    return;
  }
  const dpi = (source.width / width) * 72;
  if (dpi >= SOFT_DPI) {
    return;
  }
  found.push({
    rule: 'low-resolution-image',
    status: dpi < BAD_DPI ? 'error' : 'warning',
    source: 'document',
    detail: `${it.src} is ${source.width}×${source.height} printed ${round(width)}pt wide, which is ${Math.round(dpi)} dpi — under ${SOFT_DPI} it looks soft on paper`,
    occurrences: 1,
    signature: `${it.src}/${round(width)}`,
  });
}

const round = (n: number) => Math.round(n * 10) / 10;

function tiny(size: number, what: string, found: Raw[]): void {
  if (size >= SMALLEST_READABLE_PT) {
    return;
  }
  found.push({
    rule: 'tiny-text',
    status: 'error',
    source: 'document',
    detail: `${what} is set at ${size}pt, below the ${SMALLEST_READABLE_PT}pt where print stops being legible`,
    where: what,
    occurrences: 1,
    signature: `${size}`,
  });
}

function faint(
  color: string | undefined,
  background: string | undefined,
  what: string,
  found: Raw[],
): void {
  const ratio = contrast(color, background);
  if (ratio === null || ratio >= FAINT_RATIO) {
    return;
  }
  found.push({
    rule: 'faint-text',
    status: 'warning',
    source: 'document',
    detail: `${what} is ${color} on ${background}, a contrast of ${ratio.toFixed(1)} to 1 — under ${FAINT_RATIO} it disappears on paper`,
    where: what,
    occurrences: 1,
    signature: `${color}/${background}`,
  });
}

/** WCAG's ratio, which is as good a line for paper as for a screen. */
export function contrast(a: string | undefined, b: string | undefined): number | null {
  const one = luminance(a);
  const two = luminance(b);
  if (one === null || two === null) {
    return null;
  }
  const [light, dark] = one > two ? [one, two] : [two, one];
  return (light + 0.05) / (dark + 0.05);
}

export function luminance(hex: string | undefined): number | null {
  const parsed = rgb(hex);
  if (!parsed) {
    return null;
  }
  const [r, g, b] = parsed.map((channel) => {
    const c = channel / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function rgb(hex: string | undefined): [number, number, number] | null {
  if (typeof hex !== 'string') {
    return null;
  }
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(hex);
  if (short) {
    return [1, 2, 3].map((i) => Number.parseInt(short[i].repeat(2), 16)) as [
      number,
      number,
      number,
    ];
  }
  const long = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(hex);
  return long
    ? ([1, 2, 3].map((i) => Number.parseInt(long[i], 16)) as [number, number, number])
    : null;
}

export function quote(text: string | undefined): string {
  if (!text) {
    return 'text';
  }
  const short = text.length > 32 ? `${text.slice(0, 32)}…` : text;
  return `“${short}”`;
}

/**
 * An engine diagnostic, as a finding.
 *
 * They arrive as `warning[missing-glyph]: what happened — what to do`. The
 * engine has already decided how serious it is, so that is taken rather than
 * guessed. One that does not fit the shape is kept whole rather than dropped:
 * the engine said it for a reason.
 */
function fromEngine(note: string): Raw {
  const split = /^(warning|error)\[([a-z-]+)\]:\s*(.*)$/s.exec(note);
  if (!split) {
    return { rule: 'engine', status: 'warning', source: 'engine', detail: note, occurrences: 1 };
  }
  return {
    rule: split[2],
    status: split[1] === 'error' ? 'error' : 'warning',
    source: 'engine',
    detail: split[3],
    occurrences: 1,
  };
}
