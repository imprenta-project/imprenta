import { parentPort, workerData } from 'node:worker_threads';
import { Engine } from './engine.js';
import type { ImageAsset } from './types.js';
export interface Request {
  id: number;
  ir: string | Uint8Array;
  images?: ImageAsset[];
}

async function main(): Promise<void> {
  const port = parentPort;
  if (!port) throw new Error('the receipt worker needs a parent port');
  const engine = await Engine.load({ wasm: workerData.wasm });
  port.on('message', (request: Request) => {
    try {
      const result = engine.render(request.ir, request.images);
      const bytes = result.escpos.buffer as ArrayBuffer;
      port.postMessage({ id: request.id, result }, [bytes]);
    } catch (error) {
      port.postMessage({
        id: request.id,
        error: error instanceof Error ? error.message : String(error),
      });
    }
  });
  port.postMessage({ ready: true });
}
main().catch((error) =>
  parentPort?.postMessage({
    ready: false,
    error: error instanceof Error ? error.message : String(error),
  }),
);
