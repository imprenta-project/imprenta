/**
 * A workbook fed to the writer a batch of rows at a time.
 *
 * The point of it is what the caller never holds: an export's rows go past in
 * batches, so the largest thing in the JS heap is one batch rather than the
 * whole ledger. Measured on the Rust side, a million rows cost 1.77 GB
 * declared and 47.9 MB streamed, and the bytes that come out are identical.
 *
 * A session is stateful and cannot move between writers, so a `Book` takes one
 * out of the pool and holds it until `finish`.
 */
import { asWrite, type WriteOptions, type WriteResult, writers } from './index.js';
import type { Job, Lease } from './pool.js';

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

export interface BookOptions extends WriteOptions {
  /**
   * Write straight to this file instead of handing back the bytes.
   *
   * The package is written from the worker, so a large export never becomes a
   * buffer in the calling thread's heap.
   */
  path?: string;
}

export interface StreamResult extends Omit<WriteResult, 'xlsx'> {
  /** `null` when a path was given: the bytes went to disk and never here. */
  xlsx: Uint8Array | null;
  /** Present only when a path was given. */
  path?: string;
}

export class Book {
  private lease: Lease | null = null;
  private readonly opening: Promise<void>;
  private readonly path?: string;
  private rowsIn = 0;
  private done = false;

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
    this.path = options.path;
    const declared = JSON.stringify(sheets);
    this.opening = (async () => {
      const pool = await writers(options);
      const lease = await pool.lease();
      try {
        await lease.send({ op: 'open', sheets: declared });
        this.lease = lease;
      } catch (error) {
        // A workbook that never opened must not sit on a writer.
        lease.release();
        throw error;
      }
    })();
    // Nobody awaits a constructor, so a failure to open would surface as an
    // unhandled rejection before the first call could report it.
    this.opening.catch(() => undefined);
  }

  /**
   * Adds a batch of rows to the sheet that is open.
   *
   * Batch size trades memory against call overhead. A hundred to a few
   * thousand is comfortable; ten thousand upwards only buys back the memory
   * this exists to save.
   */
  rows(rows: unknown[]): Promise<void> {
    return this.feed({ op: 'rows', json: JSON.stringify(rows) });
  }

  /** Adds one row. Convenience for the times there is only one. */
  row(row: unknown): Promise<void> {
    return this.rows([row]);
  }

  /** Closes the open sheet and moves to the next one declared. */
  nextSheet(): Promise<void> {
    return this.feed({ op: 'nextSheet' });
  }

  /**
   * Merges a block of the open sheet, counted from its top left.
   *
   * Can be called after the rows it covers, because merges are written at the
   * end of a sheet — which matters, since a total row's span is the last thing
   * a streaming producer learns. {@link at} says where the sheet has got to.
   */
  merge(merge: MergeRange): Promise<void> {
    return this.feed({ op: 'merge', json: JSON.stringify(merge) });
  }

  /** How many rows have gone into the open sheet. The next one is this. */
  get at(): number {
    return this.rowsIn;
  }

  /**
   * Closes the workbook.
   *
   * With a path the file was written from the worker and nothing comes back
   * but its size; without one the bytes come back.
   */
  async finish(): Promise<StreamResult> {
    const lease = await this.claim();
    this.done = true;
    try {
      const reply = await lease.send(
        this.path ? { op: 'finish', path: this.path } : { op: 'finish' },
      );
      if (this.path) {
        return {
          path: reply.path as string,
          xlsx: null,
          bytes: reply.bytes as number,
          sheets: reply.sheets as number,
        };
      }
      return asWrite(reply);
    } finally {
      lease.release();
      this.lease = null;
    }
  }

  private async feed(request: Job): Promise<void> {
    const lease = await this.claim();
    const reply = await lease.send(request);
    this.rowsIn = (reply.at as number) ?? this.rowsIn;
  }

  private async claim(): Promise<Lease> {
    if (this.done) throw new Error('this workbook has already been finished');
    await this.opening;
    if (!this.lease) throw new Error('this workbook is not open');
    return this.lease;
  }
}
