import { type Node as HostNode, type Instance, isText } from '../host.js';
import type { Theme } from '../tailwind.js';
import { SIZES } from './elements.js';
import type { Edges, IrBand, IrDocument, IrNode, Run } from './ir.js';
import { type Resolved, resolve } from './tailwind.js';

/**
 * Turns the resolved host tree into the IR.
 *
 * Nothing here measures, styles or decides anything. It renames props into
 * what the engine's schema calls them, and it flattens a paragraph's children
 * into runs. That is the whole job: the vocabulary is the same on both sides
 * on purpose, so this stays a translation rather than a second layout engine.
 */
export function toIr(root: Instance, theme: Theme = {}): IrDocument {
  const bands = lift(root.children, theme);
  return {
    page: page(root.props, theme),
    ...(accumulators(root.props) ?? {}),
    ...bands.found,
    children: blocks(bands.rest, theme),
  };
}

/**
 * Takes the bands out of the children.
 *
 * They are written among them because that is where an author will put them,
 * and they are not part of the flow: a header left in the children would
 * print once, at the top of the first page.
 */
function lift(
  children: HostNode[],
  theme: Theme,
): { found: { header?: IrBand; footer?: IrBand }; rest: HostNode[] } {
  const found: { header?: IrBand; footer?: IrBand } = {};
  const rest: HostNode[] = [];

  for (const child of children) {
    if (isText(child) || (child.type !== 'header' && child.type !== 'footer')) {
      rest.push(child);
      continue;
    }
    const which = child.type as 'header' | 'footer';
    if (found[which]) {
      throw new Error(`a document has one ${which}, and this one declares two`);
    }
    found[which] = {
      height: child.props.height as number,
      children: blocks(child.children, theme),
    };
  }

  return { found, rest };
}

/**
 * The children of a container, with `<Theme>` folded away.
 *
 * A theme draws nothing; it says how the classes below it resolve. So it
 * never becomes a node — its children take its place, carrying the merged
 * theme down with them.
 */
function blocks(children: HostNode[], theme: Theme): IrNode[] {
  const out: IrNode[] = [];
  for (const child of children) {
    if (!isText(child) && child.type === 'theme') {
      out.push(...blocks(child.children, merge(theme, child.props)));
    } else {
      out.push(block(child, theme));
    }
  }
  return out;
}

/** An inner theme adds to the outer one rather than replacing it. */
function merge(outer: Theme, props: Record<string, unknown>): Theme {
  const inner = props as Theme;
  return {
    colors: { ...outer.colors, ...inner.colors },
    ptPerRem: inner.ptPerRem ?? outer.ptPerRem,
  };
}

/**
 * The style props a class list stands for.
 *
 * Resolved first so an explicit prop can override it: the prop is the more
 * specific of the two and the one TypeScript checked.
 */
function classes(props: Record<string, unknown>, theme: Theme): Resolved {
  return props.className ? resolve(props.className as string, theme) : {};
}

function page(props: Record<string, unknown>, theme: Theme): IrDocument['page'] {
  const named = SIZES[(props.size as keyof typeof SIZES) ?? 'A4'] ?? SIZES.A4;
  const [w, h] = props.landscape ? [named[1], named[0]] : named;
  // The page's margin is what padding on the document means; there is
  // nothing outside a page to put a margin in.
  const styled = classes(props, theme);
  return {
    width: (props.width as number) ?? w,
    height: (props.height as number) ?? h,
    margin: edges(props.margin) ?? styled.padding ?? { top: 34, right: 34, bottom: 34, left: 34 },
  };
}

function accumulators(props: Record<string, unknown>): { accumulators: string[] } | undefined {
  const declared = props.accumulators as string[] | undefined;
  return declared?.length ? { accumulators: declared } : undefined;
}

/** As {@link prune}, for a node, which always keeps at least its `t`. */
function irNode(value: Record<string, unknown> & { t: string }): IrNode {
  return prune(value) as IrNode;
}

function block(node: HostNode, theme: Theme): IrNode {
  if (isText(node)) {
    throw new Error(
      `loose text — "${node.text.trim()}" — must go inside a <Text>, since only a paragraph knows how to set it`,
    );
  }

  const { props, children } = node;
  switch (node.type) {
    case 'text':
      return irNode({
        t: 'text',
        runs: runs(children, inherited(props, theme), theme),
        style: textStyle(props, theme),
      });
    case 'box':
    case 'row':
      return irNode({
        t: node.type,
        style: boxStyle(props, theme),
        children: children.length ? blocks(children, theme) : undefined,
      });
    case 'image':
      return { t: 'image', src: props.src as string, width: props.width as number };
    case 'spacer':
      return { t: 'spacer', height: props.height as number };
    case 'pageBreak':
      return irNode({ t: 'pageBreak', to: props.to });
    case 'table':
      return irNode({
        t: 'table',
        columns: (props.columns as Record<string, unknown>[]).map(column),
        header: props.header,
        rows: props.rows,
        repeatHeader: props.repeatHeader,
        padding: edges(props.padding),
        spaceAfter: props.spaceAfter,
      });
    case 'list':
      return irNode({
        t: 'list',
        marker: props.marker,
        items: props.items,
        style: prune({
          size: props.size ?? classes(props, theme).size,
          color: props.color ?? classes(props, theme).color,
        }),
        gutter: props.gutter,
      });
    case 'canvas':
      return irNode({
        t: 'canvas',
        width: props.width,
        height: props.height,
        ops: props.ops,
        fill: props.fill,
        stroke: props.stroke,
        spaceAfter: props.spaceAfter,
      });
    case 'link': {
      if (children.length !== 1) {
        throw new Error(
          `a <Link> wraps exactly one child, and this one has ${children.length}; put a <Box> round them`,
        );
      }
      return { t: 'link', href: props.href as string, child: block(children[0], theme) };
    }
    default:
      throw new Error(`<${node.type}> is not something a document can contain`);
  }
}

/** The style in force at a point inside a paragraph. */
interface Inherited {
  weight?: 'bold';
  italic?: true;
  color?: string;
}

/**
 * Flattens a paragraph's children into styled stretches.
 *
 * A paragraph in the IR is a list of runs, never a string, so a bold word
 * keeps its face through shaping. The tree walked here is the one React
 * produced, which is why an author's component inside a paragraph has its
 * hooks and its context like any other.
 */
function runs(children: HostNode[], start: Inherited, theme: Theme): Run[] {
  const out: Run[] = [];
  for (const child of children) {
    inline(child, start, out, theme);
  }
  return out;
}

/**
 * What a paragraph's own classes mean for the text inside it.
 *
 * `font-bold` on a paragraph is not a paragraph property in the IR — weight
 * lives on runs — so it becomes the style the runs start from. A stretch that
 * says otherwise still wins, which is what inheritance means.
 */
function inherited(props: Record<string, unknown>, theme: Theme): Inherited {
  const styled = classes(props, theme);
  return {
    ...(styled.weight === 'bold' ? { weight: 'bold' as const } : {}),
    ...(styled.italic ? { italic: true as const } : {}),
  };
}

function inline(node: HostNode, style: Inherited, out: Run[], theme: Theme): void {
  if (isText(node)) {
    push(out, node.text, style);
    return;
  }
  const { children, props } = node;
  // The innermost of anything wins, which is what nesting means everywhere
  // else and what an author will assume here.
  const styled = classes(props, theme);
  const own: Inherited = {
    ...(styled.weight === 'bold' ? { weight: 'bold' as const } : {}),
    ...(styled.italic ? { italic: true as const } : {}),
    ...((props.color ?? styled.color) ? { color: (props.color ?? styled.color) as string } : {}),
  };

  switch (node.type) {
    case 'theme':
      each(children, style, out, merge(theme, props));
      return;
    case 'b':
      each(children, { ...style, ...own, weight: 'bold' }, out, theme);
      return;
    case 'i':
      each(children, { ...style, ...own, italic: true }, out, theme);
      return;
    case 'span':
      each(children, { ...style, ...own }, out, theme);
      return;
    // What a page knows about itself is written as a token and filled in as
    // the page is painted: glyphs cannot be substituted once they are shaped,
    // so the number has to arrive before the shaping rather than after.
    case 'pageNumber':
      push(out, '{{page}}', style);
      return;
    case 'pageCount':
      push(out, '{{pages}}', style);
      return;
    case 'runningTotal':
      push(out, `{{${(props.at as string) ?? 'closing'}:${props.name as string}}}`, style);
      return;
    default:
      throw new Error(`a paragraph cannot contain <${node.type}>`);
  }
}

function each(children: HostNode[], style: Inherited, out: Run[], theme: Theme): void {
  for (const child of children) {
    inline(child, style, out, theme);
  }
}

/**
 * Appends to the previous run when nothing about the style differs.
 *
 * Where JSX happened to split a string should not reach the engine: every run
 * is a shaping call, and a break between two is a line break parley could
 * otherwise have chosen better.
 */
function push(out: Run[], text: string, style: Inherited): void {
  if (text === '') {
    return;
  }
  const last = out[out.length - 1];
  if (
    last &&
    last.weight === style.weight &&
    last.italic === style.italic &&
    last.color === style.color
  ) {
    last.text += text;
    return;
  }
  out.push({ text, ...style });
}

function textStyle(
  props: Record<string, unknown>,
  theme: Theme,
): Record<string, unknown> | undefined {
  const styled = classes(props, theme);
  return prune({
    size: props.size ?? styled.size,
    color: props.color ?? styled.color,
    widows: props.widows,
    orphans: props.orphans,
    spaceAfter: props.spaceAfter ?? styled.spaceAfter,
  });
}

function boxStyle(
  props: Record<string, unknown>,
  theme: Theme,
): Record<string, unknown> | undefined {
  const styled = classes(props, theme);
  return prune({
    width: props.width ?? styled.width,
    padding: edges(props.padding) ?? styled.padding,
    background: props.background ?? styled.background,
    border: border(props, styled),
    radius: props.radius ?? styled.radius,
    spaceAfter: props.spaceAfter ?? styled.spaceAfter,
  });
}

/**
 * A border, as the engine holds it: one per side, each with its own width and
 * colour.
 *
 * Tailwind says it in three pieces — `border-2 border-slate-300 border-b` —
 * so they are collected here into the four sides the painter draws. A width
 * with no colour is black, because a box has no text colour to inherit.
 */
function border(
  props: Record<string, unknown>,
  styled: Resolved,
): Record<string, unknown> | undefined {
  const width = (props.borderWidth as number) ?? styled.borderWidth;
  const color = (props.border as string) ?? styled.border;
  if (width === undefined && color === undefined) {
    return undefined;
  }

  const sides = (props.borderSides as string[]) ??
    styled.borderSides ?? ['top', 'right', 'bottom', 'left'];
  const side = { width: width ?? 0.75, color: color ?? '#000000' };
  return Object.fromEntries(sides.map((name) => [name, side]));
}

function column(spec: Record<string, unknown>): Record<string, unknown> {
  return prune({ ...spec, width: length(spec.width) }) ?? {};
}

/**
 * Writes a width the way the schema reads lengths.
 *
 * The author writes `60`, `"50%"` or `"auto"`; the engine wants a tagged
 * value. Translating here rather than exposing the tagged form keeps the
 * shape an implementation detail of the two ends, not of the author.
 */
function length(value: unknown): Record<string, unknown> | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value === 'number') {
    return { unit: 'pt', value };
  }
  if (value === 'auto') {
    return { unit: 'auto' };
  }
  if (typeof value === 'string' && value.endsWith('%')) {
    return { unit: 'percent', value: Number.parseFloat(value) / 100 };
  }
  throw new Error(`${JSON.stringify(value)} is not a width: use a number, "50%" or "auto"`);
}

/** One number means all four sides; anything else is passed through. */
function edges(value: unknown): Edges | undefined {
  if (value === undefined) {
    return undefined;
  }
  if (typeof value === 'number') {
    return { top: value, right: value, bottom: value, left: value };
  }
  return value as Edges;
}

/**
 * Drops the keys the author never set.
 *
 * The IR gives every field a default, so an absent key and a key holding the
 * default mean the same thing to the engine — but not to a human reading the
 * JSON, or to a byte-for-byte comparison in a test.
 */
function prune<T extends Record<string, unknown>>(value: T): T | undefined {
  const kept = Object.entries(value).filter(([, v]) => v !== undefined);
  return kept.length ? (Object.fromEntries(kept) as T) : undefined;
}
