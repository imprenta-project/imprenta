/**
 * Imprenta's PDF engine as a WebAssembly module.
 *
 * One artefact for every runtime. The same file runs in Node, a browser, Deno,
 * Bun and on an edge worker, because the module imports nothing at all — there
 * is no Node-API, no WASI and no glue layer for a host to have to provide.
 *
 * # This blocks the thread it is called on
 *
 * A WebAssembly call is synchronous and a long document takes seconds, so an
 * [`Engine`] used on a server's main thread stops that server answering. That
 * is not a wart to work around later: it is why {@link "@imprentajs/wasm/pool"}
 * exists, and a service should reach for the pool. Use `Engine` directly when
 * you are already off the main thread — inside a worker, in a CLI, in a build
 * step — or when the documents are small enough that it does not matter.
 */
import {
  check,
  compile,
  EngineError,
  type Exports,
  instantiate,
  Memory,
  type WasmSource,
} from './module.js';

export { compile, EngineError, type WasmSource } from './module.js';

/** A typeface the document may ask for, and the file behind it. */
export interface Font {
  /** Defaults to regular. */
  weight?: 'regular' | 'bold';
  italic?: boolean;
  data: Uint8Array;
}

/** An image the document refers to by name. */
export interface Image {
  name: string;
  data: Uint8Array;
}

export interface EngineOptions {
  fonts: Font[];
  images?: Image[];
  /**
   * The module, or the bytes of it.
   *
   * Optional in Node, where the package's own `imprenta-pdf.wasm` is read from
   * disk. Everywhere else it has to be supplied, because how bytes arrive is
   * the one thing a portable module cannot decide for itself — `fetch`, a
   * bundler's `?url`, an embedded base64, a KV store.
   */
  wasm?: WasmSource;
}

export interface RenderResult {
  pdf: Uint8Array;
  pages: number;
  /** Size of the PDF in bytes, the same as `pdf.length`. */
  bytes: number;
  /** Anything the engine noticed — clipped text, a character no font covers. */
  diagnostics: string[];
}

/** The page a document is set on, as the IR declares it. */
export interface PageSetup {
  width: number;
  height: number;
  margin?: unknown;
}

export interface PrinterSetup {
  page: PageSetup;
  header?: unknown;
  footer?: unknown;
  /** Names of the running totals, in the order a band refers to them. */
  accumulators?: string[];
}

/**
 * One instance of the engine, with its fonts already loaded.
 *
 * The fonts are copied into the module once, here, rather than on every
 * render: an instance kept warm for a queue of documents would otherwise hand
 * itself the same typeface thousands of times.
 */
export class Engine {
  private readonly memory: Memory;

  private constructor(private readonly e: Exports) {
    this.memory = new Memory(e);
  }

  static async load(options: EngineOptions): Promise<Engine> {
    const source = options.wasm ?? (await defaultWasm());
    const exports = await instantiate(await compile(source));
    const engine = new Engine(exports);
    engine.loadAssets(options);
    return engine;
  }

  /**
   * Renders a declared document.
   *
   * The document crosses as JSON, exactly as it does through the native
   * addon, so the two bindings cannot describe a document differently. Pass a
   * string and it is encoded here; pass bytes if you already have them, which
   * is what arrives from a file, a queue or an HTTP body.
   */
  render(ir: string | Uint8Array): RenderResult {
    const input = typeof ir === 'string' ? this.memory.writeText(ir) : this.memory.write(ir);
    try {
      check(this.e, this.memory, this.e.imprenta_render(input[0], input[1]));
      return this.collect();
    } finally {
      this.memory.free(input);
    }
  }

  /**
   * Measures a run of table rows and hands back one height each.
   *
   * The first pass of a sharded render — see `shard.ts`. Bytes rather than
   * numbers because this is the thing that crosses between engines, and four
   * bytes a row is the whole point of measuring separately from painting.
   */
  measureRows(setup: string, head: string, rows: string): Uint8Array {
    const s = this.memory.writeText(setup);
    const h = this.memory.writeText(head);
    const r = this.memory.writeText(rows);
    try {
      check(this.e, this.memory, this.e.imprenta_measure_rows(s[0], s[1], h[0], h[1], r[0], r[1]));
      const out = this.memory.read(this.e.imprenta_out_ptr(), this.e.imprenta_out_len());
      this.e.imprenta_out_release();
      return out;
    } finally {
      this.memory.free(s);
      this.memory.free(h);
      this.memory.free(r);
    }
  }

  /**
   * Paints a run of the rows this engine measured, as a fragment.
   *
   * `extra` is the tail the fragment's last page needs and this engine never
   * measured — a page's worth at most, because a fragment is cut on a page
   * boundary and only that one page straddles the seam.
   */
  fragmentMeasured(
    setup: string,
    head: string,
    from: number,
    to: number,
    extra: string,
  ): RenderResult {
    const s = this.memory.writeText(setup);
    const h = this.memory.writeText(head);
    const e = extra ? this.memory.writeText(extra) : ([0, 0] as [number, number]);
    try {
      check(
        this.e,
        this.memory,
        this.e.imprenta_fragment_measured(s[0], s[1], h[0], h[1], from, to, e[0], e[1]),
      );
      const out = this.collect();
      // The rows are the largest thing an instance holds, and it has just
      // finished with them.
      this.e.imprenta_measured_release();
      return out;
    } finally {
      this.memory.free(s);
      this.memory.free(h);
      this.memory.free(e);
    }
  }

  /** Packs measured heights and says where the pages fall. */
  plan(setup: string, heights: Uint8Array): string {
    const s = this.memory.writeText(setup);
    const h = this.memory.write(heights);
    try {
      check(this.e, this.memory, this.e.imprenta_plan(s[0], s[1], h[0], h[1]));
      const out = this.memory.readText(this.e.imprenta_out_ptr(), this.e.imprenta_out_len());
      this.e.imprenta_out_release();
      return out;
    } finally {
      this.memory.free(s);
      this.memory.free(h);
    }
  }

  /** Puts the fragments of a sharded render into one file. */
  merge(fragments: Uint8Array[]): RenderResult {
    check(this.e, this.memory, this.e.imprenta_merge_reset());
    for (const fragment of fragments) {
      const held = this.memory.write(fragment);
      try {
        check(this.e, this.memory, this.e.imprenta_merge_push(held[0], held[1]));
      } finally {
        this.memory.free(held);
      }
    }
    check(this.e, this.memory, this.e.imprenta_merge_finish());
    return this.collect();
  }

  /** Begins a document that will arrive in pieces. */
  printer(setup: PrinterSetup): Printer {
    const input = this.memory.writeText(JSON.stringify(setup));
    try {
      check(this.e, this.memory, this.e.imprenta_stream_open(input[0], input[1]));
    } finally {
      this.memory.free(input);
    }
    return new Printer(this.e, this.memory, () => this.collect());
  }

  /**
   * How much linear memory the instance is holding, in bytes.
   *
   * Worth being able to look at, because WebAssembly memory only ever grows:
   * an instance's footprint is the high-water mark of every document it has
   * rendered, not what it is holding now. A pool that renders one very large
   * document among small ones keeps that peak until the instance is dropped.
   */
  get memoryBytes(): number {
    return this.e.memory.buffer.byteLength;
  }

  private loadAssets(options: EngineOptions): void {
    // Refused here rather than at the first render. A document cannot be set
    // without a typeface, and finding that out forty seconds into a ledger —
    // or worse, on the one request a month that has no cached engine — is a
    // failure that costs far more than the check.
    if (options.fonts.length === 0) {
      throw new EngineError('no fonts were supplied, and a document cannot be set without one');
    }
    check(this.e, this.memory, this.e.imprenta_assets_reset());
    for (const font of options.fonts) {
      const weight = this.memory.writeText(font.weight ?? 'regular');
      const data = this.memory.write(font.data);
      try {
        check(
          this.e,
          this.memory,
          this.e.imprenta_assets_font(weight[0], weight[1], font.italic ? 1 : 0, data[0], data[1]),
        );
      } finally {
        this.memory.free(weight);
        this.memory.free(data);
      }
    }
    for (const image of options.images ?? []) {
      const name = this.memory.writeText(image.name);
      const data = this.memory.write(image.data);
      try {
        check(
          this.e,
          this.memory,
          this.e.imprenta_assets_image(name[0], name[1], data[0], data[1]),
        );
      } finally {
        this.memory.free(name);
        this.memory.free(data);
      }
    }
  }

  /** Reads the finished document and gives the module its bytes back. */
  private collect(): RenderResult {
    const pdf = this.memory.read(this.e.imprenta_out_ptr(), this.e.imprenta_out_len());
    const pages = this.e.imprenta_out_pages();
    const reported = this.memory.readText(
      this.e.imprenta_diagnostics_ptr(),
      this.e.imprenta_diagnostics_len(),
    );
    const diagnostics: string[] = reported ? JSON.parse(reported) : [];
    // Released as soon as it is read: an instance that kept its last PDF would
    // hold the largest one it ever made for as long as it lived, and
    // WebAssembly memory is never returned to the host.
    this.e.imprenta_out_release();
    return { pdf, pages, bytes: pdf.length, diagnostics };
  }
}

/**
 * A document fed to the engine a piece at a time.
 *
 * What the caller never holds is the point: a ledger's rows go past in
 * batches, so the largest thing in the JS heap is one batch rather than the
 * document. The pages that come out are byte for byte the pages `render`
 * would have produced from the same content declared whole.
 *
 * Unlike the native binding there is no promise to await and no call that can
 * be refused for arriving out of order. A WebAssembly call cannot interleave
 * with another, so the ordering the native side has to enforce at run time is
 * structural here.
 */
export class Printer {
  private open = true;

  constructor(
    private readonly e: Exports,
    private readonly memory: Memory,
    private readonly collect: () => RenderResult,
  ) {}

  /** Adds a batch of nodes — headings, paragraphs, whole short tables. */
  nodes(nodes: unknown[]): void {
    this.send(this.e.imprenta_stream_nodes, JSON.stringify(nodes));
  }

  /** Adds one node. Convenience for the times there is only one. */
  node(node: unknown): void {
    this.nodes([node]);
  }

  /** Begins a table. Its rows follow, in as many batches as suits you. */
  openTable(head: unknown): void {
    this.send(this.e.imprenta_stream_open_table, JSON.stringify(head));
  }

  /**
   * Adds a batch of rows.
   *
   * Batch size is a memory-against-overhead trade and nothing else. There is
   * no thread hop here as there is in the native binding, so small batches
   * cost less than they do there — but each one is still a JSON encode, and a
   * few hundred to a thousand rows is the range worth being in.
   */
  rows(rows: unknown[]): void {
    this.send(this.e.imprenta_stream_rows, JSON.stringify(rows));
  }

  closeTable(): void {
    this.ensureOpen();
    check(this.e, this.memory, this.e.imprenta_stream_close_table());
  }

  /** Atoms the engine is still holding: about a page's worth, always. */
  get pending(): number {
    return this.e.imprenta_stream_pending();
  }

  /** Paints what is left and closes the document. */
  finish(): RenderResult {
    this.ensureOpen();
    this.open = false;
    check(this.e, this.memory, this.e.imprenta_stream_finish());
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
    if (!this.open) {
      throw new Error('this document has already been finished');
    }
  }
}

/**
 * The module that ships with this package.
 *
 * Node only, and only as a convenience: everywhere else the bytes have to be
 * supplied, because a portable module cannot know whether they arrive by
 * `fetch`, from a bundler or out of a KV store. Imported lazily so a bundler
 * targeting the browser never has to resolve `node:fs`.
 */
async function defaultWasm(): Promise<Uint8Array> {
  const isNode = typeof process !== 'undefined' && process.versions?.node != null;
  if (!isNode) {
    throw new Error(
      'pass `wasm` with the module bytes: only Node can read imprenta-pdf.wasm from disk',
    );
  }
  const { readFile } = await import('node:fs/promises');
  const { fileURLToPath } = await import('node:url');
  return new Uint8Array(
    await readFile(fileURLToPath(new URL('../imprenta-pdf.wasm', import.meta.url))),
  );
}
