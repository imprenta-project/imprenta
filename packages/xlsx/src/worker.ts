/**
 * One writer, on its own thread.
 *
 * A WebAssembly call is synchronous, and a million-row export takes seconds:
 * on the thread that answers requests that stops a service answering. The
 * native binding met that with libuv's pool; this meets it with a worker,
 * which is the same promise to the caller.
 *
 * Deliberately dumb — it owns a [`Writer`] and answers messages. Every
 * decision about how many of these there are lives in `pool.ts`.
 */
import { parentPort, workerData } from 'node:worker_threads';
import type { Image } from './writer.js';
import { type SyncBook, Writer } from './writer.js';

export interface BootData {
  wasm: ArrayBuffer;
}

/** Everything the pool can ask for. One message, one reply, always. */
export type Request =
  | { id: number; op: 'write'; ir: string; images?: Image[] }
  | { id: number; op: 'writeToFile'; ir: string; path: string; images?: Image[] }
  | { id: number; op: 'open'; sheets: string; images?: Image[] }
  | { id: number; op: 'rows'; json: string }
  | { id: number; op: 'merge'; json: string }
  | { id: number; op: 'nextSheet' }
  | { id: number; op: 'finish'; path?: string };

async function main(): Promise<void> {
  const port = parentPort;
  if (!port) throw new Error('this module is only meaningful inside a worker');
  const boot = workerData as BootData;

  const writer = await Writer.load({ wasm: new Uint8Array(boot.wasm) });
  let book: SyncBook | null = null;

  /**
   * Written from here rather than handed back, so a large export never
   * becomes a buffer in the calling thread's heap on its way to disk.
   */
  const toFile = async (bytes: Uint8Array, path: string) => {
    const { writeFile } = await import('node:fs/promises');
    await writeFile(path, bytes);
  };

  const sendBytes = (id: number, out: { xlsx: Uint8Array; sheets: number }) => {
    // Transferred, not cloned: the workbook is the largest thing that crosses.
    const buffer = out.xlsx.buffer as ArrayBuffer;
    port.postMessage({ id, xlsx: buffer, sheets: out.sheets }, [buffer]);
  };

  port.on('message', async (request: Request) => {
    try {
      switch (request.op) {
        case 'write':
          sendBytes(request.id, writer.write(request.ir, request.images));
          return;
        case 'writeToFile': {
          const out = writer.write(request.ir, request.images);
          await toFile(out.xlsx, request.path);
          port.postMessage({
            id: request.id,
            path: request.path,
            bytes: out.bytes,
            sheets: out.sheets,
          });
          return;
        }
        case 'open':
          book = writer.book(JSON.parse(request.sheets), request.images);
          port.postMessage({ id: request.id, ok: true });
          return;
        case 'rows':
        case 'merge':
        case 'nextSheet': {
          if (!book) throw new Error('no workbook is open');
          if (request.op === 'rows') book.rows(JSON.parse(request.json));
          else if (request.op === 'merge') book.merge(JSON.parse(request.json));
          else book.nextSheet();
          port.postMessage({ id: request.id, ok: true, at: book.at });
          return;
        }
        case 'finish': {
          if (!book) throw new Error('no workbook is open');
          const out = book.finish();
          book = null;
          if (request.path) {
            await toFile(out.xlsx, request.path);
            port.postMessage({
              id: request.id,
              path: request.path,
              bytes: out.bytes,
              sheets: out.sheets,
            });
          } else {
            sendBytes(request.id, out);
          }
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
