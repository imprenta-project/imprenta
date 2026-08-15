/**
 * The ABI, as JavaScript sees it.
 *
 * Everything the module exports is a number in and a number out; anything
 * larger travels through linear memory. This file is the only place that
 * knows that, so the rest of the package can speak in documents and fonts.
 */

/** What the crate exports. Nothing here is optional: a module missing one of
 * these is not this module, and failing at load time beats failing mid-render. */
export interface Exports {
  memory: WebAssembly.Memory;
  imprenta_alloc(len: number): number;
  imprenta_dealloc(ptr: number, len: number): void;
  imprenta_assets_reset(): number;
  imprenta_assets_image(namePtr: number, nameLen: number, dataPtr: number, dataLen: number): number;
  imprenta_write(irPtr: number, irLen: number): number;
  imprenta_book_open(ptr: number, len: number): number;
  imprenta_book_rows(ptr: number, len: number): number;
  imprenta_book_merge(ptr: number, len: number): number;
  imprenta_book_next_sheet(): number;
  imprenta_book_row(): number;
  imprenta_book_finish(): number;
  imprenta_out_sheets(): number;
  imprenta_out_ptr(): number;
  imprenta_out_len(): number;
  imprenta_out_release(): number;
  imprenta_error_ptr(): number;
  imprenta_error_len(): number;
}

const REQUIRED = [
  'memory',
  'imprenta_alloc',
  'imprenta_dealloc',
  'imprenta_assets_reset',
  'imprenta_assets_image',
  'imprenta_write',
  'imprenta_book_open',
  'imprenta_book_rows',
  'imprenta_book_merge',
  'imprenta_book_next_sheet',
  'imprenta_book_row',
  'imprenta_book_finish',
  'imprenta_out_ptr',
  'imprenta_out_len',
  'imprenta_out_sheets',
  'imprenta_out_release',
  'imprenta_error_ptr',
  'imprenta_error_len',
] as const;

/** Bytes, a compiled module, or anything `WebAssembly.compile` accepts. */
export type WasmSource = BufferSource | WebAssembly.Module;

export async function compile(source: WasmSource): Promise<WebAssembly.Module> {
  if (source instanceof WebAssembly.Module) return source;
  return WebAssembly.compile(source);
}

/**
 * The module takes no imports at all, which is what lets one artefact run in
 * Node, a browser, Deno, Bun and on an edge runtime with no shim between.
 * Passing an empty import object is not a simplification here — it is the
 * whole contract, and `test/module.test.ts` asserts it stays that way.
 */
export async function instantiate(module: WebAssembly.Module): Promise<Exports> {
  const instance = await WebAssembly.instantiate(module, {});
  for (const name of REQUIRED) {
    if (!(name in instance.exports)) {
      throw new Error(
        `the WebAssembly module is missing ${name}; it is not an Imprenta spreadsheet writer`,
      );
    }
  }
  return unsigned(instance.exports);
}

/**
 * Every export is wrapped so its result is read as an unsigned 32-bit value.
 *
 * A wasm32 pointer crosses as an `i32` and JavaScript reads an `i32` as
 * signed, so once linear memory grows past 2 GiB every pointer comes back
 * negative — and the next line uses it as an offset, which throws an error
 * about bounds that reads like corruption. Wrapping here rather than at each
 * call site is deliberate: a future export cannot forget. Everything the
 * module returns is a pointer, a length, a count or a flag, none of which can
 * meaningfully be negative.
 */
function unsigned(exports: WebAssembly.Exports): Exports {
  const wrapped: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(exports)) {
    wrapped[name] =
      typeof value === 'function'
        ? (...args: number[]) => (value as (...args: number[]) => number)(...args) >>> 0
        : value;
  }
  return wrapped as unknown as Exports;
}

/**
 * Reading and writing the module's memory.
 *
 * A fresh view every time and never a cached one: `alloc` can grow the memory,
 * and growing it detaches every `ArrayBuffer` handed out before. A view kept
 * across a call is the kind of bug that works until a document gets big.
 */
export class Memory {
  constructor(private readonly e: Exports) {}

  /** Copies `data` into memory the module owns. Give it back with `free`. */
  write(data: Uint8Array): [ptr: number, len: number] {
    if (data.length === 0) return [0, 0];
    const ptr = this.e.imprenta_alloc(data.length);
    new Uint8Array(this.e.memory.buffer).set(data, ptr);
    return [ptr, data.length];
  }

  writeText(text: string): [ptr: number, len: number] {
    return this.write(encoder.encode(text));
  }

  free([ptr, len]: [number, number]): void {
    if (len > 0) this.e.imprenta_dealloc(ptr, len);
  }

  /**
   * Copies bytes out.
   *
   * A copy rather than a view over linear memory, and deliberately: a view
   * stays valid only until the next allocation grows the memory, and a caller
   * holding a PDF has every reason to keep it longer than that. The copy is
   * one pass over bytes the engine has already spent milliseconds producing.
   */
  read(ptr: number, len: number): Uint8Array {
    return new Uint8Array(this.e.memory.buffer, ptr, len).slice();
  }

  readText(ptr: number, len: number): string {
    if (len === 0) return '';
    return decoder.decode(new Uint8Array(this.e.memory.buffer, ptr, len));
  }
}

const encoder = new TextEncoder();
const decoder = new TextDecoder();

/** The engine's own account of why a call returned 0. */
export class EngineError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'EngineError';
  }
}

/** Turns the ABI's `0` into the message the engine left behind. */
export function check(e: Exports, memory: Memory, ok: number): void {
  if (ok !== 0) return;
  const message = memory.readText(e.imprenta_error_ptr(), e.imprenta_error_len());
  throw new EngineError(message || 'the engine failed without saying why');
}
