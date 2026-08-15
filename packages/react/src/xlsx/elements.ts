import type { ReactNode } from 'react';
import { host } from '../element.js';

/**
 * The vocabulary a spreadsheet is written in.
 *
 * Deliberately not the document one with different props. There is no
 * `<Document>`, no margin and no page here, because a workbook has none of
 * those; and there is a `<Cell>` that carries a **typed value**, because that
 * is the thing a spreadsheet has and a printed page does not.
 */

/** Tailwind classes, resolved against the theme in force. */
interface Styled {
  className?: string;
}

export interface WorkbookProps {
  children?: ReactNode;
}

export interface SheetProps {
  /**
   * The name on the tab.
   *
   * Excel allows 31 characters and forbids `[]:*?/\`. A name that breaks
   * either rule is refused when the workbook is built rather than silently
   * truncated, because two sheets truncated to the same name is a workbook
   * that will not open.
   */
  name: string;
  /** Rows and columns held still while the rest scrolls. */
  freeze?: { rows?: number; columns?: number };

  /**
   * The sheet's rows as plain data, appended after whatever rows the children
   * declare.
   *
   * The same shape `<Row>` and `<Cell>` spell out, minus the elements — and
   * that is the point. A React element per cell costs a fiber, an instance
   * and a props object for the duration of one synchronous render; measured
   * on 200 000 rows, 6 427 bytes of heap per row against 865 for the same
   * rows as data (issue #11). A ledger declares its header band as children,
   * where layout reads naturally, and hands the body over here, where a
   * hundred thousand rows are just an array.
   */
  rows?: SheetRow[];

  children?: ReactNode;
}

/** One row of {@link SheetProps.rows}: what `<Row>` says, as data. */
export interface SheetRow {
  cells?: SheetCell[];
  /** In points, as everything vertical is. */
  height?: number;
  /** Tailwind classes, resolved against the theme in force. */
  className?: string;
  /** Whether this row's cells are the labels of the sheet's autofilter. */
  filter?: boolean;
}

/** One cell of a data row: what `<Cell>` says, as data. Text goes in `value` —
 * a string stays text, leading zeros and all, exactly as children would. */
export interface SheetCell {
  value?: string | number | boolean | Date;
  /** A formula, with or without its leading `=`. */
  formula?: string;
  /** What the formula comes to, if the producer already knows. */
  cached?: number;
  /** A number format code, such as `#,##0.00` or `dd/mm/yyyy`. */
  format?: string;
  /** Tailwind classes, resolved against the theme in force. */
  className?: string;
  /** How many columns this cell covers. Becomes a merge. */
  colSpan?: number;
  /** How many rows this cell covers. Becomes a merge. */
  rowSpan?: number;
  /** An image hung off this cell, as `<Image>` inside a `<Cell>` would be. */
  image?: SheetImage;
}

/** What `<Image>` says, as data on a cell of {@link SheetProps.rows}. */
export interface SheetImage {
  /** The name the bytes were handed over under. */
  src: string;
  /** In points. The height comes from the image's own pixels. */
  width: number;
  align?: 'start' | 'center' | 'end';
  valign?: 'start' | 'center' | 'end';
  /** A nudge from wherever it was placed, in points. */
  offset?: { x?: number; y?: number };
}

export interface ColumnProps extends Styled {
  /**
   * In Excel's own unit: about the width of one digit of the body font.
   *
   * Not points and not pixels. A spreadsheet is a grid of text and Excel
   * measures its columns in characters, so this is the one measurement in
   * Imprenta that is not a length.
   */
  width?: number;
  /** A number format every cell in this column falls back on. */
  format?: string;
}

export interface RowProps extends Styled {
  /** In points, as everything vertical is. */
  height?: number;

  /**
   * Whether this row's cells are the labels of an autofilter.
   *
   * The dropdowns Excel puts on a header so the recipient can filter and sort
   * each column. Marked on the row rather than declared as a range, because
   * the range ends at the last row of the sheet — and a producer streaming a
   * million rows does not know which that is. The engine works it out when the
   * sheet closes, which is the only moment anybody can.
   *
   * One to a sheet, which is what Excel allows. Two rows asking for it is
   * refused rather than letting the second quietly win.
   */
  filter?: boolean;

  children?: ReactNode;
}

export interface ImageProps {
  /**
   * The name the bytes were handed over under.
   *
   * The image itself never travels in the workbook — `write(ir, { images })`
   * takes the bytes — so a declared sheet can be serialised, cached or put on
   * a queue without a logo stuck to it.
   */
  src: string;

  /**
   * How wide to draw it, in points.
   *
   * There is no height, and that is deliberate: the image's own pixels give
   * the ratio. Asking for both is the one way to squash a logo, and it is
   * always somebody copying the numbers off the last one.
   */
  width: number;

  /**
   * Where it sits inside the cell — or inside the merge that swallowed it.
   *
   * Centring cannot be worked out by whoever declares the sheet: it needs the
   * picture's height, and the height comes from the image's own pixels, which
   * only the engine has read. A producer that computed an offset itself would
   * get it right for the logo in front of it and wrong for the next one,
   * silently, because the picture is still on the page.
   *
   * Defaults to the top-left corner, which is where a picture goes if nobody
   * says otherwise.
   */
  align?: 'start' | 'center' | 'end';
  valign?: 'start' | 'center' | 'end';

  /** A nudge from wherever it was placed, in points. */
  offset?: { x?: number; y?: number };
}

export interface CellProps extends Styled {
  /**
   * What is in the cell, with its type.
   *
   * A number stays a number, a boolean stays a boolean, and a `Date` becomes
   * the serial Excel keeps underneath a date. This is the whole difference
   * from a printed page: text children give you the characters, and `value`
   * gives you something `SUM` can add up.
   *
   * ```tsx
   * <Cell>007</Cell>          // the text 007, leading zeros kept
   * <Cell value={7} />        // the number 7
   * ```
   */
  value?: string | number | boolean | Date;

  /** A formula, with or without its leading `=`. */
  formula?: string;

  /**
   * What the formula comes to, if the producer already knows.
   *
   * Excel recalculates on open and does not need this. Every reader that only
   * reads — pandas, a script, a preview — does: to them a formula with no
   * cached value is an empty cell, and a total that vanishes when the file is
   * read by a script is a bad surprise.
   */
  cached?: number;

  /** A number format code, such as `#,##0.00` or `dd/mm/yyyy`. */
  format?: string;

  /** How many columns this cell covers. Becomes a merge. */
  colSpan?: number;
  /** How many rows this cell covers. Becomes a merge. */
  rowSpan?: number;

  /** Text, when there is no `value`. */
  children?: ReactNode;
}

/** The workbook. One per render, and it holds sheets and nothing else. */
export const Workbook = host<WorkbookProps>('workbook');

/** One sheet, which is one tab. */
export const Sheet = host<SheetProps>('sheet');

/**
 * A column's width and the format its cells fall back on.
 *
 * Declared in order, before the rows. A column nobody mentions keeps Excel's
 * default width, so only say what differs.
 */
export const Column = host<ColumnProps>('column');

/**
 * A row of cells.
 *
 * A style here reaches the cells the row does not have, which is what makes a
 * shaded header band run the full width rather than stopping at the last
 * cell — Excel formats rows natively and this is that.
 */
export const Row = host<RowProps>('row');

/** One cell: what is in it, what type it is, and how it is formatted. */
export const Cell = host<CellProps>('cell');

/**
 * An image, hung off the cell it is written in.
 *
 * ```tsx
 * <Cell><Image src="logo" width={120} /></Cell>
 * ```
 *
 * Written inside a cell rather than declared beside the sheet with a row and
 * a column. Coordinates would be a second thing to keep in step with the
 * rows: insert a header above and the logo stays where it was, which is the
 * bug the anchor exists to prevent. It floats over the grid rather than
 * sitting in the cell, so the cell it names stays empty.
 */
export const Image = host<ImageProps>('image');

export { Theme, type ThemeProps } from '../element.js';
