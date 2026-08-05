/**
 * A document fed to the engine a piece at a time.
 *
 * The point of it is what the caller never holds: a ledger's rows go past in
 * batches, so the largest thing in the JS heap is one batch rather than the
 * whole document. Nothing else changes — the pages that come out are byte for
 * byte the pages `render` would have produced from the same content declared
 * whole.
 *
 * A session is stateful and cannot move between engines, so a `Printer` takes
 * one out of the pool and holds it until `finish`. On a pool of eight that
 * means eight open documents at once and a ninth waiting, which is the honest
 * cost of streaming rather than a limit worth hiding.
 */
import { asRender, poolFor, type RenderOptions, type RenderResult } from './index.js';
import type { Job, Lease } from './pool.js';

/** The page a document is set on, as the IR declares it. */
export interface PageSetup {
  width: number;
  height: number;
  margin?: unknown;
}

export interface PrinterOptions extends RenderOptions {
  /** A band repeated at the top of every page. */
  header?: unknown;
  footer?: unknown;
  /** Names of the running totals, in the order a band refers to them. */
  accumulators?: string[];
}

export interface StreamResult extends Omit<RenderResult, 'pdf'> {
  /** `null` when a path was given: the bytes went to disk and never here. */
  pdf: Uint8Array | null;
  /** Present only when a path was given. */
  path?: string;
}

export class Printer {
  private lease: Lease | null = null;
  private readonly opening: Promise<void>;
  private pendingAtoms = 0;
  private done = false;

  /**
   * `page` is the same setup a whole document declares.
   *
   * Bands are given here rather than fed as pieces because they belong to the
   * document: the paginator has to know how much room a header takes before it
   * packs the first row.
   */
  constructor(page: PageSetup, options: PrinterOptions) {
    const setup = JSON.stringify({
      page,
      ...(options.header ? { header: options.header } : {}),
      ...(options.footer ? { footer: options.footer } : {}),
      ...(options.accumulators ? { accumulators: options.accumulators } : {}),
    });
    this.opening = (async () => {
      const pool = await poolFor(options);
      const lease = await pool.lease();
      try {
        await lease.send({ op: 'open', setup });
        this.lease = lease;
      } catch (error) {
        // A document that never opened must not sit on an engine.
        lease.release();
        throw error;
      }
    })();
    // Nobody awaits a constructor, so a failure to open would surface as an
    // unhandled rejection before the first call could report it. Held here,
    // and re-raised by whatever is called next.
    this.opening.catch(() => undefined);
  }

  /** Adds a batch of nodes — headings, paragraphs, whole short tables. */
  nodes(nodes: unknown[]): Promise<void> {
    return this.feed({ op: 'nodes', json: JSON.stringify(nodes) });
  }

  /** Adds one node. Convenience for the times there is only one. */
  node(node: unknown): Promise<void> {
    return this.nodes([node]);
  }

  /** Begins a table. Its rows follow, in as many batches as suits you. */
  openTable(head: unknown): Promise<void> {
    return this.feed({ op: 'openTable', json: JSON.stringify(head) });
  }

  /**
   * Adds a batch of rows.
   *
   * Batch size is a memory-against-overhead trade and nothing else. Sending
   * rows one at a time costs a message each and is slower than not streaming;
   * a hundred to a thousand is the range worth being in, and ten thousand buys
   * nothing while doubling what the heap holds.
   */
  rows(rows: unknown[]): Promise<void> {
    return this.feed({ op: 'rows', json: JSON.stringify(rows) });
  }

  closeTable(): Promise<void> {
    return this.feed({ op: 'closeTable' });
  }

  /** Paints what is left and closes the file. */
  async finish(path?: string): Promise<StreamResult> {
    const lease = await this.claim();
    this.done = true;
    try {
      const reply = await lease.send(path ? { op: 'finish', path } : { op: 'finish' });
      if (path) {
        return {
          path: reply.path as string,
          pdf: null,
          pages: reply.pages as number,
          bytes: reply.bytes as number,
          diagnostics: reply.diagnostics as string[],
        };
      }
      return asRender(reply);
    } finally {
      lease.release();
      this.lease = null;
    }
  }

  /**
   * Atoms the engine is still holding: about a page's worth, always.
   *
   * Reported back with every batch rather than fetched on demand, so watching
   * the number the whole design exists to keep flat costs nothing.
   */
  get pending(): number {
    return this.pendingAtoms;
  }

  private async feed(request: Job): Promise<void> {
    const lease = await this.claim();
    const reply = await lease.send(request);
    this.pendingAtoms = (reply.pending as number) ?? this.pendingAtoms;
  }

  private async claim(): Promise<Lease> {
    if (this.done) throw new Error('this document has already been finished');
    await this.opening;
    if (!this.lease) throw new Error('this document is not open');
    return this.lease;
  }
}
