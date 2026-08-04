import { DocumentStream, type StreamOptions, type StreamResult } from '../index.js';

/**
 * A document fed to the engine a piece at a time.
 *
 * The point of it is what the caller never holds: a ledger's rows go past in
 * batches, so the largest thing in the JS heap is one batch rather than the
 * whole document. Nothing else changes — the pages that come out are byte for
 * byte the pages `render` would have produced from the same content declared
 * whole.
 *
 * Each call is a promise and has to be awaited before the next: that is what
 * keeps a batch from queueing up behind the engine, which would put the
 * document back in memory by another route.
 */
export class Printer {
  private readonly inner: DocumentStream;

  /**
   * `page` is the same setup a whole document declares.
   *
   * Bands are given here rather than fed as pieces because they belong to
   * the document: the paginator has to know how much room a header takes
   * before it packs the first row.
   */
  constructor(page: PageSetup, options: PrinterOptions) {
    const { header, footer, ...rest } = options;
    this.inner = new DocumentStream(JSON.stringify(page), {
      ...rest,
      ...(header ? { header: JSON.stringify(header) } : {}),
      ...(footer ? { footer: JSON.stringify(footer) } : {}),
    });
  }

  /**
   * Adds a batch of nodes — headings, paragraphs, whole short tables.
   *
   * Plenty of documents have no table in them at all, and for those this is
   * the whole API. Batch as you would rows: sending a transcript's paragraphs
   * one at a time costs a round trip each and is slower than not streaming.
   */
  nodes(nodes: unknown[]): Promise<void> {
    return this.inner.nodes(JSON.stringify(nodes));
  }

  /** Adds one node. Convenience for the times there is only one. */
  node(node: unknown): Promise<void> {
    return this.nodes([node]);
  }

  /** Begins a table. Its rows follow, in as many batches as suits you. */
  openTable(head: unknown): Promise<void> {
    return this.inner.openTable(JSON.stringify(head));
  }

  /**
   * Adds a batch of rows.
   *
   * Batch size is a memory-against-overhead trade and nothing else. Measured
   * on a hundred thousand rows: one row per batch takes 2.5 s because the hop
   * to the engine's thread dominates; a hundred takes 1.35 s; a thousand takes
   * 1.33 s; ten thousand takes the same 1.33 s but doubles what the heap
   * holds. Anywhere from a hundred to a thousand, and a thousand by default.
   */
  rows(rows: unknown[]): Promise<void> {
    return this.inner.rows(JSON.stringify(rows));
  }

  closeTable(): Promise<void> {
    return this.inner.closeTable();
  }

  /** Paints what is left and closes the file. */
  finish(path?: string): Promise<StreamResult> {
    return this.inner.finish(path);
  }

  /** Atoms the engine is still holding: about a page's worth, always. */
  get pending(): number {
    return this.inner.pending;
  }
}

export interface PrinterOptions extends Omit<StreamOptions, 'header' | 'footer'> {
  /** Repeated at the top of every page. */
  header?: unknown;
  /** Repeated at the bottom of every page. */
  footer?: unknown;
}

export interface PageSetup {
  width?: number;
  height?: number;
  margin?: number | { top?: number; right?: number; bottom?: number; left?: number };
}
