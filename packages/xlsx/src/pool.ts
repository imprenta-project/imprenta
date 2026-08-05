/**
 * The workers, and who gets one.
 *
 * Two shapes of work go through here and they want different things:
 *
 * - a whole workbook is one message and one reply, so any free worker will do;
 * - a streamed workbook is a session that lives across many messages, and a
 *   session cannot move — so it **leases** a worker for its lifetime.
 *
 * Deliberately not shared with the page engine's pool of the same shape. A
 * page and a sheet are different models, and one pool serving both would be
 * the first place that stopped being true.
 */
import { fileURLToPath } from 'node:url';
import { Worker } from 'node:worker_threads';
import type { Request } from './worker.js';

/**
 * A request without its id, which the pool assigns.
 *
 * `Omit` over a union collapses it to the keys every member shares — which
 * here is `op` alone, so every payload field would be rejected. Distributing
 * the omit over each member keeps them.
 */
export type Job = Request extends infer T
  ? T extends { id: number }
    ? Omit<T, 'id'>
    : never
  : never;
export type { Request };

export interface PoolOptions {
  /**
   * Defaults to two.
   *
   * Not the core count, unlike the page engine's: writing a sheet is XML into
   * a zip with no shaping and no layout, so a second writer is there to keep
   * one long export from blocking a short one rather than to go faster.
   */
  size?: number;
  /** The module's bytes. Defaults to the one shipped with the package. */
  wasm?: ArrayBufferLike | ArrayBufferView;
}

interface Pending {
  resolve: (value: Record<string, unknown>) => void;
  reject: (error: Error) => void;
}

/** Every worker holds its own instance and therefore its own linear memory. */
const DEFAULT_SIZE = 2;

export class Pool {
  private readonly free: Worker[] = [];
  private readonly queue: { request: Job; pending: Pending }[] = [];
  private readonly inflight = new Map<Worker, { id: number; pending: Pending }>();
  private readonly leased = new Set<Worker>();
  /** Leases waiting for a worker to come free, in the order they asked. */
  private readonly waiting: { resolve: (w: Worker) => void; reject: (e: Error) => void }[] = [];
  private readonly all: Worker[] = [];
  private nextId = 1;
  private closed = false;

  private constructor() {}

  static async start(options: PoolOptions): Promise<Pool> {
    const size = options.size ?? DEFAULT_SIZE;
    const wasm = options.wasm ?? (await defaultWasm());
    const entry = fileURLToPath(new URL('./worker.js', import.meta.url));

    const pool = new Pool();
    await Promise.all(
      Array.from({ length: size }, async () => {
        // A copy per worker: `postMessage` with a transfer list detaches the
        // buffer, so handing the same one to the second worker hands it
        // nothing.
        const worker = new Worker(entry, {
          workerData: { wasm: copyOf(wasm) },
        });
        pool.all.push(worker);
        await ready(worker);
        pool.attach(worker);
        pool.free.push(worker);
      }),
    );
    return pool;
  }

  /** Sends one message to the first free worker and waits for its reply. */
  run(request: Job): Promise<Record<string, unknown>> {
    if (this.closed) return Promise.reject(new Error('the writer has been closed'));
    return new Promise((resolve, reject) => {
      const pending = { resolve, reject };
      const worker = this.free.pop();
      if (worker) this.dispatch(worker, request, pending);
      else this.queue.push({ request, pending });
    });
  }

  /**
   * Takes a worker out of circulation until `release` is called.
   *
   * A streamed workbook holds one for as long as it is open, which is the
   * honest cost of a session: two open exports are the most there can be at
   * once by default, and a third waits.
   */
  async lease(): Promise<Lease> {
    if (this.closed) throw new Error('the writer has been closed');
    const worker =
      this.free.pop() ??
      (await new Promise<Worker>((resolve, reject) => {
        this.waiting.push({ resolve, reject });
      }));
    this.leased.add(worker);
    return new Lease(this, worker);
  }

  /** @internal */
  send(worker: Worker, request: Job): Promise<Record<string, unknown>> {
    return new Promise((resolve, reject) => {
      this.dispatch(worker, request, { resolve, reject });
    });
  }

  /** @internal */
  release(worker: Worker): void {
    this.leased.delete(worker);
    this.handOn(worker);
  }

  /**
   * Where a worker that has just come free goes.
   *
   * Leases first, then queued jobs, then the free list. Leases first because
   * a streamed workbook that cannot start holds its caller's whole loop, while
   * a queued job is only waiting.
   */
  private handOn(worker: Worker): void {
    if (this.closed) return;
    const waiter = this.waiting.shift();
    if (waiter) {
      waiter.resolve(worker);
      return;
    }
    const next = this.queue.shift();
    if (next) this.dispatch(worker, next.request, next.pending);
    else this.free.push(worker);
  }

  get size(): number {
    return this.all.length;
  }

  async close(): Promise<void> {
    this.closed = true;
    for (const { pending } of this.queue.splice(0)) {
      pending.reject(new Error('the writer was closed before this workbook was written'));
    }
    for (const [, { pending }] of this.inflight) {
      pending.reject(new Error('the writer was closed while this workbook was being written'));
    }
    this.inflight.clear();
    for (const waiter of this.waiting.splice(0)) {
      waiter.reject(new Error('the writer has been closed'));
    }
    await Promise.all(this.all.map((w) => w.terminate()));
  }

  private attach(worker: Worker): void {
    worker.on('message', (reply: Record<string, unknown> & { id: number; error?: string }) => {
      const current = this.inflight.get(worker);
      if (!current || current.id !== reply.id) return;
      this.inflight.delete(worker);

      if (reply.error !== undefined) current.pending.reject(new Error(reply.error));
      else current.pending.resolve(reply);

      // A leased worker goes back to its lease, not to the pool.
      if (this.leased.has(worker)) return;
      this.handOn(worker);
    });

    // A worker that dies takes its document with it, and there is no honest
    // way to say how far it got.
    worker.on('error', (error) => {
      const current = this.inflight.get(worker);
      this.inflight.delete(worker);
      current?.pending.reject(error);
    });
  }

  private dispatch(worker: Worker, request: Job, pending: Pending): void {
    const id = this.nextId++;
    this.inflight.set(worker, { id, pending });
    worker.postMessage({ ...request, id });
  }
}

/** One worker, held for the life of a streamed workbook. */
export class Lease {
  private open = true;
  private busy = false;

  constructor(
    private readonly pool: Pool,
    private readonly worker: Worker,
  ) {}

  /**
   * One call in flight at a time, and a second is refused rather than queued.
   *
   * A stream is read in order and two promises in flight have no order at all,
   * so a caller that forgot to await would silently interleave batches. It is
   * also what stops an unawaited loop piling the whole ledger into the queue —
   * which would put the document back in memory by the one route streaming
   * exists to avoid. A rejection, never a silent queue.
   */
  async send(request: Job): Promise<Record<string, unknown>> {
    if (!this.open) throw new Error('this workbook has already been finished');
    if (this.busy) {
      throw new Error('await the previous call before making another on this workbook');
    }
    this.busy = true;
    try {
      return await this.pool.send(this.worker, request);
    } finally {
      this.busy = false;
    }
  }

  release(): void {
    if (!this.open) return;
    this.open = false;
    this.pool.release(this.worker);
  }
}

function ready(worker: Worker): Promise<void> {
  return new Promise((resolve, reject) => {
    worker.once('message', (m: { ready: boolean; error?: string }) => {
      if (m.ready) resolve();
      else reject(new Error(m.error ?? 'a writer failed to start'));
    });
    worker.once('error', reject);
  });
}

function copyOf(source: ArrayBufferLike | ArrayBufferView): ArrayBuffer {
  const view = ArrayBuffer.isView(source)
    ? new Uint8Array(source.buffer, source.byteOffset, source.byteLength)
    : new Uint8Array(source);
  return view.slice().buffer as ArrayBuffer;
}

async function defaultWasm(): Promise<Uint8Array> {
  const { readFile } = await import('node:fs/promises');
  return new Uint8Array(
    await readFile(fileURLToPath(new URL('../imprenta-xlsx.wasm', import.meta.url))),
  );
}
