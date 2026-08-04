import type { ReactElement } from 'react';
import type { IrBand, IrDocument, IrNode } from './ir.js';
import { toDocument } from './render.js';

/**
 * A document, in the pieces the engine reads.
 *
 * `toDocument` hands over the whole thing, which for a ledger is the largest
 * object in the process. These are the same content in the order the engine
 * consumes it, so a printer can be fed and the pieces dropped.
 */
export type Chunk =
  | {
      t: 'open';
      page: IrDocument['page'];
      header?: IrBand;
      footer?: IrBand;
      accumulators?: string[];
    }
  | { t: 'nodes'; nodes: IrNode[] }
  | { t: 'openTable'; head: Record<string, unknown> }
  | { t: 'rows'; rows: unknown[] }
  | { t: 'closeTable' };

export interface ChunkOptions {
  /**
   * Rows per batch.
   *
   * A memory-against-overhead trade and nothing else: measured on the engine,
   * one row per batch costs twice the time, a hundred is as fast as it gets,
   * and ten thousand buys nothing while doubling what the heap holds.
   */
  batch?: number;
}

/** Above this a table is worth breaking up; below it, batching buys nothing. */
const WORTH_BREAKING = 200;

const DEFAULT_BATCH = 1000;

/**
 * Renders a document and yields it in pieces.
 *
 * React itself is not streamed — it builds a tree, and a component that
 * returns forty thousand rows has already made them by the time this sees
 * anything. What this avoids is the second copy: the JSON string, the parse,
 * and the engine holding a document it reads once. For the rows themselves to
 * never exist at once, the producer has to be a generator, and that is the
 * caller's own shape rather than something React can impose.
 */
export async function* toChunks(
  element: ReactElement,
  options: ChunkOptions = {},
): AsyncGenerator<Chunk> {
  const batch = options.batch ?? DEFAULT_BATCH;
  const document = await toDocument(element);

  yield {
    t: 'open',
    page: document.page,
    ...(document.header ? { header: document.header } : {}),
    ...(document.footer ? { footer: document.footer } : {}),
    ...(document.accumulators ? { accumulators: document.accumulators } : {}),
  };

  // Ordinary nodes gather until a big table interrupts them, so a document of
  // headings and paragraphs crosses in one piece rather than in hundreds.
  let waiting: IrNode[] = [];
  const flush = function* (): Generator<Chunk> {
    if (waiting.length > 0) {
      yield { t: 'nodes', nodes: waiting };
      waiting = [];
    }
  };

  for (const node of document.children) {
    const rows = (node as { t: string; rows?: unknown[] }).rows;
    if (node.t !== 'table' || !rows || rows.length <= WORTH_BREAKING) {
      waiting.push(node);
      continue;
    }

    yield* flush();
    const { rows: _rows, t: _t, ...head } = node as Record<string, unknown>;
    yield { t: 'openTable', head };
    for (let sent = 0; sent < rows.length; sent += batch) {
      yield { t: 'rows', rows: rows.slice(sent, sent + batch) };
    }
    yield { t: 'closeTable' };
  }

  yield* flush();
}
