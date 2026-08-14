import type { ReactNode } from 'react';
import { host } from '../element.js';
import type { Edges } from './ir.js';

/**
 * The vocabulary an author writes in.
 *
 * Each of these is a host element: a string the reconciler hands back
 * untouched, wearing a component's type so JSX checks the props. They are the
 * IR's node kinds and nothing more — nothing here decides how anything looks,
 * because that is the engine's job and the author's, not the library's.
 */

export type PageSize = 'A3' | 'A4' | 'A5' | 'Letter' | 'Legal' | 'Tabloid';

/** Tailwind classes, resolved against the theme in force. */
interface Styled {
  className?: string;
}

export interface BandProps {
  /**
   * Room taken out of every page for this band.
   *
   * Out of the content box rather than the margin, so a band can never
   * overlap the last line.
   */
  height: number;
  children?: ReactNode;
}

export interface RunningTotalProps {
  /** One of the document's `accumulators`. */
  name: string;
  /** Where in the page to read it. The close, unless said otherwise. */
  at?: 'opening' | 'closing';
}

export interface DocumentProps extends Styled {
  /**
   * Running totals the document keeps, in the order a band refers to them.
   *
   * This is what makes "suma y sigue" possible: a row contributes to one, and
   * a footer reads what it stood at when the page opened and closed.
   */
  accumulators?: string[];
  /** A named paper size. Ignored when `width` and `height` are given. */
  size?: PageSize;
  /** Turns the chosen size on its side. */
  landscape?: boolean;
  width?: number;
  height?: number;
  /** One number for all four sides, or only the sides that differ. */
  margin?: number | Edges;
  children?: ReactNode;
}

export interface TextProps extends Styled {
  size?: number;
  color?: string;
  /**
   * Which edge of its box the lines are set against.
   *
   * The same words a table column takes, so that an amount under a table
   * lines up with the amounts in it. `justify` is the odd one: nothing moves,
   * the spaces widen until every line but the last reaches the far edge.
   */
  align?: 'start' | 'end' | 'center' | 'justify';
  /** Minimum lines carried to the top of a page. */
  widows?: number;
  /** Minimum lines left at the foot of one. */
  orphans?: number;
  spaceAfter?: number;
  children?: ReactNode;
}

export interface BoxProps extends Styled {
  width?: number;
  padding?: number | Edges;
  background?: string;
  /** Colour of the border. Black unless said otherwise. */
  border?: string;
  borderWidth?: number;
  /** Which sides the border is drawn on. All four by default. */
  borderSides?: ('top' | 'right' | 'bottom' | 'left')[];
  /** Corner radius, brought down to what the box can hold. */
  radius?: number;
  spaceAfter?: number;
  children?: ReactNode;
}

export interface ImageProps {
  /** Name of an asset handed to the engine alongside the document. */
  src: string;
  width: number;
}

export interface LinkProps {
  href: string;
  children?: ReactNode;
}

export interface SpacerProps {
  height?: number;
  /**
   * Takes whatever is left of the page, so that whatever follows sits at its
   * foot — an invoice's payment terms, a signature line.
   *
   * The one thing an author cannot work out for themselves: only the packer
   * knows where the content stopped. `height` becomes the least the gap may
   * be. Meaningful in the flow only; inside a box there is no page to fill,
   * and the engine says so rather than quietly doing nothing.
   */
  grow?: boolean;
}

export interface PageBreakProps {
  /** `odd` opens the next section on a right-hand page. */
  to?: 'next' | 'odd' | 'even';
}

/**
 * A width the engine understands.
 *
 * A bare number is points, a string ending in `%` is a share of what is
 * available, and `auto` is what is left over. The author writes the short
 * form; `document.ts` writes the tagged one the schema wants.
 */
export type Width = number | `${number}%` | 'auto';

export interface ColumnProps {
  width?: Width;
  align?: 'start' | 'end' | 'center';
  /** What to do with a cell too wide for its column. Wraps by default. */
  overflow?: 'wrap' | 'ellipsis' | 'clip';
}

export interface CellProps {
  text: string;
  /**
   * How many columns the cell covers. One when absent.
   *
   * What it is for is a two-level header: a group name over the pair of
   * columns it names. The cell takes the x of the first column it covers and
   * the width of all of them, so it lines up with them exactly. A span past
   * the last column stops at the last column.
   *
   * The columns it covers belong to it: the next cell in the row starts after
   * them, so a spanned header row is written short rather than padded with
   * the blanks a spreadsheet would want. Its alignment and its overflow are
   * the first column's — a group name has no column of its own to ask.
   */
  colSpan?: number;
  size?: number;
  color?: string;
  weight?: 'regular' | 'bold';
  italic?: boolean;
}

export interface RowProps {
  cells: CellProps[];
  style?: Omit<BoxProps, 'children'>;
  /** What this row adds to the document's running totals. */
  totals?: { accumulator: number; value: number }[];
}

export interface TableProps {
  columns: ColumnProps[];
  /**
   * Comes back at the top of every continuation page unless turned off.
   *
   * Several rows when the table needs them: a grouped report wants to say
   * which group this is *and* what its columns mean, and a reader on the
   * fortieth page of a group should not have to choose which half of that
   * question gets answered. They are painted as one block.
   */
  header?: RowProps | RowProps[];
  rows: RowProps[];
  repeatHeader?: boolean;
  padding?: number | Edges;
  spaceAfter?: number;
}

export interface ListProps extends Styled {
  marker?: 'decimal' | 'bullet' | 'lowerAlpha' | 'upperAlpha' | 'none';
  items: string[];
  size?: number;
  color?: string;
  /** Width of the marker gutter. Twice the font size unless said otherwise. */
  gutter?: number;
}

export interface CanvasProps {
  width: number;
  height: number;
  ops: unknown[];
  fill?: string;
  stroke?: { color?: string; width?: number };
  spaceAfter?: number;
}

export interface InlineProps extends Styled {
  children?: ReactNode;
}

export interface SpanProps extends InlineProps {
  color?: string;
}

/**
 * Repeated at the top of every page.
 *
 * Written among the document's children and lifted out of them, because that
 * is where an author will put it and it is not part of the flow.
 */
export const Header = host<BandProps>('header');

/** Repeated at the bottom of every page. */
export const Footer = host<BandProps>('footer');

/** This page's number, filled in as the page is painted. */
export const PageNumber = host<Record<string, never>>('pageNumber');

/**
 * How many pages there are.
 *
 * Costs the memory of the whole document: nothing can know the total until
 * the last page is packed, so nothing can be painted until then. A footer
 * that only numbers its pages pays none of that.
 */
export const PageCount = host<Record<string, never>>('pageCount');

/** A running total, as it stood when this page opened or closed. */
export const RunningTotal = host<RunningTotalProps>('runningTotal');

/** The document itself: page setup, and everything on the pages. */
export const Document = host<DocumentProps>('document');

/** A paragraph. Its children are text and inline styling, never blocks. */
export const Text = host<TextProps>('text');

/** A container: padding, decoration, and children stacked inside. */
export const Box = host<BoxProps>('box');

/** Children set side by side rather than stacked. */
export const Row = host<BoxProps>('row');

export const Image = host<ImageProps>('image');

/** Makes its one child clickable. */
export const Link = host<LinkProps>('link');

/** Vertical space that draws nothing. */
export const Spacer = host<SpacerProps>('spacer');

/** Forces what follows onto a new page. */
export const PageBreak = host<PageBreakProps>('pageBreak');

/** Rows and columns. Geometry only: every visual comes from the props. */
export const Table = host<TableProps>('table');

export const List = host<ListProps>('list');

/** Arbitrary drawing, for whatever the engine has no primitive for. */
export const Canvas = host<CanvasProps>('canvas');

export { Theme, type ThemeProps } from '../element.js';

/** Bold, inside a paragraph. */
export const B = host<InlineProps>('b');

/** Italic, inside a paragraph. */
export const I = host<InlineProps>('i');

/** A coloured stretch, inside a paragraph. */
export const Span = host<SpanProps>('span');

/** Paper sizes in points, portrait. The A series converted from millimetres. */
export const SIZES: Record<PageSize, [number, number]> = {
  A3: [841.8898, 1190.5512],
  A4: [595.2756, 841.8898],
  A5: [419.5276, 595.2756],
  Letter: [612, 792],
  Legal: [612, 1008],
  Tabloid: [792, 1224],
};
