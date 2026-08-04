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
  children?: ReactNode;
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
  children?: ReactNode;
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

export { Theme, type ThemeProps } from '../element.js';
