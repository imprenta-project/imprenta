/**
 * The workbook IR, in the shape `imprenta-xlsx` reads.
 *
 * One contract in two languages: every type here has a counterpart in
 * `crates/imprenta-xlsx/src/ir.rs` and `style.rs`, and the two change in the
 * same commit or serde quietly drops the field that only one of them knows
 * about. The end-to-end test is what catches it, because nothing else can.
 */

export interface IrWorkbook {
  sheets: IrSheet[];
}

export interface IrSheet {
  name: string;
  columns?: IrColumn[];
  rows?: IrRow[];
  merges?: IrMerge[];
  freeze?: IrFreeze;
  pictures?: IrPicture[];
}

/**
 * An image floating over the grid, anchored to a cell.
 *
 * Recorded on the sheet rather than in the cell, which is where the format
 * keeps it: a picture is not a value, and a reader looking for one in that
 * cell finds nothing there.
 */
export interface IrPicture {
  /** The name the bytes were handed over under. */
  image: string;
  /** Zero-based, as merges are. */
  row: number;
  column: number;
  /** A nudge from wherever it was placed, in points. */
  dx?: number;
  dy?: number;
  /** In points. The height comes from the image's own pixels. */
  width: number;
  /** Where it sits in the block it hangs from. Absent means the corner. */
  align?: Placement;
  valign?: Placement;
}

export type Placement = 'start' | 'center' | 'end';

export interface IrColumn {
  /** In Excel's own unit: about the width of one digit of the body font. */
  width?: number;
  style?: CellStyle;
}

export interface IrRow {
  cells?: IrCell[];
  /** In points. */
  height?: number;
  style?: CellStyle;
  /** Whether these cells are the labels of the sheet's autofilter. */
  filter?: boolean;
}

export interface IrCell {
  value: IrValue;
  style?: CellStyle;
}

/**
 * What a cell holds, and therefore what Excel will let you do with it.
 *
 * The tag is written out rather than inferred from the shape: a producer that
 * means "the text 007" and one that means "the number 7" must be able to say
 * which, and a JSON number cannot carry the difference on its own.
 */
export type IrValue =
  | { t: 'blank' }
  | { t: 'text'; v: string }
  | { t: 'number'; v: number }
  | { t: 'bool'; v: boolean }
  /** An Excel serial. There is no date type underneath one. */
  | { t: 'date'; v: number }
  | { t: 'formula'; v: { formula: string; cached?: number } };

/** Zero-based, both ends included. */
export interface IrMerge {
  fromRow: number;
  fromColumn: number;
  toRow: number;
  toColumn: number;
}

export interface IrFreeze {
  rows?: number;
  columns?: number;
}

export type Side = 'top' | 'right' | 'bottom' | 'left';

/** The border widths and styles Excel has, and no others. */
export type Line = 'thin' | 'medium' | 'thick' | 'dashed' | 'dotted' | 'double';

export interface IrBorder {
  style: Line;
  color?: string;
}

export interface IrFont {
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  /** In points, which is what Excel measures type in. */
  size?: number;
  color?: string;
  /** Nothing is embedded: a spreadsheet asks, and the machine that opens it
   * supplies the face or does not. */
  name?: string;
}

export interface IrAlignment {
  horizontal?: 'left' | 'center' | 'right' | 'justify';
  vertical?: 'top' | 'middle' | 'bottom';
  wrap?: boolean;
  /** Excel counts these in units of about three characters. */
  indent?: number;
}

/** Everything Excel can be told about one cell's appearance. */
export interface CellStyle {
  font?: IrFont;
  /** A solid fill. */
  fill?: string;
  border?: Partial<Record<Side, IrBorder>>;
  align?: IrAlignment;
  /** A number format code, such as `#,##0.00` or `dd/mm/yyyy`. */
  format?: string;
}
