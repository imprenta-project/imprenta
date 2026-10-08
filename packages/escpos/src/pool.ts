import { Worker } from 'node:worker_threads';
import { defaultWasm, EngineError } from './engine.js';
import type { ImageAsset, RenderOptions, RenderResult } from './types.js';
import type { Request } from './worker.js';

interface Pending {
  resolve(result: RenderResult): void;
  reject(error: Error): void;
}
interface Job {
  request: Request;
  pending: Pending;
}

/** One independent linear memory per worker; no printer state crosses jobs. */
export class Pool {
  private readonly all: Worker[] = [];
  private readonly free: Worker[] = [];
  private readonly queue: Job[] = [];
  private readonly inflight = new Map<Worker, Job>();
  private nextId = 1;
  closed = false;

  static async start(options: RenderOptions = {}): Promise<Pool> {
    const size = options.size ?? 2;
    if (!Number.isInteger(size) || size < 1 || size > 128)
      throw new Error('size must be an integer between 1 and 128');
    const wasm = options.wasm ?? (await defaultWasm());
    const pool = new Pool();
    try {
      // Await all startup attempts before cleanup, so a late worker cannot escape it.
      const started = await Promise.allSettled(
        Array.from({ length: size }, async () => {
          const worker = new Worker(new URL('./worker.js', import.meta.url), {
            workerData: { wasm },
          });
          pool.all.push(worker);
          await waitForReady(worker);
          pool.attach(worker);
          pool.free.push(worker);
          worker.unref();
        }),
      );
      for (const result of started) if (result.status === 'rejected') throw result.reason;
      return pool;
    } catch (error) {
      await pool.close();
      throw error;
    }
  }

  run(ir: string | Uint8Array, images?: ImageAsset[]): Promise<RenderResult> {
    if (this.closed) return Promise.reject(new Error('the receipt pool has been closed'));
    return new Promise((resolve, reject) => {
      const job: Job = { request: { id: this.nextId++, ir, images }, pending: { resolve, reject } };
      const worker = this.free.pop();
      if (worker) this.dispatch(worker, job);
      else this.queue.push(job);
    });
  }

  private attach(worker: Worker): void {
    worker.on('message', (reply: { id: number; result?: RenderResult; error?: string }) => {
      const job = this.inflight.get(worker);
      if (!job || job.request.id !== reply.id) return;
      this.inflight.delete(worker);
      if (reply.error !== undefined) job.pending.reject(new EngineError(reply.error));
      else if (reply.result) job.pending.resolve(reply.result);
      else job.pending.reject(new Error('receipt worker returned no result'));
      if (this.closed) return;
      const next = this.queue.shift();
      if (next) this.dispatch(worker, next);
      else {
        this.free.push(worker);
        worker.unref();
      }
    });
    const failed = (error: Error) => {
      if (this.closed) return;
      this.reject(error);
      void this.close();
    };
    worker.on('error', failed);
    worker.on('exit', (code) => failed(new Error(`receipt worker exited (${code})`)));
  }

  private dispatch(worker: Worker, job: Job): void {
    worker.ref();
    this.inflight.set(worker, job);
    try {
      worker.postMessage(job.request);
    } catch (error) {
      this.inflight.delete(worker);
      this.free.push(worker);
      worker.unref();
      job.pending.reject(error instanceof Error ? error : new Error(String(error)));
    }
  }

  private reject(error: Error): void {
    for (const job of this.queue.splice(0)) job.pending.reject(error);
    for (const job of this.inflight.values()) job.pending.reject(error);
    this.inflight.clear();
  }

  async close(): Promise<void> {
    if (this.closed) return;
    this.closed = true;
    this.reject(new Error('the receipt pool has been closed'));
    this.free.length = 0;
    await Promise.all(this.all.map((worker) => worker.terminate()));
  }
}

/** Startup messages are handled before the worker receives any render jobs. */
function waitForReady(worker: Worker): Promise<void> {
  return new Promise((resolve, reject) => {
    const onError = (error: Error) => reject(error);
    const onExit = (code: number) =>
      reject(new Error(`receipt worker exited during startup (${code})`));

    worker.once('error', onError);
    worker.once('exit', onExit);
    worker.once('message', (reply: { ready?: boolean; error?: string }) => {
      worker.off('error', onError);
      worker.off('exit', onExit);
      if (reply.ready) resolve();
      else reject(new Error(reply.error ?? 'receipt worker failed to start'));
    });
  });
}
