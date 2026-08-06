/**
 * Imprenta's PDF engine.
 *
 * The engine is a WebAssembly module that imports nothing at all, so one
 * artefact runs in Node, a browser, Deno, Bun and on an edge worker — there is
 * no per-platform binary and nothing for a host to provide.
 *
 * # Nothing runs on the calling thread
 *
 * A WebAssembly call is synchronous and a long document takes seconds, so a
 * render on the thread that answers requests would stop a service answering.
 * Every function here goes to a worker instead, and returns a promise — the
 * same contract the native addon offered over libuv's pool, one boundary
 * further out.
 *
 * The pool starts on the first call and reuses its engines afterwards, so the
 * fonts are copied into WebAssembly once rather than once a document. If a
 * process prints in bursts and wants the memory back in between, {@link close}
 * stops it; the next call starts it again.
 *
 * If you are already off the main thread — inside a worker, in a CLI, in the
 * browser — reach for {@link Engine} instead and skip the hop.
 */
import { Pool, type PoolOptions } from './pool.js';
import { renderSharded, shardable, WORTH_SHARDING } from './shard.js';

export { Engine, EngineError, Printer as SyncPrinter } from './engine.js';
export type { Font, Image } from './pool.js';

/** A typeface and the file behind it. */
export interface FontSource {
  /** `"regular"` or `"bold"`. Defaults to regular. */
  weight?: string;
  italic?: boolean;
  data: Uint8Array;
}

/**
 * An image the document refers to by name.
 *
 * Format and pixel size are read from the bytes; the caller supplies neither.
 */
export interface ImageSource {
  name: string;
  data: Uint8Array;
}

export interface RenderOptions {
  fonts: FontSource[];
  images?: ImageSource[];
  /**
   * How many engines to keep. Defaults to the number of cores, capped at
   * eight, because each one holds its own linear memory.
   */
  size?: number;
  /** The module's bytes. Defaults to the one shipped with the package. */
  wasm?: ArrayBufferLike | ArrayBufferView;
  /**
   * Whether one long document may be split across the pool.
   *
   * On by default, and it only ever applies to documents that can take it —
   * see `shardable` in `shard.ts` for what those are, and why the rest render
   * on one engine instead. Turn it off to compare, or if you would rather
   * spend the whole pool on other requests than on this document.
   */
  shard?: boolean;
  /**
   * Linear memory, in bytes, above which an engine is replaced once the
   * document that grew it is finished. Defaults to 64 MB; `Infinity` keeps
   * every engine for the life of the process.
   *
   * WebAssembly memory only ever grows — there is no instruction to shrink
   * one — so an engine's footprint is the high-water mark of the largest
   * document it has ever rendered. Without this, a service that prints one
   * ledger a month carries that ledger's memory from the day it arrived,
   * once per engine in the pool.
   */
  recycleAbove?: number;
}

export interface RenderResult {
  pdf: Uint8Array;
  pages: number;
  /** Size of the PDF in bytes. */
  bytes: number;
  /** Anything the engine noticed — clipped text, a character no font covers. */
  diagnostics: string[];
}

/** As {@link RenderResult}, for a document that went straight to disk. */
export interface WriteResult {
  path: string;
  pages: number;
  bytes: number;
  diagnostics: string[];
}

/**
 * One pool per set of assets.
 *
 * Keyed by the fonts and images, so a service printing invoices and reports
 * with different families keeps an engine for each rather than reloading them
 * on every call — and a service with one set keeps exactly one pool.
 */
const pools = new Map<string, Promise<Pool>>();

function keyFor(options: RenderOptions): string {
  const parts = [
    String(options.size ?? ''),
    String(options.recycleAbove ?? ''),
    ...options.fonts.map(
      (f) => `${f.weight ?? 'regular'}/${f.italic ? 'i' : 'r'}/${f.data.length}`,
    ),
    ...(options.images ?? []).map((i) => `${i.name}/${i.data.length}`),
  ];
  return parts.join('|');
}

function poolFor(options: RenderOptions): Promise<Pool> {
  const key = keyFor(options);
  let pool = pools.get(key);
  if (!pool) {
    pool = Pool.start(options as PoolOptions);
    pools.set(key, pool);
    // A pool that failed to start must not be remembered as one that did.
    pool.catch(() => pools.delete(key));
  }
  return pool;
}

/** Renders a declared document and hands back the bytes. */
export async function render(ir: string, options: RenderOptions): Promise<RenderResult> {
  const pool = await poolFor(options);

  // Long enough to be worth the extra passes, and shaped so that splitting it
  // cannot change where the pages fall. Anything else goes down the ordinary
  // path, which is not a fallback so much as the same engine used the simple
  // way.
  if (options.shard !== false && pool.size > 1) {
    const document = shardable(JSON.parse(ir));
    if (document && document.rows.length >= WORTH_SHARDING) {
      return renderSharded(pool, document);
    }
  }

  const reply = await pool.run({ op: 'render', ir });
  return asRender(reply);
}

/**
 * Renders a declared document straight to a file.
 *
 * Preferred for anything large: the PDF is written from the worker, so a
 * 128 MB ledger never becomes a 128 MB buffer in the calling thread's heap on
 * its way to disk.
 */
export async function renderToFile(
  ir: string,
  path: string,
  options: RenderOptions,
): Promise<WriteResult> {
  const pool = await poolFor(options);
  const reply = await pool.run({ op: 'renderToFile', ir, path });
  return {
    path: reply.path as string,
    pages: reply.pages as number,
    bytes: reply.bytes as number,
    diagnostics: reply.diagnostics as string[],
  };
}

/**
 * Stops every engine and forgets them.
 *
 * Rarely needed in a server, which wants them warm. Worth calling in a script
 * or a test that would otherwise sit with a pool of workers alive at the end.
 */
export async function close(): Promise<void> {
  const running = [...pools.values()];
  pools.clear();
  await Promise.all(running.map((p) => p.then((pool) => pool.close()).catch(() => undefined)));
}

/** @internal — shared with the streaming half. */
export function asRender(reply: Record<string, unknown>): RenderResult {
  const pdf = new Uint8Array(reply.pdf as ArrayBuffer);
  return {
    pdf,
    pages: reply.pages as number,
    bytes: pdf.length,
    diagnostics: reply.diagnostics as string[],
  };
}

/** @internal — shared with the streaming half. */
export { poolFor };
