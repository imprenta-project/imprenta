import { type StreamResult, WorkbookStream } from '../index.js';

/**
 * A workbook fed to the writer a batch of rows at a time.
 *
 * The point of it is what the caller never holds: an export's rows go past in
 * batches, so the largest thing in the JS heap is one batch rather than the
 * whole ledger. Measured on the Rust side, a million rows cost 1.77 GB
 * declared and 47.9 MB streamed, and the bytes that come out are identical.
 *
 * Each call is a promise and has to be awaited before the next: a second one
 * while the first is running is refused, because two promises in flight have
 * no order and a spreadsheet is written in order.
 */
export class Book {
  private readonly inner: WorkbookStream;

  /**
   * `sheets` are the sheets as they will appear, without their rows.
   *
   * Names, columns and frozen panes have to be known before anything is
   * written — the package declares every sheet in its first entry — but the
   * rows are exactly what a streaming producer does not have yet. A sheet may
   * carry rows here and they are written first, so declaring a header and
   * feeding the body is the obvious thing.
   */
  constructor(sheets: SheetSetup[], options: BookOptions = {}) {
    this.inner = new WorkbookStream(JSON.stringify(sheets), options.path);
  }

  /**
   * Adds a batch of rows to the sheet that is open.
   *
   * Batch size trades memory against call overhead. Unlike the PDF side there
   * is no thread to hop, so small batches cost almost nothing in time — a
   * hundred to a few thousand is comfortable, and ten thousand upwards only
   * buys back the memory this exists to save.
   */
  rows(rows: unknown[]): Promise<void> {
    return this.inner.rows(JSON.stringify(rows));
  }

  /** Adds one row. Convenience for the times there is only one. */
  row(row: unknown): Promise<void> {
    return this.rows([row]);
  }

  /** Closes the open sheet and moves to the next one declared. */
  nextSheet(): Promise<void> {
    return this.inner.nextSheet();
  }

  /**
   * Merges a block of the open sheet, counted from its top left.
   *
   * Can be called after the rows it covers, because merges are written at the
   * end of a sheet — which matters, since a total row's span is the last thing
   * a streaming producer learns.
   */
  merge(merge: MergeRange): Promise<void> {
    return this.inner.merge(JSON.stringify(merge));
  }

  /**
   * Closes the workbook.
   *
   * With a path given to the constructor the file was written from Rust and
   * nothing comes back but its size; without one the bytes come back, which
   * costs a copy of them in the JS heap.
   */
  finish(): Promise<StreamResult> {
    return this.inner.finish();
  }
}

/** A sheet as it will appear, before any rows have been fed to it. */
export interface SheetSetup {
  name: string;
  columns?: { width?: number; style?: unknown }[];
  rows?: unknown[];
  merges?: MergeRange[];
  freeze?: { rows?: number; columns?: number };
}

/** Zero-based, both ends included. */
export interface MergeRange {
  fromRow: number;
  fromColumn: number;
  toRow: number;
  toColumn: number;
}

export interface BookOptions {
  /**
   * Write straight to this file instead of handing back the bytes.
   *
   * Preferred for anything large: a hundred-megabyte export should not exist
   * as a hundred megabytes in the JS heap on its way to disk.
   */
  path?: string;
}
