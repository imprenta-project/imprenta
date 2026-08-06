/**
 * Imprenta's spreadsheet writer.
 *
 * The writer is a WebAssembly module that imports nothing at all, so one
 * artefact runs in Node, a browser, Deno, Bun and on an edge worker.
 *
 * # Nothing runs on the calling thread
 *
 * A WebAssembly call is synchronous and a million-row export takes seconds, so
 * everything here goes to a worker and returns a promise — the same contract
 * the native binding offered over libuv's pool.
 *
 * If you are already off the main thread — inside a worker, in a CLI, in the
 * browser — reach for {@link Writer} instead and skip the hop.
 */
import { Pool, type PoolOptions } from './pool.js';
import type { Image } from './writer.js';

export type { Image, SheetSetup, WriteOutcome, WriterOptions } from './writer.js';
export { EngineError, SyncBook, Writer } from './writer.js';

export interface WriteResult {
  xlsx: Uint8Array;
  /** Size of the workbook in bytes. */
  bytes: number;
  sheets: number;
}

/** As {@link WriteResult}, for a workbook that went straight to disk. */
export interface FileResult {
  path: string;
  bytes: number;
  sheets: number;
}

export interface WriteOptions extends PoolOptions {
  /**
   * The bytes behind the names the sheets' pictures use.
   *
   * Given per workbook rather than per pool: a pool is warm across callers,
   * and a logo belongs to the export rather than to the writer that happened
   * to take it.
   */
  images?: Image[];
}

/**
 * One pool, started on the first call.
 *
 * Unlike the page engine there is nothing to key it on: a workbook carries its
 * own styles and there are no fonts to load, so every caller wants the same
 * writer.
 */
let pool: Promise<Pool> | null = null;

function writers(options: WriteOptions = {}): Promise<Pool> {
  if (!pool) {
    pool = Pool.start(options);
    // A pool that failed to start must not be remembered as one that did.
    pool.catch(() => {
      pool = null;
    });
  }
  return pool;
}

/** Writes a declared workbook and hands back the bytes. */
export async function write(ir: string, options?: WriteOptions): Promise<WriteResult> {
  const reply = await (await writers(options)).run({ op: 'write', ir, images: options?.images });
  return asWrite(reply);
}

/**
 * Writes a declared workbook straight to a file.
 *
 * Preferred for anything large: the package is written from the worker, so a
 * big export never becomes a buffer in the calling thread's heap.
 */
export async function writeToFile(
  ir: string,
  path: string,
  options?: WriteOptions,
): Promise<FileResult> {
  const reply = await (await writers(options)).run({
    op: 'writeToFile',
    ir,
    path,
    images: options?.images,
  });
  return {
    path: reply.path as string,
    bytes: reply.bytes as number,
    sheets: reply.sheets as number,
  };
}

/**
 * Stops every writer and forgets them.
 *
 * Rarely needed in a server, which wants them warm. Worth calling in a script
 * or a test that would otherwise sit with workers alive at the end.
 */
export async function close(): Promise<void> {
  const running = pool;
  pool = null;
  if (running) await running.then((p) => p.close()).catch(() => undefined);
}

/** @internal — shared with the streaming half. */
export function asWrite(reply: Record<string, unknown>): WriteResult {
  const xlsx = new Uint8Array(reply.xlsx as ArrayBuffer);
  return { xlsx, bytes: xlsx.length, sheets: reply.sheets as number };
}

/** @internal — shared with the streaming half. */
export { writers };
