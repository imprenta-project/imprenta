import { check, compile, type Exports, instantiate, Memory, type WasmSource } from './module.js';
import type { ImageAsset, RenderResult } from './types.js';

export { compile, EngineError, type WasmSource } from './module.js';
export interface EngineOptions {
  wasm?: WasmSource;
}
/** One reusable synchronous WASM instance, for a worker, CLI or browser. */
export class Engine {
  private readonly memory: Memory;
  private constructor(private readonly e: Exports) {
    this.memory = new Memory(e);
  }
  static async load(options: EngineOptions = {}): Promise<Engine> {
    return new Engine(await instantiate(await compile(options.wasm ?? (await defaultWasm()))));
  }
  render(ir: string | Uint8Array, images: ImageAsset[] = []): RenderResult {
    check(this.e, this.memory, this.e.imprenta_assets_reset());
    for (const image of images) {
      const name = this.memory.writeText(image.name);
      const data = this.memory.write(image.data);
      try {
        check(this.e, this.memory, this.e.imprenta_assets_image(...name, ...data));
      } finally {
        this.memory.free(name);
        this.memory.free(data);
      }
    }
    const input = typeof ir === 'string' ? this.memory.writeText(ir) : this.memory.write(ir);
    try {
      check(this.e, this.memory, this.e.imprenta_render(...input));
      const escpos = this.memory.read(this.e.imprenta_out_ptr(), this.e.imprenta_out_len());
      const metadata = JSON.parse(
        this.memory.readText(this.e.imprenta_meta_ptr(), this.e.imprenta_meta_len()),
      ) as Omit<RenderResult, 'escpos' | 'bytes'>;
      return { escpos, bytes: escpos.length, ...metadata };
    } finally {
      this.memory.free(input);
      this.e.imprenta_out_release();
    }
  }
  get memoryBytes(): number {
    return this.e.memory.buffer.byteLength;
  }
}
export async function defaultWasm(): Promise<Uint8Array<ArrayBuffer>> {
  if (typeof process === 'undefined' || !process.versions?.node)
    throw new Error('pass wasm bytes to load the receipt engine outside Node');
  const { readFile } = await import('node:fs/promises');
  return new Uint8Array(await readFile(new URL('../imprenta-escpos.wasm', import.meta.url)));
}
