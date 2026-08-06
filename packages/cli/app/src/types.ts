export interface Listing {
  documentsDir: string;
  configPath: string | null;
  documents: { id: string; group: string | null }[];
}

export interface Finding {
  rule: string;
  status: 'error' | 'warning';
  source: 'document' | 'engine';
  detail: string;
  where?: string;
  occurrences: number;
}

/** Which of the two a component turned out to declare, by what it returned. */
export type Format = 'pdf' | 'xlsx';

export interface Report {
  id: string;
  format: Format;
  /** Pages, for a document. Sheets, for a workbook. */
  parts: number;
  bytes: number;
  checks: Finding[];
  ir: unknown;
}

export type View = 'preview' | 'source';

/**
 * The page the engine was told to print on, in points.
 *
 * The only part of the document IR this app reads for itself. Everything else
 * it shows came back measured; this is what the frame has to be shaped like.
 */
export interface PageSetup {
  width: number;
  height: number;
}

export interface IrDocument {
  page?: PageSetup;
}

/**
 * As much of the workbook IR as the grid reads.
 *
 * Not imported from `@imprentajs/react/xlsx`: this is the browser app, and it
 * has no business depending on the authoring library to draw a table. It reads
 * what the server sent, which is JSON.
 */
export type Side = 'top' | 'right' | 'bottom' | 'left';

export interface IrWorkbook {
  sheets: IrSheet[];
}

export interface IrSheet {
  name: string;
  columns?: { width?: number }[];
  rows?: IrRow[];
  merges?: { fromRow: number; fromColumn: number; toRow: number; toColumn: number }[];
  freeze?: { rows?: number; columns?: number };
  pictures?: IrPicture[];
}

export interface IrPicture {
  image: string;
  row: number;
  column: number;
  dx?: number;
  dy?: number;
  width: number;
  align?: 'start' | 'center' | 'end';
  valign?: 'start' | 'center' | 'end';
}

export interface IrRow {
  cells?: IrCell[];
  height?: number;
  style?: CellStyle;
}

export interface IrCell {
  value?:
    | { t: 'blank' }
    | { t: 'text'; v: string }
    | { t: 'number'; v: number }
    | { t: 'bool'; v: boolean }
    | { t: 'date'; v: number }
    | { t: 'formula'; v: { formula: string; cached?: number } };
  style?: CellStyle;
}

export interface CellStyle {
  font?: {
    bold?: boolean;
    italic?: boolean;
    underline?: boolean;
    strike?: boolean;
    size?: number;
    color?: string;
  };
  fill?: string;
  border?: Partial<Record<Side, { style: string; color?: string }>>;
  align?: {
    horizontal?: 'left' | 'center' | 'right' | 'justify';
    vertical?: 'top' | 'middle' | 'bottom';
    wrap?: boolean;
    indent?: number;
  };
  format?: string;
}
