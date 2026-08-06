/**
 * One engine, on its own thread.
 *
 * The rule this package has always stated first: **nothing runs on the main
 * thread.** A WebAssembly call is synchronous and a long document takes
 * seconds, so a render on the thread that answers requests stops the service
 * answering. The native addon met that with libuv's pool; this meets it with a
 * worker, which is the same promise to the caller and one boundary instead of
 * a thread inside the binding.
 *
 * Deliberately dumb: it owns an [`Engine`] and answers messages. Every
 * decision about how many of these there are lives in `pool.ts`.
 */
import { parentPort, workerData } from 'node:worker_threads';
import { Engine, type Printer } from './engine.js';
import { compile } from './module.js';

export interface BootData {
  wasm: ArrayBuffer;
  fonts: { weight?: 'regular' | 'bold'; italic?: boolean; data: ArrayBuffer }[];
  images: { name: string; data: ArrayBuffer }[];
  /**
   * Throwaway documents rendered before reporting ready.
   *
   * Measured on this engine: a chunk a warm instance renders in 66 ms takes
   * 190 ms on a cold one, almost all of it the runtime tiering the module up.
   * A pool exists to pay that at boot rather than on somebody's request.
   */
  warmups: number;
  /**
   * Linear memory, in bytes, above which the instance is replaced once the
   * document it grew on is finished.
   *
   * WebAssembly has no instruction to shrink a memory, so an instance's
   * footprint is the high-water mark of the largest document it has ever
   * rendered, for as long as it lives. A service that prints one ledger a
   * month and invoices the rest of the time would otherwise carry that
   * ledger's memory from the day it first arrived, once per worker.
   *
   * A new instance is the only thing that gives it back. It costs a fresh set
   * of fonts and a cold start — real, and nothing beside the seconds the
   * document that tripped it took.
   */
  recycleAbove: number;
}

/** Everything the pool can ask for. One message, one reply, always. */
export type Request =
  | { id: number; op: 'render'; ir: string }
  | { id: number; op: 'renderToFile'; ir: string; path: string }
  | { id: number; op: 'open'; setup: string }
  | { id: number; op: 'nodes'; json: string }
  | { id: number; op: 'openTable'; json: string }
  | { id: number; op: 'rows'; json: string }
  | { id: number; op: 'closeTable' }
  | { id: number; op: 'finish'; path?: string }
  /** How much linear memory this worker's instance is holding. */
  | { id: number; op: 'memory' }
  // The four passes of a sharded render. See `shard.ts` for why there are
  // four and why they run in this order.
  | { id: number; op: 'measure'; setup: string; head: string; rows: string }
  | {
      id: number;
      op: 'fragmentMeasured';
      setup: string;
      head: string;
      from: number;
      to: number;
      extra: string;
    }
  | { id: number; op: 'plan'; setup: string; heights: ArrayBuffer }
  | { id: number; op: 'fragment'; setup: string; head: string; rows: string }
  | { id: number; op: 'merge'; fragments: ArrayBuffer[] };

const WARMUP = JSON.stringify({
  page: { width: 595, height: 842 },
  children: Array.from({ length: 200 }, (_, i) => ({
    t: 'text',
    runs: [{ text: `Calentando el motor, línea ${i}` }],
  })),
});

async function main(): Promise<void> {
  const port = parentPort;
  if (!port) throw new Error('this module is only meaningful inside a worker');
  const boot = workerData as BootData;

  // Compiled once and kept. Recycling makes a new *instance* — a new linear
  // memory — and reuses the module, so the runtime keeps the tiered-up code
  // the warm-up paid for.
  const module = await compile(new Uint8Array(boot.wasm));
  const assets = {
    fonts: boot.fonts.map((f) => ({ ...f, data: new Uint8Array(f.data) })),
    images: boot.images.map((i) => ({ ...i, data: new Uint8Array(i.data) })),
  };

  let engine = await Engine.load({ wasm: module, ...assets });
  for (let i = 0; i < boot.warmups; i++) engine.render(WARMUP);

  let printer: Printer | null = null;

  /**
   * Replaces the instance when the document it just finished left it swollen.
   *
   * After the reply, never before it: the caller has its bytes and is not
   * kept waiting for an instantiation it did not ask for. Never while a
   * document is open either — a session lives inside one instance and cannot
   * move to another.
   */
  const recycle = async (): Promise<void> => {
    if (printer || engine.memoryBytes <= boot.recycleAbove) return;
    engine = await Engine.load({ wasm: module, ...assets });
  };

  /**
   * Written from here rather than handed back, so a 128 MB ledger never
   * becomes a 128 MB Buffer in the main thread's heap on its way to disk.
   * The native addon wrote it from Rust; a worker is the same guarantee, one
   * boundary further out.
   */
  const toFile = async (bytes: Uint8Array, path: string) => {
    const { writeFile } = await import('node:fs/promises');
    await writeFile(path, bytes);
  };

  port.on('message', async (request: Request) => {
    try {
      switch (request.op) {
        case 'render': {
          const out = engine.render(request.ir);
          const buffer = out.pdf.buffer as ArrayBuffer;
          // Transferred, not cloned: the PDF is the largest thing that
          // crosses and a copy would double the peak for nothing.
          port.postMessage(
            { id: request.id, pdf: buffer, pages: out.pages, diagnostics: out.diagnostics },
            [buffer],
          );
          await recycle();
          return;
        }
        case 'renderToFile': {
          const out = engine.render(request.ir);
          await toFile(out.pdf, request.path);
          port.postMessage({
            id: request.id,
            path: request.path,
            pages: out.pages,
            bytes: out.bytes,
            diagnostics: out.diagnostics,
          });
          await recycle();
          return;
        }
        case 'memory': {
          // Diagnostics, and the only way to see from outside that an
          // instance was replaced. `pool.test.ts` is what it is for.
          port.postMessage({ id: request.id, bytes: engine.memoryBytes });
          return;
        }
        case 'measure': {
          const heights = engine.measureRows(request.setup, request.head, request.rows);
          const buffer = heights.buffer as ArrayBuffer;
          port.postMessage({ id: request.id, heights: buffer }, [buffer]);
          return;
        }
        case 'plan': {
          const plan = engine.plan(request.setup, new Uint8Array(request.heights));
          port.postMessage({ id: request.id, plan });
          return;
        }
        case 'fragmentMeasured': {
          const out = engine.fragmentMeasured(
            request.setup,
            request.head,
            request.from,
            request.to,
            request.extra,
          );
          const buffer = out.pdf.buffer as ArrayBuffer;
          port.postMessage(
            { id: request.id, pdf: buffer, pages: out.pages, diagnostics: out.diagnostics },
            [buffer],
          );
          await recycle();
          return;
        }
        case 'fragment': {
          // One piece of a document, told which page it starts on. Fed rather
          // than declared so the rows never become a second copy of
          // themselves inside the module.
          const piece = engine.printer(JSON.parse(request.setup));
          piece.openTable(JSON.parse(request.head));
          piece.rows(JSON.parse(request.rows));
          piece.closeTable();
          const out = piece.finish();
          const buffer = out.pdf.buffer as ArrayBuffer;
          port.postMessage(
            { id: request.id, pdf: buffer, pages: out.pages, diagnostics: out.diagnostics },
            [buffer],
          );
          await recycle();
          return;
        }
        case 'merge': {
          const out = engine.merge(request.fragments.map((f) => new Uint8Array(f)));
          const buffer = out.pdf.buffer as ArrayBuffer;
          port.postMessage(
            { id: request.id, pdf: buffer, pages: out.pages, diagnostics: out.diagnostics },
            [buffer],
          );
          await recycle();
          return;
        }
        case 'open':
          printer = engine.printer(JSON.parse(request.setup));
          port.postMessage({ id: request.id, ok: true });
          return;
        case 'nodes':
        case 'openTable':
        case 'rows':
        case 'closeTable': {
          if (!printer) throw new Error('no document is open');
          if (request.op === 'nodes') printer.nodes(JSON.parse(request.json));
          else if (request.op === 'openTable') printer.openTable(JSON.parse(request.json));
          else if (request.op === 'rows') printer.rows(JSON.parse(request.json));
          else printer.closeTable();
          // The number the whole streaming design exists to keep flat, sent
          // back with every batch so the caller can watch it without a round
          // trip of its own.
          port.postMessage({ id: request.id, ok: true, pending: printer.pending });
          return;
        }
        case 'finish': {
          if (!printer) throw new Error('no document is open');
          const out = printer.finish();
          printer = null;
          if (request.path) {
            await toFile(out.pdf, request.path);
            port.postMessage({
              id: request.id,
              path: request.path,
              pages: out.pages,
              bytes: out.bytes,
              diagnostics: out.diagnostics,
            });
          } else {
            const buffer = out.pdf.buffer as ArrayBuffer;
            port.postMessage(
              { id: request.id, pdf: buffer, pages: out.pages, diagnostics: out.diagnostics },
              [buffer],
            );
          }
          await recycle();
          return;
        }
      }
    } catch (error) {
      port.postMessage({ id: request.id, error: message(error) });
    }
  });

  port.postMessage({ ready: true });
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

main().catch((error) => {
  parentPort?.postMessage({ ready: false, error: message(error) });
});
