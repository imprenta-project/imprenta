import { Pool } from './pool.js';
import type { Receipt, RenderOptions, RenderResult } from './types.js';

let running: Promise<Pool> | null = null;

async function engines(options: RenderOptions): Promise<Pool> {
  if (!running) {
    running = Pool.start(options);
    running.catch(() => {
      running = null;
    });
  }
  const pool = await running;
  if (pool.closed) {
    running = null;
    return engines(options);
  }
  return pool;
}

/** Rust/WASM on worker threads, like the other document engines. */
export async function render(
  input: Receipt | string | Uint8Array,
  options: RenderOptions = {},
): Promise<RenderResult> {
  let ir: string | Uint8Array;
  if (options.profile) {
    const receipt = parseReceipt(input);
    ir = serialize({ ...receipt, profile: { ...receipt.profile, ...options.profile } });
  } else {
    ir = typeof input === 'string' || input instanceof Uint8Array ? input : serialize(input);
  }
  return (await engines(options)).run(ir, options.images);
}

export async function close(): Promise<void> {
  const pool = running;
  running = null;
  if (pool) await pool.then((pool) => pool.close()).catch(() => undefined);
}

function parseReceipt(input: Receipt | string | Uint8Array): Receipt {
  if (typeof input === 'string') return JSON.parse(input);
  if (input instanceof Uint8Array) return JSON.parse(new TextDecoder().decode(input));
  return input;
}

function serialize(input: unknown): string {
  return JSON.stringify(input, (key, value) => {
    if (typeof value === 'number' && !Number.isFinite(value))
      throw new Error(`${key} must be a finite number`);
    return value;
  });
}
