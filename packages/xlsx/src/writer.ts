/**
 * The spreadsheet writer, as one WebAssembly instance.
 *
 * Synchronous, and blocks the thread it is called on. That is right inside a
 * worker, a CLI or the browser, and wrong on a server's main thread — which is
 * what `index.ts` and its pool are for.
 */
import { check, compile, type Exports, instantiate, Memory, type WasmSource } from './module.js';

export { compile, EngineError, type WasmSource } from './module.js';

export interface WriterOptions {
  /**
   * The module, or the bytes of it.
   *
   * Optional in Node, where the package's own `imprenta-xlsx.wasm` is read
   * from disk. Everywhere else it has to be supplied, because how bytes arrive
   * is the one thing a portable module cannot decide for itself.
   */
  wasm?: WasmSource;
}

export interface WriteOutcome {
  xlsx: Uint8Array;
  /** Size of the workbook in bytes. */
  bytes: number;
  sheets: number;
}

/** A sheet as it will appear, without its rows. */
export interface SheetSetup {
  name?: string;
  columns?: unknown[];
  freeze?: unknown;
  rows?: unknown[];
}

export class Writer {
  private readonly memory: Memory;

  private constructor(private readonly e: Exports) {
    this.memory = new Memory(e);
  }

  static async load(options: WriterOptions = {}): Promise<Writer> {
    const source = options.wasm ?? (await defaultWasm());
    return new Writer(await instantiate(await compile(source)));
  }

  /**
   * Writes a declared workbook.
   *
   * The workbook crosses as JSON — the same shape the engine has always taken,
   * and what comes back from a file, a queue or an HTTP body.
   */
  write(ir: string | Uint8Array): WriteOutcome {
    const input = typeof ir === 'string' ? this.memory.writeText(ir) : this.memory.write(ir);
    try {
      check(this.e, this.memory, this.e.imprenta_write(input[0], input[1]));
      return this.collect();
    } finally {
      this.memory.free(input);
    }
  }

  /** Begins a workbook whose rows will arrive in batches. */
  book(sheets: SheetSetup[]): SyncBook {
    const input = this.memory.writeText(JSON.stringify(sheets));
    try {
      check(this.e, this.memory, this.e.imprenta_book_open(input[0], input[1]));
    } finally {
      this.memory.free(input);
    }
    return new SyncBook(this.e, this.memory, () => this.collect());
  }

  /** How much linear memory the instance holds. It only ever grows. */
  get memoryBytes(): number {
    return this.e.memory.buffer.byteLength;
  }

  private collect(): WriteOutcome {
    const xlsx = this.memory.read(this.e.imprenta_out_ptr(), this.e.imprenta_out_len());
    const sheets = this.e.imprenta_out_sheets();
    // Released as soon as it is read: WebAssembly memory never goes back to
    // the host, so an instance that kept its last workbook would hold the
    // largest one it ever made for as long as it lived.
    this.e.imprenta_out_release();
    return { xlsx, bytes: xlsx.length, sheets };
  }
}

/**
 * A workbook being fed, on this thread.
 *
 * Unlike the native binding there is no promise to await and no call that can
 * be refused for arriving out of order: one call into a module cannot
 * interleave with another, so the ordering is structural.
 */
export class SyncBook {
  private open = true;

  constructor(
    private readonly e: Exports,
    private readonly memory: Memory,
    private readonly collect: () => WriteOutcome,
  ) {}

  rows(rows: unknown[]): void {
    this.send(this.e.imprenta_book_rows, JSON.stringify(rows));
  }

  row(row: unknown): void {
    this.rows([row]);
  }

  /** Closes the open sheet and opens the next one that was declared. */
  nextSheet(): void {
    this.ensureOpen();
    check(this.e, this.memory, this.e.imprenta_book_next_sheet());
  }

  /**
   * Merges a block of the open sheet.
   *
   * Rows and columns count from the top of the sheet, not from where the
   * current batch happens to start — which is why {@link at} exists.
   */
  merge(merge: unknown): void {
    this.send(this.e.imprenta_book_merge, JSON.stringify(merge));
  }

  /** How many rows have gone into the open sheet. The next one is this. */
  get at(): number {
    return this.e.imprenta_book_row();
  }

  finish(): WriteOutcome {
    this.ensureOpen();
    this.open = false;
    check(this.e, this.memory, this.e.imprenta_book_finish());
    return this.collect();
  }

  private send(call: (ptr: number, len: number) => number, json: string): void {
    this.ensureOpen();
    const input = this.memory.writeText(json);
    try {
      check(this.e, this.memory, call.call(this.e, input[0], input[1]));
    } finally {
      this.memory.free(input);
    }
  }

  private ensureOpen(): void {
    if (!this.open) throw new Error('this workbook has already been finished');
  }
}

async function defaultWasm(): Promise<Uint8Array> {
  const isNode = typeof process !== 'undefined' && process.versions?.node != null;
  if (!isNode) {
    throw new Error(
      'pass `wasm` with the module bytes: only Node can read imprenta-xlsx.wasm from disk',
    );
  }
  const { readFile } = await import('node:fs/promises');
  const { fileURLToPath } = await import('node:url');
  return new Uint8Array(
    await readFile(fileURLToPath(new URL('../imprenta-xlsx.wasm', import.meta.url))),
  );
}
